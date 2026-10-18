//! 全局唤起快捷键（docs/decisions.md D37）。
//!
//! - Wayland：xdg-desktop-portal GlobalShortcuts（GNOME 48+、KDE Plasma）。首次绑定时系统弹出确认对话框，
//!   之后由系统记住；修改快捷键在系统设置中进行（门户 v2 可以直接打开配置界面）。
//! - X11：XGrabKey（不需要任何权限），设置中可以录制新的组合键。
//! - 都不可用时：引导用户在系统设置中添加自定义快捷键，命令为 `qantrip toggle`（单实例经 D-Bus 唤起）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use futures_util::StreamExt;
use gtk::gdk;
use gtk::glib;
use gtk::glib::translate::IntoGlib;

use crate::i18n::l;
use crate::x11::X11;

/// 一次唤起
#[derive(Clone, Debug)]
pub struct Press {
    pub source: &'static str,
    /// X11 事件时间戳（激活面板时使用），其他来源为 0
    pub timestamp: u32,
    /// Wayland 激活令牌（门户在 Activated 信号中提供时）
    pub token: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Portal,
    X11,
    /// 系统自定义快捷键 → `qantrip toggle`
    Manual,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum State {
    /// 正在向系统注册（门户对话框可能正在等待用户确认）
    Pending,
    /// 已注册；`trigger` 为系统显示的组合键
    Active { trigger: String },
    /// 注册失败（被占用、用户拒绝等）
    Failed { reason: String },
    /// 本桌面不支持应用注册全局快捷键，需要用户在系统中绑定 `qantrip toggle`
    Manual,
}

pub const SHORTCUT_ID: &str = "toggle-panel";

pub struct HotKey {
    mode: Cell<Mode>,
    state: RefCell<State>,
    accel: RefCell<String>,
    x11: Option<Arc<X11>>,
    paused: Cell<bool>,
    suspended: Cell<bool>,
    portal: RefCell<Option<Rc<PortalBinding>>>,
    on_press: RefCell<Vec<Rc<dyn Fn(&Press)>>>,
    on_state: RefCell<Vec<Rc<dyn Fn()>>>,
}

struct PortalBinding {
    proxy: ashpd::desktop::global_shortcuts::GlobalShortcuts,
    session: ashpd::desktop::Session<ashpd::desktop::global_shortcuts::GlobalShortcuts>,
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

fn x_mods(mods: gdk::ModifierType) -> u16 {
    let mut m = 0u16;
    if mods.contains(gdk::ModifierType::SHIFT_MASK) {
        m |= 1;
    }
    if mods.contains(gdk::ModifierType::CONTROL_MASK) {
        m |= 4;
    }
    if mods.contains(gdk::ModifierType::ALT_MASK) {
        m |= 8;
    }
    if mods.contains(gdk::ModifierType::SUPER_MASK) {
        m |= 64;
    }
    m
}

impl HotKey {
    pub fn new(mode: Mode, accel: String, x11: Option<Arc<X11>>) -> Rc<HotKey> {
        Rc::new(HotKey {
            mode: Cell::new(mode),
            state: RefCell::new(if mode == Mode::Manual { State::Manual } else { State::Pending }),
            accel: RefCell::new(accel),
            x11,
            paused: Cell::new(false),
            suspended: Cell::new(false),
            portal: RefCell::new(None),
            on_press: RefCell::new(Vec::new()),
            on_state: RefCell::new(Vec::new()),
        })
    }

    pub fn mode(&self) -> Mode {
        self.mode.get()
    }

    pub fn state(&self) -> State {
        self.state.borrow().clone()
    }

    pub fn accel(&self) -> String {
        self.accel.borrow().clone()
    }

    /// 当前快捷键的显示文字（门户模式下为系统报告的组合键）
    pub fn display(&self) -> String {
        match &*self.state.borrow() {
            State::Active { trigger } if !trigger.is_empty() && self.mode.get() == Mode::Portal => trigger.clone(),
            _ => accel_label(&self.accel.borrow()),
        }
    }

    pub fn connect_press(&self, f: impl Fn(&Press) + 'static) {
        self.on_press.borrow_mut().push(Rc::new(f));
    }

    pub fn connect_state(&self, f: impl Fn() + 'static) {
        self.on_state.borrow_mut().push(Rc::new(f));
    }

    fn set_state(&self, s: State) {
        *self.state.borrow_mut() = s;
        let list: Vec<_> = self.on_state.borrow().clone();
        for f in list {
            f();
        }
    }

    /// 由各来源调用（X11 事件、门户信号、`qantrip toggle`）
    pub fn press(&self, p: Press) {
        if self.suspended.get() && p.source != "command" {
            return;
        }
        let list: Vec<_> = self.on_press.borrow().clone();
        for f in list {
            f(&p);
        }
    }

    // MARK: 注册

    pub fn start(self: &Rc<Self>) {
        match self.mode.get() {
            Mode::X11 => {
                let accel = self.accel();
                // 启动时前台已是排除的应用：先检查能否注册，再保持暂停
                if self.register_x11(&accel)
                    && self.paused.get()
                    && let Some(x) = &self.x11
                {
                    x.ungrab();
                }
            }
            Mode::Portal => {
                let this = self.clone();
                glib::spawn_future_local(async move { this.bind_portal().await });
            }
            Mode::Manual => self.set_state(State::Manual),
        }
    }

    /// X11：注册指定组合键，成功时返回 true（不修改设置）
    pub fn register_x11(&self, accel: &str) -> bool {
        let Some(x) = &self.x11 else { return false };
        let Some((key, mods)) = parse_accel(accel) else {
            self.set_state(State::Failed { reason: l("无法识别的快捷键", "Unrecognized shortcut") });
            return false;
        };
        if x.grab(key.into_glib(), x_mods(mods)) {
            *self.accel.borrow_mut() = accel.to_string();
            self.set_state(State::Active { trigger: accel_label(accel) });
            true
        } else {
            self.set_state(State::Failed { reason: l("已被其他程序占用", "In use by another app") });
            false
        }
    }

    /// 只检查能否注册（录制时使用）：成功则保持注册
    pub fn try_register(&self, accel: &str) -> bool {
        if self.mode.get() != Mode::X11 {
            return false;
        }
        self.register_x11(accel)
    }

    async fn bind_portal(self: Rc<Self>) {
        use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
        self.set_state(State::Pending);
        let result: Result<(PortalBinding, String), String> = async {
            let proxy = GlobalShortcuts::new().await.map_err(|e| e.to_string())?;
            let session = proxy.create_session(Default::default()).await.map_err(|e| e.to_string())?;
            let accel = self.accel();
            let trigger = portal_trigger(&accel);
            let shortcut = NewShortcut::new(SHORTCUT_ID, l("打开 Qantrip 面板", "Open the Qantrip panel")).preferred_trigger(trigger.as_deref());
            let response = proxy
                .bind_shortcuts(&session, &[shortcut], None, Default::default())
                .await
                .map_err(|e| e.to_string())?
                .response()
                .map_err(|e| e.to_string())?;
            let desc = response.shortcuts().iter().find(|s| s.id() == SHORTCUT_ID).map(|s| s.trigger_description().to_string()).unwrap_or_default();
            Ok((PortalBinding { proxy, session }, desc))
        }
        .await;
        match result {
            Ok((binding, desc)) => {
                crate::log::event("hotkey_portal", serde_json::json!({"trigger": desc, "version": binding.proxy.version()}));
                let desc = normalize_trigger(&desc);
                let stream = binding.proxy.receive_activated().await;
                *self.portal.borrow_mut() = Some(Rc::new(binding));
                self.set_state(State::Active { trigger: desc });
                if let Ok(mut stream) = stream {
                    let weak = Rc::downgrade(&self);
                    glib::spawn_future_local(async move {
                        while let Some(ev) = stream.next().await {
                            let Some(this) = weak.upgrade() else { return };
                            if ev.shortcut_id() != SHORTCUT_ID {
                                continue;
                            }
                            let token = ev.options().get("activation_token").and_then(|v| <&str>::try_from(v).ok().map(str::to_string));
                            this.press(Press { source: "portal", timestamp: 0, token });
                        }
                    });
                }
            }
            Err(e) => {
                crate::log::event("hotkey_portal_failed", serde_json::json!({"error": e}));
                let cancelled = e.to_lowercase().contains("cancel");
                self.set_state(State::Failed {
                    reason: if cancelled {
                        l("没有在系统对话框中确认", "Not confirmed in the system dialog")
                    } else {
                        l("系统没有接受快捷键注册", "The system didn't accept the shortcut")
                    },
                });
            }
        }
    }

    /// 门户模式：重新请求绑定（用户之前取消了对话框时）
    pub fn retry(self: &Rc<Self>) {
        if self.mode.get() == Mode::Portal {
            let old = self.portal.borrow_mut().take();
            let this = self.clone();
            glib::spawn_future_local(async move {
                if let Some(b) = old {
                    let _ = b.session.close().await;
                }
                this.bind_portal().await
            });
        } else {
            self.start();
        }
    }

    /// 门户 v2：打开系统的快捷键配置界面。不支持时返回 false。
    pub fn configure(self: &Rc<Self>) -> bool {
        let Some(b) = self.portal.borrow().clone() else { return false };
        if b.proxy.version() < 2 {
            return false;
        }
        glib::spawn_future_local(async move {
            let _ = b.proxy.configure_shortcuts(&b.session, None, Default::default()).await;
        });
        true
    }

    /// 排除的应用（D35）：X11 下暂时注销，按键原样交给应用。门户模式下无法让按键穿透，由调用方忽略唤起。
    pub fn set_paused(&self, paused: bool) {
        if self.paused.get() == paused {
            return;
        }
        self.paused.set(paused);
        if self.mode.get() == Mode::X11 && !self.suspended.get() {
            if paused {
                if let Some(x) = &self.x11 {
                    x.ungrab();
                }
            } else {
                let accel = self.accel();
                self.register_x11(&accel);
            }
        }
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn paused(&self) -> bool {
        self.paused.get()
    }

    /// 录制新快捷键期间暂停（X11）
    pub fn suspend(&self) {
        self.suspended.set(true);
        if self.mode.get() == Mode::X11
            && let Some(x) = &self.x11
        {
            x.ungrab();
        }
    }

    pub fn resume(&self) {
        self.suspended.set(false);
        if self.mode.get() == Mode::X11 && !self.paused.get() {
            let accel = self.accel();
            self.register_x11(&accel);
        }
    }
}
