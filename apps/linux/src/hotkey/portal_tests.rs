//! Fake Portal on a private D-Bus: no GTK initialization, real desktop or authorization.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::DirBuilderExt;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use super::*;
use ashpd::zvariant::{OwnedObjectPath, OwnedValue, Value};

const PATH: &str = "/org/freedesktop/portal/desktop";
const NAME: &str = "org.freedesktop.portal.Desktop";
const INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

struct Bus(Child, String, std::path::PathBuf);

impl Bus {
    fn new() -> Self {
        Self::with_policy("")
    }

    fn with_policy(policy: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "qantrip-fake-bus-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&directory).unwrap();
        let config = directory.join("bus.conf");
        std::fs::write(&config, format!(r#"<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/>{policy}</policy></busconfig>"#)).unwrap();
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("fake Portal tests require dbus-daemon");
        // No service directories: never activate the host portal.
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut address).unwrap();
        Self(child, address.trim().into(), directory)
    }

    async fn connection(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.1.as_str()).unwrap().method_timeout(Duration::from_secs(2)).build().await.unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.2);
    }
}

#[derive(Clone)]
struct Pending {
    destination: String,
    path: String,
    session: OwnedObjectPath,
}

#[derive(Clone)]
struct FakePortal {
    pending: async_channel::Sender<Pending>,
    sessions: Arc<Mutex<Vec<OwnedObjectPath>>>,
    closes: Arc<AtomicUsize>,
    configurations: Arc<AtomicUsize>,
    version: u32,
}

struct FakeSession(Arc<AtomicUsize>);

#[zbus::interface(name = "org.freedesktop.portal.Session")]
impl FakeSession {
    async fn close(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Registry;

#[zbus::interface(name = "org.freedesktop.host.portal.Registry")]
impl Registry {
    async fn register(&self, app_id: &str, _options: HashMap<String, OwnedValue>) {
        assert_eq!(app_id, crate::APP_ID);
    }
}

fn option(options: &HashMap<String, OwnedValue>, key: &str) -> String {
    <&str>::try_from(options.get(key).unwrap()).unwrap().into()
}

fn request_path(header: &zbus::message::Header<'_>, options: &HashMap<String, OwnedValue>) -> String {
    let sender = header.sender().unwrap().as_str().trim_start_matches(':').replace('.', "_");
    format!("{PATH}/request/{sender}/{}", option(options, "handle_token"))
}

#[zbus::interface(name = "org.freedesktop.portal.GlobalShortcuts")]
impl FakePortal {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        self.version
    }

    async fn create_session(
        &self,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> OwnedObjectPath {
        let sender = header.sender().unwrap().as_str().trim_start_matches(':').replace('.', "_");
        let path = format!("{PATH}/session/{sender}/{}", option(&options, "session_handle_token"));
        connection.object_server().at(path.as_str(), FakeSession(self.closes.clone())).await.unwrap();
        let path = OwnedObjectPath::try_from(path).unwrap();
        self.sessions.lock().unwrap().push(path.clone());
        let request = request_path(&header, &options);
        let results = HashMap::from([("session_handle", Value::from(path.as_str()))]);
        connection
            .emit_signal(header.sender().map(|s| s.as_str()), request.as_str(), "org.freedesktop.portal.Request", "Response", &(0u32, results))
            .await
            .unwrap();
        request.try_into().unwrap()
    }

    async fn bind_shortcuts(
        &self,
        session: OwnedObjectPath,
        _shortcuts: Vec<(String, HashMap<String, OwnedValue>)>,
        _parent: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> OwnedObjectPath {
        let path = request_path(&header, &options);
        self.pending.send(Pending { destination: header.sender().unwrap().to_string(), path: path.clone(), session }).await.unwrap();
        path.try_into().unwrap()
    }

    async fn configure_shortcuts(&self, _session: OwnedObjectPath, _parent: String, _options: HashMap<String, OwnedValue>) {
        self.configurations.fetch_add(1, Ordering::SeqCst);
    }
}

fn shortcuts(id: &str, trigger: &str) -> Vec<(String, HashMap<String, Value<'static>>)> {
    vec![(id.into(), HashMap::from([("description".into(), Value::from("Toggle")), ("trigger_description".into(), Value::from(trigger.to_owned()))]))]
}

async fn reply(connection: &zbus::Connection, pending: &Pending, code: u32, id: &str, trigger: &str) {
    let results = HashMap::from([("shortcuts", Value::new(shortcuts(id, trigger)))]);
    connection
        .emit_signal(Some(pending.destination.as_str()), pending.path.as_str(), "org.freedesktop.portal.Request", "Response", &(code, results))
        .await
        .unwrap();
}

async fn key(connection: &zbus::Connection, pending: &Pending, id: &str, phase: &str) {
    let options = HashMap::from([("activation_token", Value::from("test-token-do-not-log"))]);
    connection.emit_signal(Some(pending.destination.as_str()), PATH, INTERFACE, phase, &(pending.session.clone(), id, 123u64, options)).await.unwrap();
}

async fn until(mut condition: impl FnMut() -> bool) {
    timeout(Duration::from_secs(3), async {
        while !condition() {
            glib::timeout_future(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fake Portal condition timed out");
}

async fn pending(receiver: &async_channel::Receiver<Pending>) -> Pending {
    timeout(Duration::from_secs(3), receiver.recv()).await.unwrap().unwrap()
}

fn start(owner: &Rc<HotKey>, connection: zbus::Connection, previous: Option<Task>) -> Task {
    let generation = owner.lifecycle.borrow_mut().begin(true).unwrap();
    owner.set_state(State::Pending);
    Task::spawn(Rc::downgrade(owner), generation, Ok("CTRL+q".into()), previous, async { Ok(connection) })
}

#[test]
fn failed_subscription_never_reports_registered() {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let bus = Bus::with_policy(r#"<deny send_destination="org.freedesktop.DBus" send_interface="org.freedesktop.DBus" send_member="AddMatch"/>"#);
                let server = bus.connection().await;
                let (tx, rx) = async_channel::unbounded();
                let fake = FakePortal { pending: tx, sessions: Default::default(), closes: Default::default(), configurations: Default::default(), version: 2 };
                server.object_server().at(PATH, fake).await.unwrap();
                server.object_server().at(PATH, Registry).await.unwrap();
                server
                    .call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "RequestName", &(NAME, 0u32))
                    .await
                    .unwrap();
                let owner = HotKey::new(super::super::Mode::Portal, "<Control>q".into(), None);
                let observed = Rc::new(std::cell::Cell::new(false));
                let states = observed.clone();
                let weak = Rc::downgrade(&owner);
                owner.connect_state(move || {
                    if weak.upgrade().is_some_and(|owner| owner.state().registered()) {
                        states.set(true);
                    }
                });
                let task = start(&owner, bus.connection().await, None);
                until(|| matches!(owner.state(), State::Failed { .. })).await;
                assert_eq!(owner.state(), State::Failed { reason: Failure::Subscription });
                assert!(!observed.get());
                assert!(rx.is_empty());
                task.finish().await;
            })
        })
        .unwrap();
}

#[test]
fn portal_lifecycle_against_fake_service() {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let bus = Bus::new();
                let server = bus.connection().await;
                let (tx, rx) = async_channel::unbounded();
                let fake = FakePortal { pending: tx, sessions: Default::default(), closes: Default::default(), configurations: Default::default(), version: 2 };
                server.object_server().at(PATH, fake.clone()).await.unwrap();
                server.object_server().at(PATH, Registry).await.unwrap();
                server.request_name(NAME).await.unwrap();
                let owner = HotKey::new(super::super::Mode::Portal, "<Control>q".into(), None);
                let events = Rc::new(std::cell::RefCell::new(Vec::new()));
                let received = events.clone();
                owner.connect_press(move |event| received.borrow_mut().push((event.phase, event.token.clone())));

                // A binding request is not a verified capability.
                let connection = bus.connection().await;
                let task = start(&owner, connection.clone(), None);
                let request = pending(&rx).await;
                assert_eq!(owner.state(), State::AwaitingPermission);
                assert!(matches!(owner.capability(), Capability::PortalVersion(2)));
                // Cancelled authorization is stable; duplicate start/retry cannot create dialogs.
                owner.start();
                owner.retry();
                reply(&server, &request, 1, SHORTCUT_ID, "Ctrl+Q").await;
                until(|| matches!(owner.state(), State::Failed { reason: Failure::Cancelled })).await;
                until(|| connection.is_closed()).await;
                owner.start();
                assert_eq!(owner.state(), State::Failed { reason: Failure::Cancelled });
                assert!(connection.is_closed());
                assert_eq!(fake.closes.load(Ordering::SeqCst), 1);
                assert!(rx.is_empty());
                task.finish().await;

                let connection = bus.connection().await;
                let task = start(&owner, connection.clone(), None);
                let request = pending(&rx).await;
                reply(&server, &request, 0, SHORTCUT_ID, "Ctrl+Q").await;
                until(|| owner.state().registered()).await;
                assert!(owner.capabilities().configure);
                // Foreign session, foreign id, repeated press, then one release.
                key(&server, &Pending { session: "/foreign/session".try_into().unwrap(), ..request.clone() }, SHORTCUT_ID, "Activated").await;
                key(&server, &request, "other", "Activated").await;
                key(&server, &request, SHORTCUT_ID, "Activated").await;
                key(&server, &request, SHORTCUT_ID, "Activated").await;
                key(&server, &request, SHORTCUT_ID, "Deactivated").await;
                until(|| events.borrow().len() == 2).await;
                assert_eq!(events.borrow()[0], (Phase::Pressed, Some("test-token-do-not-log".into())));
                assert_eq!(events.borrow()[1], (Phase::Released, None));
                assert!(task.configure());
                until(|| fake.configurations.load(Ordering::SeqCst) == 1).await;
                server
                    .emit_signal(
                        Some(request.destination.as_str()),
                        PATH,
                        INTERFACE,
                        "ShortcutsChanged",
                        &(request.session.clone(), shortcuts(SHORTCUT_ID, "Alt+Q")),
                    )
                    .await
                    .unwrap();
                until(|| matches!(owner.state(), State::Active { trigger } if trigger == "Alt+Q")).await;

                // Rebind closes the old session/connection before beginning another request.
                let old_generation = owner.lifecycle.borrow().generation;
                let next = start(&owner, bus.connection().await, Some(task));
                let second = pending(&rx).await;
                assert!(connection.is_closed());
                assert_eq!(fake.closes.load(Ordering::SeqCst), 2);
                owner.portal_update(old_generation, Capability::Verified(Capabilities::portal(2)), State::Active { trigger: "stale".into() });
                assert_eq!(owner.state(), State::AwaitingPermission);
                reply(&server, &second, 0, "other", "Ctrl+Q").await;
                until(|| matches!(owner.state(), State::Failed { reason: Failure::NotBound })).await;
                next.finish().await;

                // Cancel a pending request, then deliver its late success: it cannot revive state.
                let connection = bus.connection().await;
                let task = start(&owner, connection.clone(), None);
                let request = pending(&rx).await;
                owner.stop();
                task.finish().await;
                assert!(connection.is_closed());
                assert_eq!(owner.state(), State::Closed);
                // Broadcast instead of directed reply because the old unique name is gone.
                server
                    .emit_signal(
                        None::<&str>,
                        request.path.as_str(),
                        "org.freedesktop.portal.Request",
                        "Response",
                        &(0u32, HashMap::from([("shortcuts", Value::new(shortcuts(SHORTCUT_ID, "Ctrl+Q")))])),
                    )
                    .await
                    .unwrap();
                assert_eq!(owner.state(), State::Closed);
                assert_eq!(fake.closes.load(Ordering::SeqCst), 4);

                // Service loss while active must clear availability without an automatic retry.
                let owner = HotKey::new(super::super::Mode::Portal, "<Control>q".into(), None);
                let task = start(&owner, bus.connection().await, None);
                let request = pending(&rx).await;
                reply(&server, &request, 0, SHORTCUT_ID, "Ctrl+Q").await;
                until(|| owner.state().registered()).await;
                server.release_name(NAME).await.unwrap();
                until(|| matches!(owner.state(), State::Failed { reason: Failure::Disconnected })).await;
                assert!(!matches!(owner.capability(), Capability::Verified(_)));
                assert!(rx.is_empty());
                task.finish().await;

                // v1 binds successfully but cannot open ConfigureShortcuts.
                server.request_name(NAME).await.unwrap();
                server.object_server().interface::<_, FakePortal>(PATH).await.unwrap().get_mut().await.version = 1;
                let owner = HotKey::new(super::super::Mode::Portal, "<Control>q".into(), None);
                let task = start(&owner, bus.connection().await, None);
                let request = pending(&rx).await;
                reply(&server, &request, 0, SHORTCUT_ID, "Ctrl+Q").await;
                until(|| owner.state().registered()).await;
                assert!(!owner.capabilities().configure);
                server
                    .emit_signal(
                        Some(request.destination.as_str()),
                        request.session.as_str(),
                        "org.freedesktop.portal.Session",
                        "Closed",
                        &HashMap::<String, Value>::new(),
                    )
                    .await
                    .unwrap();
                until(|| matches!(owner.state(), State::Failed { reason: Failure::Disconnected })).await;
                task.finish().await;

                // Connection/stream termination during consent never becomes Active.
                let connection = bus.connection().await;
                let owner = HotKey::new(super::super::Mode::Portal, "<Control>q".into(), None);
                let task = start(&owner, connection.clone(), None);
                let _request = pending(&rx).await;
                connection.close().await.unwrap();
                until(|| matches!(owner.state(), State::Failed { reason: Failure::Disconnected })).await;
                task.finish().await;

                // Dropping the coordinator also cancels an unfinished request.
                let connection = bus.connection().await;
                let owner = HotKey::new(super::super::Mode::Portal, "<Control>q".into(), None);
                let task = start(&owner, connection.clone(), None);
                let done = task.done.clone();
                let _request = pending(&rx).await;
                let super::super::Backend::Portal(slot) = &owner.backend else { unreachable!() };
                *slot.borrow_mut() = Some(task);
                drop(owner);
                timeout(Duration::from_secs(3), done.recv()).await.unwrap().unwrap();
                assert!(connection.is_closed());
            })
        })
        .unwrap();
}
