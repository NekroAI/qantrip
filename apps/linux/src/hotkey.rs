//! Linux 快捷键协调器。后端候选、已验证能力、运行状态和失败原因分别建模。
//! 所有 GTK 回调在 GLib 主线程执行；Portal 资源由可取消的异步任务持有。

mod model;
mod portal;
mod x11_backend;

pub use model::{Capabilities, Capability, Failure, Mode, Phase, Press, ShortcutId, Source, State, select_backend};

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gtk::gdk;
use gtk::glib::translate::IntoGlib;

use crate::x11::{Modifiers, X11};
use model::{EventGate, Lifecycle};

pub const SHORTCUT_ID: &str = ShortcutId::TogglePanel.as_str();

pub fn manual_command() -> String {
    format!("{} toggle", gtk::glib::shell_quote(crate::autostart::exec_path()).to_string_lossy())
}

fn niri_binding(executable: &str) -> String {
    // KDL 引号字符串；spawn 直接传递参数，不经过 shell。
    let executable = executable.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t");
    format!("binds {{\n    Ctrl+Shift+Q repeat=false {{ spawn \"{executable}\" \"toggle\"; }}\n}}")
}

pub fn niri_binding_hint() -> (String, String) {
    let config = niri_binding(&crate::autostart::exec_path());
    let hint = crate::t!(
        "在 niri 配置文件（通常为 ~/.config/niri/config.kdl）中添加以下配置；已有 Ctrl+Shift+Q 绑定时请替换它或改用其他组合。保存后按快捷键验证。移动应用后需更新路径。\n{config}",
        "Add this to your niri configuration (usually ~/.config/niri/config.kdl). Replace any existing Ctrl+Shift+Q binding or choose another combination. Save and press the shortcut to verify. Update the path if you move the app.\n{config}"
    );
    (config, hint)
}

pub fn compositor_binding_hint(session: &crate::session::Session) -> Option<(String, String)> {
    if session.is_niri() {
        return Some(niri_binding_hint());
    }
    let desktop = session.desktop_name.split(':').find(|name| name.eq_ignore_ascii_case("sway") || name.eq_ignore_ascii_case("hyprland"))?;
    let command = manual_command();
    let (config, location) = if desktop.eq_ignore_ascii_case("sway") {
        // Sway expands config variables even before invoking the shell. For paths
        // containing config metacharacters, point the user to a simple launcher.
        if crate::autostart::exec_path().chars().any(|c| c.is_control() || "$#;,\\\"'".contains(c)) {
            return Some((
                command.clone(),
                crate::i18n::l(
                    "应用路径含 Sway 配置特殊字符。请将此命令放入固定路径的启动脚本，再用 bindsym --no-repeat Ctrl+Shift+q exec 绑定脚本；详见安装文档。",
                    "The app path contains Sway configuration metacharacters. Put this command in a launcher script at a simple path, then bind the script with bindsym --no-repeat Ctrl+Shift+q exec; see the installation guide.",
                ),
            ));
        }
        (format!("bindsym --no-repeat Ctrl+Shift+q exec {command}"), "~/.config/sway/config")
    } else {
        // Lua string containing a shell-quoted command (two distinct escaping layers).
        let lua = command.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t");
        (format!("hl.bind(\"CTRL + SHIFT + Q\", hl.dsp.exec_cmd(\"{lua}\"), {{ repeating = false }})"), "~/.config/hypr/hyprland.lua (0.55+)")
    };
    let hint = crate::t!(
        "在 {location} 中添加以下配置，并移除重复绑定。移动 AppImage 后需更新路径。长按重复已关闭；旧版 Hyprland 的配置见安装文档。\n{config}",
        "Add this to {location} and remove duplicate bindings. Update the path after moving the AppImage. Key repeat is disabled; see the installation guide for older Hyprland syntax.\n{config}"
    );
    Some((config, hint))
}

/// GTK 加速键字符串 → (keysym, X 修饰键掩码)
pub fn parse_accel(accel: &str) -> Option<(gdk::Key, gdk::ModifierType)> {
    let (key, mods) = gtk::accelerator_parse(accel)?;
    if key == gdk::Key::VoidSymbol {
        return None;
    }
    Some((key, mods))
}

/// 门户返回的组合键说明（GNOME 为 "Press <Control>q"）转为界面显示的 "Ctrl+Q"；其他格式原样保留
pub fn normalize_trigger(desc: &str) -> String {
    if let Some(i) = desc.find('<') {
        let accel = desc[i..].trim();
        if parse_accel(accel).is_some() {
            return accel_label(accel);
        }
    }
    desc.trim().to_string()
}

/// 界面显示，如 "Ctrl+Q"
pub fn accel_label(accel: &str) -> String {
    match parse_accel(accel) {
        Some((k, m)) => gtk::accelerator_get_label(k, m).to_string(),
        None => accel.to_string(),
    }
}

/// 门户的快捷键格式（XDG shortcuts 规范），如 "CTRL+q"
pub fn portal_trigger(accel: &str) -> Option<String> {
    let (key, mods) = parse_accel(accel)?;
    let mut parts = Vec::new();
    if mods.contains(gdk::ModifierType::CONTROL_MASK) {
        parts.push("CTRL".to_string());
    }
    if mods.contains(gdk::ModifierType::ALT_MASK) {
        parts.push("ALT".to_string());
    }
    if mods.contains(gdk::ModifierType::SHIFT_MASK) {
        parts.push("SHIFT".to_string());
    }
    if mods.contains(gdk::ModifierType::SUPER_MASK) {
        parts.push("LOGO".to_string());
    }
    parts.push(key.name()?.to_string());
    Some(parts.join("+"))
}

/// 后端边界保持为 enum：三个后端共享同一个生命周期和订阅入口。
enum Backend {
    X11 { connection: Arc<X11>, worker: x11_backend::Worker },
    Portal(RefCell<Option<portal::Task>>),
    Manual,
}

pub struct HotKey {
    backend: Backend,
    state: RefCell<State>,
    capability: Cell<Capability>,
    accel: RefCell<String>,
    paused: Cell<bool>,
    suspended: Cell<bool>,
    lifecycle: RefCell<Lifecycle>,
    gate: RefCell<EventGate>,
    x11_operation: Cell<u64>,
    on_press: RefCell<Vec<Rc<dyn Fn(&Press)>>>,
    on_state: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl HotKey {
    pub fn new(mode: Mode, accel: String, x11: Option<Arc<X11>>) -> Rc<Self> {
        let backend = match mode {
            Mode::X11 => x11
                .and_then(|connection| x11_backend::Worker::new(connection.clone()).map(|worker| Backend::X11 { connection, worker }))
                .unwrap_or(Backend::Manual),
            Mode::Portal => Backend::Portal(RefCell::new(None)),
            Mode::Manual => Backend::Manual,
        };
        Rc::new(Self {
            state: RefCell::new(if matches!(backend, Backend::Manual) { State::Manual } else { State::Pending }),
            capability: Cell::new(Capability::Unknown),
            backend,
            accel: RefCell::new(accel),
            paused: Cell::new(false),
            suspended: Cell::new(false),
            lifecycle: RefCell::new(Lifecycle::default()),
            gate: RefCell::new(EventGate::default()),
            x11_operation: Cell::new(0),
            on_press: RefCell::new(Vec::new()),
            on_state: RefCell::new(Vec::new()),
        })
    }

    pub fn mode(&self) -> Mode {
        match self.backend {
            Backend::X11 { .. } => Mode::X11,
            Backend::Portal(_) => Mode::Portal,
            Backend::Manual => Mode::Manual,
        }
    }

    pub fn state(&self) -> State {
        self.state.borrow().clone()
    }

    pub fn capability(&self) -> Capability {
        self.capability.get()
    }

    pub fn capabilities(&self) -> Capabilities {
        match self.capability() {
            Capability::Verified(c) => c,
            _ => Capabilities::MANUAL,
        }
    }

    pub fn accel(&self) -> String {
        self.accel.borrow().clone()
    }

    pub fn display(&self) -> String {
        match self.state() {
            State::Active { trigger } | State::Paused { trigger } if !trigger.is_empty() && self.mode() == Mode::Portal => trigger,
            _ => accel_label(&self.accel.borrow()),
        }
    }

    pub fn connect_press(&self, f: impl Fn(&Press) + 'static) {
        self.on_press.borrow_mut().push(Rc::new(f));
    }

    pub fn connect_state(&self, f: impl Fn() + 'static) {
        self.on_state.borrow_mut().push(Rc::new(f));
    }

    fn set_state(&self, state: State) {
        if self.state() == state {
            return;
        }
        *self.state.borrow_mut() = state;
        let callbacks = self.on_state.borrow().clone();
        for callback in callbacks {
            callback();
        }
    }

    fn registered(&self, trigger: String) {
        self.set_state(if self.paused.get() || self.suspended.get() { State::Paused { trigger } } else { State::Active { trigger } });
    }

    /// 订阅者会收到释放，但只有 Pressed / Invoke 执行面板动作。
    pub fn press(&self, event: Press) {
        if self.state() == State::Closed {
            return;
        }
        if event.source.physical() && !self.state().registered() {
            return;
        }
        if !self.gate.borrow_mut().accept(&event, self.suspended.get()) {
            return;
        }
        let callbacks = self.on_press.borrow().clone();
        for callback in callbacks {
            callback(&event);
        }
    }

    pub fn start(self: &Rc<Self>) {
        self.begin(false);
    }

    fn begin(self: &Rc<Self>, retry: bool) {
        // 正在等待授权时 Retry 不启动第二个授权请求。
        if retry && matches!(self.state(), State::Pending | State::AwaitingPermission) {
            return;
        }
        let Some(generation) = self.lifecycle.borrow_mut().begin(retry) else { return };
        *self.gate.borrow_mut() = EventGate::default();
        match &self.backend {
            Backend::X11 { .. } => self.schedule_x11(),
            Backend::Portal(task) => {
                let previous = task.borrow_mut().take();
                self.capability.set(Capability::Unknown);
                self.set_state(State::Pending);
                *task.borrow_mut() = Some(portal::Task::start(Rc::downgrade(self), generation, self.accel(), previous));
            }
            Backend::Manual => {
                self.capability.set(Capability::Unavailable);
                self.set_state(State::Manual);
            }
        }
    }

    pub fn retry(self: &Rc<Self>) {
        self.begin(true);
    }

    fn next_x11_operation(&self) -> u64 {
        let revision = self.x11_operation.get() + 1;
        self.x11_operation.set(revision);
        revision
    }

    fn schedule_x11(self: &Rc<Self>) {
        let revision = self.next_x11_operation();
        self.set_state(State::Pending);
        let this = self.clone();
        gtk::glib::spawn_future_local(async move {
            let _ = this.register_x11(&this.accel(), revision).await;
        });
    }

    async fn register_x11(&self, accel: &str, revision: u64) -> Result<(), Failure> {
        let Backend::X11 { connection, worker } = &self.backend else { return Err(Failure::Unsupported) };
        if revision != self.x11_operation.get() || self.state() == State::Closed {
            return Err(Failure::Superseded);
        }
        // Parsing GDK accelerators stays on the main thread; X requests do not.
        let keep = !self.paused.get() && !self.suspended.get();
        let mut outcome = if let Some((key, mods)) = parse_accel(accel) {
            worker
                .register(
                    key.into_glib(),
                    Modifiers {
                        control: mods.contains(gdk::ModifierType::CONTROL_MASK),
                        shift: mods.contains(gdk::ModifierType::SHIFT_MASK),
                        alt: mods.contains(gdk::ModifierType::ALT_MASK),
                        super_key: mods.contains(gdk::ModifierType::SUPER_MASK),
                    },
                    keep,
                )
                .await
        } else {
            self.capability.set(Capability::Unknown);
            self.set_state(State::Failed { reason: Failure::InvalidShortcut });
            return Err(Failure::InvalidShortcut);
        };
        if revision != self.x11_operation.get() || self.state() == State::Closed {
            return Err(Failure::Superseded);
        }
        if outcome.result.is_ok() && keep && !connection.is_grabbed() {
            outcome.result = Err(Failure::MappingChanged);
        }
        *self.gate.borrow_mut() = EventGate::default();
        match outcome.result {
            Ok(()) => {
                *self.accel.borrow_mut() = accel.to_string();
                self.capability.set(Capability::Verified(Capabilities::X11));
                self.registered(accel_label(accel));
            }
            Err(reason) => {
                if outcome.grabbed && connection.is_grabbed() {
                    self.capability.set(Capability::Verified(Capabilities::X11));
                    self.registered(accel_label(&self.accel()));
                } else {
                    self.capability.set(Capability::Unknown);
                    self.set_state(State::Failed { reason });
                }
            }
        }
        outcome.result
    }

    pub async fn try_register(&self, accel: &str) -> Result<(), Failure> {
        if self.state() == State::Closed {
            return Err(Failure::Disconnected);
        }
        if parse_accel(accel).is_none() {
            return Err(Failure::InvalidShortcut);
        }
        // End recording before committing: no release/regrab conflict window.
        let was_recording = self.suspended.replace(false);
        let old = self.accel();
        let revision = self.next_x11_operation();
        self.set_state(State::Pending);
        let result = self.register_x11(accel, revision).await;
        if result.is_err() && was_recording && revision == self.x11_operation.get() && self.state() != State::Closed {
            let _ = self.register_x11(&old, revision).await;
        }
        result
    }

    pub fn configure(&self) -> bool {
        if !self.capabilities().configure {
            return false;
        }
        match &self.backend {
            Backend::Portal(task) => task.borrow().as_ref().is_some_and(portal::Task::configure),
            _ => false,
        }
    }

    /// X11 抓取解除后按键回到前台应用；Portal 仅由应用层过滤唤起。
    pub fn set_paused(self: &Rc<Self>, paused: bool) {
        if self.paused.replace(paused) != paused {
            self.update_pause();
        }
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn paused(&self) -> bool {
        self.paused.get()
    }

    pub fn suspend(self: &Rc<Self>) {
        if !self.suspended.replace(true) {
            self.update_pause();
        }
    }

    pub fn resume(self: &Rc<Self>) {
        if self.suspended.replace(false) {
            self.update_pause();
        }
    }

    fn update_pause(self: &Rc<Self>) {
        if self.state() == State::Closed {
            return;
        }
        match &self.backend {
            Backend::X11 { worker, .. } => {
                *self.gate.borrow_mut() = EventGate::default();
                if self.paused.get() || self.suspended.get() {
                    self.next_x11_operation();
                    worker.ungrab();
                    self.set_state(State::Paused { trigger: self.display() });
                } else {
                    self.schedule_x11();
                }
            }
            Backend::Portal(_) if self.state().registered() => self.registered(self.display()),
            _ => {}
        }
    }

    pub fn x11_event(&self, generation: u64, phase: Phase, timestamp: u32) {
        if let Backend::X11 { connection: x, .. } = &self.backend
            && x.accepts(generation)
        {
            self.press(Press { id: ShortcutId::TogglePanel, source: Source::X11, phase, timestamp, token: None });
        }
    }

    pub fn x11_status(&self, generation: u64, result: Result<(), Failure>) {
        if matches!(self.state(), State::Closed | State::Pending) {
            return;
        }
        if let Backend::X11 { connection: x, .. } = &self.backend
            && x.accepts(generation)
        {
            *self.gate.borrow_mut() = EventGate::default();
            match result {
                Ok(()) => self.registered(accel_label(&self.accel())),
                Err(reason) => {
                    self.capability.set(Capability::Unknown);
                    self.set_state(State::Failed { reason });
                }
            }
        }
    }

    pub fn stop(&self) {
        self.lifecycle.borrow_mut().close();
        self.next_x11_operation();
        *self.gate.borrow_mut() = EventGate::default();
        self.capability.set(Capability::Unavailable);
        match &self.backend {
            Backend::X11 { worker, .. } => worker.stop(),
            Backend::Portal(task) => {
                if let Some(task) = task.borrow().as_ref() {
                    task.cancel();
                }
            }
            Backend::Manual => {}
        }
        self.set_state(State::Closed);
    }

    pub async fn shutdown(&self) {
        self.stop();
        let task = match &self.backend {
            Backend::Portal(task) => task.borrow_mut().take(),
            _ => None,
        };
        if let Some(task) = task {
            task.finish().await;
        }
        if let Backend::X11 { worker, .. } = &self.backend {
            // A stalled remote X server cannot hold application exit indefinitely.
            let finish = worker.finish();
            futures_util::pin_mut!(finish);
            let _ = futures_util::future::select(finish, Box::pin(gtk::glib::timeout_future(std::time::Duration::from_secs(2)))).await;
        }
    }

    fn portal_update(&self, generation: u64, capability: Capability, state: State) {
        if self.lifecycle.borrow().accepts(generation) {
            self.capability.set(capability);
            if let State::Active { trigger } = state {
                self.registered(trigger);
            } else {
                *self.gate.borrow_mut() = EventGate::default();
                self.set_state(state);
            }
        }
    }

    fn portal_event(&self, generation: u64, event: Press) {
        if self.lifecycle.borrow().accepts(generation) {
            self.press(event);
        }
    }
}

impl Drop for HotKey {
    fn drop(&mut self) {
        match &self.backend {
            Backend::X11 { worker, .. } => worker.stop(),
            Backend::Portal(task) => {
                if let Some(task) = task.borrow().as_ref() {
                    task.cancel();
                }
            }
            Backend::Manual => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn niri_binding_quotes_paths_and_disables_repeat() {
        assert_eq!(
            super::niri_binding("/tmp/My \"Qantrip\"\\app.AppImage"),
            "binds {\n    Ctrl+Shift+Q repeat=false { spawn \"/tmp/My \\\"Qantrip\\\"\\\\app.AppImage\" \"toggle\"; }\n}"
        );
    }
}
