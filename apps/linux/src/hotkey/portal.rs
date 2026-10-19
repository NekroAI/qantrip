//! One cancellable task owns one Portal connection, session and signal subscription.
//! A private connection also closes requests whose CreateSession reply never arrived.

use std::rc::{Rc, Weak};
use std::time::Duration;

use ashpd::desktop::global_shortcuts::{Activated, Deactivated, GlobalShortcuts, NewShortcut, ShortcutsChanged};
use ashpd::desktop::{ResponseError, Session};
use ashpd::zbus;
use futures_util::future::{Either, select};
use futures_util::{Future, StreamExt};
use gtk::glib;

use super::{Capabilities, Capability, Failure, HotKey, Phase, Press, SHORTCUT_ID, ShortcutId, Source, State};

pub(super) struct Task {
    cancel: async_channel::Sender<()>,
    configure: async_channel::Sender<()>,
    done: async_channel::Receiver<()>,
}

impl Task {
    pub fn start(owner: Weak<HotKey>, generation: u64, accel: String, previous: Option<Task>) -> Self {
        Self::spawn(owner, generation, super::portal_trigger(&accel).ok_or(Failure::InvalidShortcut), previous, connect())
    }

    fn spawn(
        owner: Weak<HotKey>,
        generation: u64,
        trigger: Result<String, Failure>,
        previous: Option<Task>,
        connection: impl Future<Output = Result<zbus::Connection, Failure>> + 'static,
    ) -> Self {
        let (cancel, cancelled) = async_channel::bounded(1);
        let (configure, commands) = async_channel::bounded(1);
        let (finished, done) = async_channel::bounded(1);
        if let Some(task) = &previous {
            task.cancel();
        }
        glib::spawn_future_local(async move {
            if let Some(task) = previous {
                task.finish().await;
            }
            let mut resources = Resources::default();
            let result = {
                let work = async {
                    let connection = connection.await?;
                    resources.connection = Some(connection.clone());
                    run(&owner, generation, &trigger?, commands, &mut resources, connection).await
                };
                futures_util::pin_mut!(work);
                let cancelled = cancelled.recv();
                futures_util::pin_mut!(cancelled);
                match select(cancelled, work).await {
                    Either::Left(_) => None,
                    Either::Right((result, _)) => Some(result),
                }
            };
            if let Some(Err(reason)) = result {
                #[cfg(not(test))]
                crate::log::event("hotkey_portal_failed", serde_json::json!({"reason": format!("{reason:?}")}));
                update(
                    &owner,
                    generation,
                    if reason == Failure::Unsupported { Capability::Unavailable } else { Capability::Unknown },
                    State::Failed { reason },
                );
            }
            // Unavailability is reported immediately. A retry still waits for cleanup,
            // so a slow Close cannot overlap a new authorization request.
            resources.close().await;
            let _ = finished.send(()).await;
        });
        Self { cancel, configure, done }
    }

    pub fn cancel(&self) {
        let _ = self.cancel.try_send(());
    }

    pub fn configure(&self) -> bool {
        self.configure.try_send(()).is_ok()
    }

    pub async fn finish(self) {
        self.cancel();
        let _ = self.done.recv().await;
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Default)]
struct Resources {
    connection: Option<zbus::Connection>,
    session: Option<Rc<Session<GlobalShortcuts>>>,
}

impl Resources {
    async fn close(&mut self) {
        if let Some(session) = self.session.take() {
            let _ = timeout(Duration::from_secs(1), session.close()).await;
        }
        // Closing this private connection invalidates any orphan request/session on the server.
        if let Some(connection) = self.connection.take() {
            let _ = timeout(Duration::from_secs(1), connection.close()).await;
        }
    }
}

async fn timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, Failure> {
    futures_util::pin_mut!(future);
    match select(future, Box::pin(glib::timeout_future(duration))).await {
        Either::Left((result, _)) => Ok(result),
        Either::Right(_) => Err(Failure::Temporary),
    }
}

fn update(owner: &Weak<HotKey>, generation: u64, capability: Capability, state: State) {
    if let Some(owner) = owner.upgrade() {
        owner.portal_update(generation, capability, state);
    }
}

async fn run(
    owner: &Weak<HotKey>,
    generation: u64,
    trigger: &str,
    commands: async_channel::Receiver<()>,
    resources: &mut Resources,
    connection: zbus::Connection,
) -> Result<(), Failure> {
    let id = ashpd::AppID::try_from(crate::APP_ID).map_err(|_| Failure::Temporary)?;
    // Older portals may not have Registry. Do not infer shortcut support from this call.
    let _registration = ashpd::register_host_app_with_connection(connection.clone(), id).await;
    #[cfg(not(test))]
    crate::log::event("hotkey_portal_registry", serde_json::json!({"ok": _registration.is_ok()}));
    let proxy = GlobalShortcuts::with_connection(connection.clone()).await.map_err(failure)?;
    let mut owners = proxy.receive_owner_changed().await.map_err(|_| Failure::Subscription)?;
    // ashpd falls back to v1 on some property errors; explicitly verify the property.
    let version = proxy.get_property::<u32>("version").await.map_err(dbus_failure)?;
    if version < 1 {
        return Err(Failure::Unsupported);
    }
    if version != proxy.version() {
        return Err(Failure::Temporary);
    }
    update(owner, generation, Capability::PortalVersion(proxy.version()), State::Pending);
    let disconnected = async {
        let changed = owners.next();
        futures_util::pin_mut!(changed);
        let closed = connection.closed();
        futures_util::pin_mut!(closed);
        let _ = select(changed, closed).await;
        Err(Failure::Disconnected)
    };
    let session = run_session(owner, generation, trigger, &proxy, commands, resources);
    futures_util::pin_mut!(session, disconnected);
    match select(disconnected, session).await {
        Either::Left((result, _)) | Either::Right((result, _)) => result,
    }
}

async fn connect() -> Result<zbus::Connection, Failure> {
    timeout(Duration::from_secs(15), zbus::connection::Builder::session().map_err(dbus_failure)?.method_timeout(Duration::from_secs(15)).build())
        .await?
        .map_err(dbus_failure)
}

async fn run_session(
    owner: &Weak<HotKey>,
    generation: u64,
    trigger: &str,
    proxy: &GlobalShortcuts,
    commands: async_channel::Receiver<()>,
    resources: &mut Resources,
) -> Result<(), Failure> {
    let session = Rc::new(timeout(Duration::from_secs(15), proxy.create_session(Default::default())).await?.map_err(failure)?);
    resources.session = Some(session.clone());
    // Session's public Serialize implementation is its D-Bus object path.
    let session_path = serde_json::to_value(&*session).ok().and_then(|v| v.as_str().map(str::to_owned)).ok_or(Failure::Temporary)?;
    let mut closed = session.receive_closed().await.map_err(|_| Failure::Subscription)?;
    // One stream preserves press/release ordering, unlike merging separate signal streams.
    let mut signals = proxy.receive_all_signals().await.map_err(|_| Failure::Subscription)?;
    let work = async {
        let shortcut = NewShortcut::new(SHORTCUT_ID, crate::i18n::l("打开 Qantrip 面板", "Open the Qantrip panel")).preferred_trigger(trigger);
        update(owner, generation, Capability::PortalVersion(proxy.version()), State::AwaitingPermission);
        let response = timeout(Duration::from_secs(300), proxy.bind_shortcuts(&session, &[shortcut], None, Default::default()))
            .await?
            .map_err(failure)?
            .response()
            .map_err(failure)?;
        let trigger = bound_trigger(response.shortcuts())?;
        let capability = Capability::Verified(Capabilities::portal(proxy.version()));
        update(owner, generation, capability, State::Active { trigger });
        loop {
            let next = signals.next();
            let command = commands.recv();
            futures_util::pin_mut!(next, command);
            match select(next, command).await {
                Either::Left((Some(message), _)) => {
                    let header = message.header();
                    let member = header.member().map(|m| m.as_str()).unwrap_or("");
                    match member {
                        "Activated" => {
                            let event: Activated = message.body().deserialize().map_err(|_| Failure::Subscription)?;
                            if matches_event(&session_path, event.session_handle().as_str(), event.shortcut_id())
                                && let Some(owner) = owner.upgrade()
                            {
                                let token = event.options().get("activation_token").and_then(|v| <&str>::try_from(v).ok().map(str::to_owned));
                                owner.portal_event(
                                    generation,
                                    Press { id: ShortcutId::TogglePanel, source: Source::Portal, phase: Phase::Pressed, timestamp: 0, token },
                                );
                            }
                        }
                        "Deactivated" => {
                            let event: Deactivated = message.body().deserialize().map_err(|_| Failure::Subscription)?;
                            if matches_event(&session_path, event.session_handle().as_str(), event.shortcut_id())
                                && let Some(owner) = owner.upgrade()
                            {
                                owner.portal_event(
                                    generation,
                                    Press { id: ShortcutId::TogglePanel, source: Source::Portal, phase: Phase::Released, timestamp: 0, token: None },
                                );
                            }
                        }
                        "ShortcutsChanged" => {
                            let event: ShortcutsChanged = message.body().deserialize().map_err(|_| Failure::Subscription)?;
                            if event.session_handle().as_str() == session_path {
                                let trigger = bound_trigger(event.shortcuts())?;
                                update(owner, generation, capability, State::Active { trigger });
                            }
                        }
                        _ => {}
                    }
                }
                Either::Left((None, _)) | Either::Right((Err(_), _)) => return Err(Failure::Disconnected),
                Either::Right((Ok(()), _)) => {
                    if proxy.version() >= 2 {
                        proxy.configure_shortcuts(&session, None, Default::default()).await.map_err(failure)?;
                    }
                }
            }
        }
    };
    futures_util::pin_mut!(work);
    let closed = closed.next();
    futures_util::pin_mut!(closed);
    match select(closed, work).await {
        Either::Left(_) => Err(Failure::Disconnected),
        Either::Right((result, _)) => result,
    }
}

fn matches_event(session: &str, event_session: &str, shortcut: &str) -> bool {
    session == event_session && shortcut == SHORTCUT_ID
}

fn bound_trigger(shortcuts: &[ashpd::desktop::global_shortcuts::Shortcut]) -> Result<String, Failure> {
    shortcuts
        .iter()
        .find(|s| s.id() == SHORTCUT_ID)
        .filter(|s| !s.trigger_description().trim().is_empty())
        .map(|s| super::normalize_trigger(s.trigger_description()))
        .ok_or(Failure::NotBound)
}

fn failure(error: ashpd::Error) -> Failure {
    use ashpd::{Error, PortalError};
    match error {
        Error::PortalNotFound(_) | Error::RequiresVersion(_, _) | Error::Portal(PortalError::NotFound(_)) => Failure::Unsupported,
        Error::Response(ResponseError::Cancelled) | Error::Portal(PortalError::Cancelled(_)) => Failure::Cancelled,
        Error::Portal(PortalError::NotAllowed(_)) => Failure::Denied,
        Error::Zbus(e) | Error::Portal(PortalError::ZBus(e)) => dbus_failure(e),
        Error::IO(_) | Error::NoResponse => Failure::Disconnected,
        // Response code 2 is unspecified failure, not necessarily refusal or conflict.
        _ => Failure::Temporary,
    }
}

fn dbus_failure(error: zbus::Error) -> Failure {
    match &error {
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.UnknownMethod"
            | "org.freedesktop.DBus.Error.UnknownInterface" => Failure::Unsupported,
            "org.freedesktop.DBus.Error.AccessDenied" | "org.freedesktop.portal.Error.NotAllowed" => Failure::Denied,
            "org.freedesktop.portal.Error.Cancelled" => Failure::Cancelled,
            "org.freedesktop.DBus.Error.Disconnected" => Failure::Disconnected,
            _ => Failure::Temporary,
        },
        zbus::Error::InputOutput(_) => Failure::Disconnected,
        _ => Failure::Temporary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_both_session_and_shortcut() {
        assert!(matches_event("/session/a", "/session/a", SHORTCUT_ID));
        assert!(!matches_event("/session/a", "/session/b", SHORTCUT_ID));
        assert!(!matches_event("/session/a", "/session/a", "other"));
    }

    #[test]
    fn typed_cancellation_does_not_use_error_text() {
        assert_eq!(failure(ashpd::Error::Response(ResponseError::Cancelled)), Failure::Cancelled);
        assert_eq!(failure(ashpd::Error::Response(ResponseError::Other)), Failure::Temporary);
        assert_eq!(failure(ashpd::PortalError::NotAllowed("opaque".into()).into()), Failure::Denied);
        assert_eq!(failure(ashpd::PortalError::Failed("cancelled by transport".into()).into()), Failure::Temporary);
    }

    #[test]
    fn successful_response_must_include_our_bound_shortcut() {
        assert_eq!(bound_trigger(&[]), Err(Failure::NotBound));
    }
}

#[cfg(test)]
#[path = "portal_tests.rs"]
mod integration_tests;
