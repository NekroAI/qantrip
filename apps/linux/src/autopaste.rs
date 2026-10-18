//! 模拟粘贴按键（docs/decisions.md D37）。
//!
//! - X11：XTest，不需要任何权限。
//! - Wayland：RemoteDesktop 门户。首次使用由用户在系统对话框中授权，恢复令牌保存在本机，之后不再询问；
//!   每次粘贴临时建立一个只含键盘的会话，发送按键后立即关闭（不常驻"远程桌面"指示）。
//! - wlroots 系合成器：没有门户时使用 wtype（virtual-keyboard 协议），尽力支持。
//! - 都不可用或未授权：只复制，由用户按 Ctrl+V。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtk::glib;

use crate::paths;
use crate::x11::X11;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    XTest,
    Portal,
    Wtype,
    None,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PortalState {
    /// 未检查
    Unknown,
    /// 门户可用但还没有授权（没有恢复令牌）
    NotAuthorized,
    /// 已授权（有恢复令牌）
    Authorized,
    /// 正在请求授权（系统对话框）
    Requesting,
    /// 用户拒绝或授权失败
    Denied(String),
    /// 本桌面没有 RemoteDesktop 门户
    Unavailable,
}

/// 组合键（keysym）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Combo {
    pub mods: &'static [u32],
    pub key: u32,
}

pub const KEY_CONTROL: u32 = 0xffe3;
pub const KEY_SHIFT: u32 = 0xffe1;
pub const CTRL_V: Combo = Combo { mods: &[KEY_CONTROL], key: 0x76 };
pub const CTRL_SHIFT_V: Combo = Combo { mods: &[KEY_CONTROL, KEY_SHIFT], key: 0x76 };
pub const SHIFT_INSERT: Combo = Combo { mods: &[KEY_SHIFT], key: 0xff63 };

impl Combo {
    pub fn label(&self) -> &'static str {
        match (self.mods.len(), self.key) {
            (2, _) => "Ctrl+Shift+V",
            (1, 0xff63) => "Shift+Insert",
            _ => "Ctrl+V",
        }
    }
}

pub struct AutoPaste {
    x11: Option<Arc<X11>>,
    wayland: bool,
    wtype: bool,
    portal: RefCell<PortalState>,
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
    busy: Cell<bool>,
    /// KDE：保持一个会话（每次建立会话 KDE 都会弹出"远程控制会话已开始"通知）；
    /// GNOME：每次粘贴临时建立（会话存在期间顶栏一直显示远程控制指示）
    keep_session: bool,
    session: RefCell<Option<Rc<Held>>>,
}

type RdProxy = ashpd::desktop::remote_desktop::RemoteDesktop;
type RdSession = ashpd::desktop::Session<RdProxy>;

struct Held {
    proxy: RdProxy,
    session: RdSession,
}

/// 恢复令牌按桌面保存（GNOME 与 KDE 的令牌互不通用）
fn token_file() -> std::path::PathBuf {
    let desktop: String =
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    paths::state_dir().join(if desktop.is_empty() { "remote-desktop-token".to_string() } else { format!("remote-desktop-token-{desktop}") })
}

fn load_token() -> Option<String> {
    std::fs::read_to_string(token_file()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn save_token(t: Option<&str>) {
    match t {
        Some(t) => {
            let _ = paths::write_private(&token_file(), t.as_bytes());
        }
        None => {
            let _ = std::fs::remove_file(token_file());
        }
    }
}

impl AutoPaste {
    pub fn new(x11: Option<Arc<X11>>, wayland: bool, wlroots: bool, keep_session: bool) -> Rc<AutoPaste> {
        // wtype 依赖 virtual-keyboard 协议，只有 wlroots 系合成器支持（GNOME / KDE 不支持）
        let wtype = wayland && wlroots && glib::find_program_in_path("wtype").is_some();
        let portal = if !wayland {
            PortalState::Unavailable
        } else if load_token().is_some() {
            PortalState::Authorized
        } else {
            PortalState::Unknown
        };
        Rc::new(AutoPaste {
            x11,
            wayland,
            wtype,
            portal: RefCell::new(portal),
            listeners: RefCell::new(Vec::new()),
            busy: Cell::new(false),
            keep_session,
            session: RefCell::new(None),
        })
    }

    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    fn set_portal(&self, s: PortalState) {
        *self.portal.borrow_mut() = s;
        let list: Vec<_> = self.listeners.borrow().clone();
        for f in list {
            f();
        }
    }

    pub fn portal_state(&self) -> PortalState {
        self.portal.borrow().clone()
    }

    /// 当前可用的方式
    pub fn method(&self) -> Method {
        if !self.wayland {
            return if self.x11.is_some() { Method::XTest } else { Method::None };
        }
        if *self.portal.borrow() == PortalState::Authorized {
            return Method::Portal;
        }
        if self.wtype {
            return Method::Wtype;
        }
        Method::None
    }

    /// 启动时检查门户是否存在（不弹出任何对话框）
    pub fn probe(self: &Rc<Self>) {
        if !self.wayland || *self.portal.borrow() != PortalState::Unknown {
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let state = match ashpd::desktop::remote_desktop::RemoteDesktop::new().await {
                Ok(p) => match p.available_device_types().await {
                    Ok(types) if types.contains(ashpd::desktop::remote_desktop::DeviceType::Keyboard) => PortalState::NotAuthorized,
                    _ => PortalState::Unavailable,
                },
                Err(_) => PortalState::Unavailable,
            };
            this.set_portal(state);
        });
    }

    /// 请求授权（系统对话框，用户可勾选"记住"）。成功后保存恢复令牌。
    pub fn authorize(self: &Rc<Self>) {
        if !self.wayland {
            return;
        }
        let this = self.clone();
        self.set_portal(PortalState::Requesting);
        glib::spawn_future_local(async move {
            let r = this.session(&[]).await;
            match r {
                Ok(_) => this.set_portal(PortalState::Authorized),
                Err(e) => {
                    crate::log::event("autopaste_authorize_failed", serde_json::json!({"error": e}));
                    this.set_portal(PortalState::Denied(e));
                }
            }
        });
    }

    /// 撤销：删除恢复令牌（系统中的授权可在系统设置中撤销）
    pub fn forget(&self) {
        if let Some(h) = self.session.borrow_mut().take() {
            glib::spawn_future_local(async move {
                let _ = h.session.close().await;
            });
        }
        save_token(None);
        if self.wayland {
            self.set_portal(PortalState::NotAuthorized);
        }
    }

    /// 建立键盘会话（使用保存的恢复令牌，系统不再询问）
    async fn open_session(&self) -> Result<Held, String> {
        use ashpd::desktop::PersistMode;
        use ashpd::desktop::remote_desktop::{DeviceType, SelectDevicesOptions};
        let proxy = RdProxy::new().await.map_err(|e| e.to_string())?;
        let session = proxy.create_session(Default::default()).await.map_err(|e| e.to_string())?;
        let token = load_token();
        let opts = SelectDevicesOptions::default()
            .set_devices(enumflags2::BitFlags::from(DeviceType::Keyboard))
            .set_persist_mode(PersistMode::ExplicitlyRevoked)
            .set_restore_token(token.as_deref());
        proxy.select_devices(&session, opts).await.map_err(|e| e.to_string())?.response().map_err(|e| e.to_string())?;
        let selected = proxy.start(&session, None, Default::default()).await.map_err(|e| e.to_string())?.response().map_err(|e| e.to_string())?;
        save_token(selected.restore_token());
        if !selected.devices().contains(DeviceType::Keyboard) {
            let _ = session.close().await;
            return Err("keyboard not granted".into());
        }
        Ok(Held { proxy, session })
    }

    async fn press(held: &Held, combo: Combo) -> Result<(), String> {
        use ashpd::desktop::remote_desktop::KeyState;
        let (p, s) = (&held.proxy, &held.session);
        for m in combo.mods {
            p.notify_keyboard_keysym(s, *m as i32, KeyState::Pressed, Default::default()).await.map_err(|e| e.to_string())?;
        }
        p.notify_keyboard_keysym(s, combo.key as i32, KeyState::Pressed, Default::default()).await.map_err(|e| e.to_string())?;
        p.notify_keyboard_keysym(s, combo.key as i32, KeyState::Released, Default::default()).await.map_err(|e| e.to_string())?;
        for m in combo.mods.iter().rev() {
            p.notify_keyboard_keysym(s, *m as i32, KeyState::Released, Default::default()).await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 建立（或复用）键盘会话并依次发送组合键；返回建立会话的耗时
    async fn session(&self, combos: &[(Combo, Duration)]) -> Result<Duration, String> {
        let t0 = Instant::now();
        let existing = self.session.borrow().clone();
        let (held, reused) = match existing {
            Some(h) => (h, true),
            None => (Rc::new(self.open_session().await?), false),
        };
        let ready = t0.elapsed();
        let mut result = Ok(());
        for (i, (combo, wait)) in combos.iter().enumerate() {
            if i > 0 || !wait.is_zero() {
                glib::timeout_future(*wait).await;
            }
            if let Err(e) = Self::press(&held, *combo).await {
                result = Err(e);
                break;
            }
        }
        if result.is_err() && reused {
            // 保持的会话已失效（如系统撤销）：重新建立一次
            self.session.borrow_mut().take();
            let fresh = Rc::new(self.open_session().await?);
            for (i, (combo, wait)) in combos.iter().enumerate() {
                if i > 0 || !wait.is_zero() {
                    glib::timeout_future(*wait).await;
                }
                Self::press(&fresh, *combo).await?;
            }
            if self.keep_session {
                *self.session.borrow_mut() = Some(fresh);
            } else {
                glib::timeout_future(Duration::from_millis(60)).await;
                let _ = fresh.session.close().await;
            }
            return Ok(ready);
        }
        result?;
        if self.keep_session {
            *self.session.borrow_mut() = Some(held);
        } else {
            // 留出时间让合成器把按键交给目标应用，再关闭会话
            glib::timeout_future(Duration::from_millis(60)).await;
            let _ = held.session.close().await;
        }
        Ok(ready)
    }

    /// 发送组合键（可多次，间隔 `gap`）。返回是否成功与耗时说明。
    pub async fn send(self: &Rc<Self>, combo: Combo, times: u32, gap: Duration) -> Result<(), String> {
        if self.busy.get() {
            return Err("busy".into());
        }
        self.busy.set(true);
        let r = self.send_inner(combo, times, gap).await;
        self.busy.set(false);
        r
    }

    async fn send_inner(self: &Rc<Self>, combo: Combo, times: u32, gap: Duration) -> Result<(), String> {
        match self.method() {
            Method::XTest => {
                let x = self.x11.clone().ok_or("no x11")?;
                for i in 0..times {
                    if i > 0 {
                        glib::timeout_future(gap).await;
                    }
                    if !x.send_combo(combo.mods, combo.key) {
                        return Err("xtest failed".into());
                    }
                }
                Ok(())
            }
            Method::Portal => {
                let list: Vec<(Combo, Duration)> = (0..times).map(|i| (combo, if i == 0 { Duration::ZERO } else { gap })).collect();
                match self.session(&list).await {
                    Ok(ready) => {
                        crate::log::event("autopaste_portal", serde_json::json!({"session_ms": crate::util::ms(ready.as_secs_f64())}));
                        Ok(())
                    }
                    Err(e) => {
                        // 令牌失效或被撤销：回到未授权状态，下次只复制并提示
                        if e.to_lowercase().contains("cancel") || e.contains("not granted") {
                            save_token(None);
                            self.set_portal(PortalState::NotAuthorized);
                        }
                        Err(e)
                    }
                }
            }
            Method::Wtype => {
                for i in 0..times {
                    if i > 0 {
                        glib::timeout_future(gap).await;
                    }
                    let mut args: Vec<String> = Vec::new();
                    for m in combo.mods {
                        args.push("-M".into());
                        args.push(if *m == KEY_SHIFT { "shift".into() } else { "ctrl".into() });
                    }
                    args.push("-k".into());
                    args.push(if combo.key == 0xff63 { "Insert".into() } else { "v".into() });
                    for m in combo.mods.iter().rev() {
                        args.push("-m".into());
                        args.push(if *m == KEY_SHIFT { "shift".into() } else { "ctrl".into() });
                    }
                    let status = crate::util::launcher()
                        .spawn(&std::iter::once(std::ffi::OsStr::new("wtype")).chain(args.iter().map(std::ffi::OsStr::new)).collect::<Vec<_>>())
                        .map_err(|e| e.to_string())?;
                    status.wait_check_future().await.map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            Method::None => Err("unavailable".into()),
        }
    }
}
