//! X11 会话：全局快捷键（XGrabKey）、前台窗口（_NET_ACTIVE_WINDOW + WM_CLASS）、模拟粘贴（XTest）、
//! 面板窗口的位置与窗口属性。全部经一个 x11rb 连接；事件在后台线程读取，经通道交给主线程。

use std::sync::{Arc, Mutex};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _, EventMask, GrabMode, ModMask, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

/// 从后台线程交给主线程的事件
pub enum XEvent {
    /// 快捷键按下（X 服务器时间戳）
    HotKey(u32),
    /// 前台窗口变化
    Active(Option<WindowInfo>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub window: u32,
    /// WM_CLASS 的类名（如 `firefox`、`Gnome-terminal`）
    pub class: String,
    /// WM_CLASS 的实例名
    pub instance: String,
    pub title: String,
    pub pid: Option<u32>,
}

struct Atoms {
    net_active_window: u32,
    net_wm_name: u32,
    net_wm_pid: u32,
    utf8_string: u32,
    net_wm_state: u32,
    net_wm_state_above: u32,
    net_wm_state_skip_taskbar: u32,
    net_wm_state_skip_pager: u32,
}

pub struct X11 {
    conn: Arc<RustConnection>,
    root: Window,
    atoms: Atoms,
    /// 当前抓取的 (键码, 修饰键)
    grabbed: Mutex<Option<(u8, u16)>>,
}

/// NumLock（Mod2）与 CapsLock（Lock）的组合：抓取时都要覆盖，否则开着 NumLock 时快捷键无效
const LOCK_VARIANTS: [u16; 4] = [0, 2, 16, 18];

fn atom(conn: &RustConnection, name: &str) -> u32 {
    conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom).unwrap_or(0)
}

impl X11 {
    /// 连接 X 服务器并启动事件线程。`tx` 接收快捷键与前台窗口变化。
    pub fn connect(tx: async_channel::Sender<XEvent>) -> Option<Arc<X11>> {
        let (conn, screen) = RustConnection::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atoms = Atoms {
            net_active_window: atom(&conn, "_NET_ACTIVE_WINDOW"),
            net_wm_name: atom(&conn, "_NET_WM_NAME"),
            net_wm_pid: atom(&conn, "_NET_WM_PID"),
            utf8_string: atom(&conn, "UTF8_STRING"),
            net_wm_state: atom(&conn, "_NET_WM_STATE"),
            net_wm_state_above: atom(&conn, "_NET_WM_STATE_ABOVE"),
            net_wm_state_skip_taskbar: atom(&conn, "_NET_WM_STATE_SKIP_TASKBAR"),
            net_wm_state_skip_pager: atom(&conn, "_NET_WM_STATE_SKIP_PAGER"),
        };
        // 监听根窗口属性变化（_NET_ACTIVE_WINDOW）
        let _ = conn.change_window_attributes(root, &xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE));
        let _ = conn.flush();
        let x = Arc::new(X11 { conn: Arc::new(conn), root, atoms, grabbed: Mutex::new(None) });
        let reader = x.clone();
        std::thread::Builder::new().name("qantrip-x11".into()).spawn(move || reader.event_loop(tx)).ok()?;
        Some(x)
    }

    fn event_loop(&self, tx: async_channel::Sender<XEvent>) {
        let _ = tx.send_blocking(XEvent::Active(self.active_window()));
        loop {
            let ev = match self.conn.wait_for_event() {
                Ok(e) => e,
                Err(_) => return,
            };
            match ev {
                Event::KeyPress(k) => {
                    let grabbed = *self.grabbed.lock().unwrap();
                    if let Some((code, mods)) = grabbed {
                        let state = u16::from(k.state) & !(2 | 16);
                        if k.detail == code && state == mods && tx.send_blocking(XEvent::HotKey(k.time)).is_err() {
                            return;
                        }
                    }
                }
                Event::PropertyNotify(p)
                    if p.window == self.root && p.atom == self.atoms.net_active_window && tx.send_blocking(XEvent::Active(self.active_window())).is_err() =>
                {
                    return;
                }
                _ => {}
            }
        }
    }

    // MARK: 快捷键

    /// 抓取快捷键：`keysym` 为 X keysym（与 GDK 键值相同），`mods` 为 X 修饰键掩码。
    /// 被其他程序占用时返回 false。
    pub fn grab(&self, keysym: u32, mods: u16) -> bool {
        self.ungrab();
        let Some(code) = self.keycode_for(keysym) else { return false };
        let mut ok = true;
        for extra in LOCK_VARIANTS {
            let m = ModMask::from(mods | extra);
            match self.conn.grab_key(true, self.root, m, code, GrabMode::ASYNC, GrabMode::ASYNC) {
                Ok(c) => {
                    if c.check().is_err() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        if !ok {
            for extra in LOCK_VARIANTS {
                let _ = self.conn.ungrab_key(code, self.root, ModMask::from(mods | extra));
            }
            let _ = self.conn.flush();
            return false;
        }
        let _ = self.conn.flush();
        *self.grabbed.lock().unwrap() = Some((code, mods));
        true
    }

    pub fn ungrab(&self) {
        if let Some((code, mods)) = self.grabbed.lock().unwrap().take() {
            for extra in LOCK_VARIANTS {
                let _ = self.conn.ungrab_key(code, self.root, ModMask::from(mods | extra));
            }
            let _ = self.conn.flush();
        }
    }

    fn keyboard_mapping(&self) -> Option<(u8, u8, Vec<u32>)> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let r = self.conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
        Some((min, r.keysyms_per_keycode, r.keysyms))
    }

    /// keysym → 键码（第一个产生该 keysym 的键，取不到时尝试大小写变体）
    pub fn keycode_for(&self, keysym: u32) -> Option<u8> {
        let (min, per, syms) = self.keyboard_mapping()?;
        let find = |ks: u32| syms.chunks(per as usize).position(|c| c.iter().take(2).any(|&s| s == ks)).map(|i| min + i as u8);
        find(keysym).or_else(|| {
            // 字母：大写 keysym 也对应同一个键
            if (0x61..=0x7a).contains(&keysym) {
                find(keysym - 0x20)
            } else if (0x41..=0x5a).contains(&keysym) {
                find(keysym + 0x20)
            } else {
                None
            }
        })
    }

    // MARK: 前台窗口

    pub fn active_window(&self) -> Option<WindowInfo> {
        let r = self.conn.get_property(false, self.root, self.atoms.net_active_window, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
        let w = r.value32()?.next()?;
        if w == 0 {
            return None;
        }
        self.window_info(w)
    }

    pub fn window_info(&self, w: u32) -> Option<WindowInfo> {
        let class = self.conn.get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256).ok()?.reply().ok()?;
        let parts: Vec<String> = class.value.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect();
        let instance = parts.first().cloned().unwrap_or_default();
        let class_name = parts.get(1).cloned().unwrap_or_else(|| instance.clone());
        let title = self
            .conn
            .get_property(false, w, self.atoms.net_wm_name, self.atoms.utf8_string, 0, 256)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .unwrap_or_default();
        let pid = self
            .conn
            .get_property(false, w, self.atoms.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()));
        Some(WindowInfo { window: w, class: class_name, instance, title, pid })
    }

    /// 当前活动窗口的 XID（插入前确认目标仍在前台）
    pub fn active_xid(&self) -> Option<u32> {
        let r = self.conn.get_property(false, self.root, self.atoms.net_active_window, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
        r.value32()?.next().filter(|w| *w != 0)
    }

    // MARK: 模拟按键（XTest）

    /// 按下组合键：`mods` 与 `key` 都是 keysym。先抬起仍按着的修饰键（用户可能还按着唤起快捷键）。
    pub fn send_combo(&self, mods: &[u32], key: u32) -> bool {
        const KEY_PRESS: u8 = 2;
        const KEY_RELEASE: u8 = 3;
        let Some(k) = self.keycode_for(key) else { return false };
        let mod_codes: Vec<u8> = mods.iter().filter_map(|m| self.keycode_for(*m)).collect();
        if mod_codes.len() != mods.len() {
            return false;
        }
        // 抬起所有常见修饰键，避免与用户仍按着的键组合成其他快捷键
        for ks in [0xffe3u32, 0xffe4, 0xffe1, 0xffe2, 0xffe9, 0xffea, 0xffeb, 0xffec] {
            if let Some(c) = self.keycode_for(ks) {
                let _ = self.conn.xtest_fake_input(KEY_RELEASE, c, 0, self.root, 0, 0, 0);
            }
        }
        for c in &mod_codes {
            let _ = self.conn.xtest_fake_input(KEY_PRESS, *c, 0, self.root, 0, 0, 0);
        }
        let _ = self.conn.xtest_fake_input(KEY_PRESS, k, 0, self.root, 0, 0, 0);
        let _ = self.conn.xtest_fake_input(KEY_RELEASE, k, 0, self.root, 0, 0, 0);
        for c in mod_codes.iter().rev() {
            let _ = self.conn.xtest_fake_input(KEY_RELEASE, *c, 0, self.root, 0, 0, 0);
        }
        self.conn.flush().is_ok()
    }

    // MARK: 面板窗口

    /// 置顶、不出现在任务栏与切换器中，并移动到指定位置。
    pub fn place_panel(&self, w: u32, x: i32, y: i32) {
        let _ = self.conn.configure_window(w, &xproto::ConfigureWindowAux::new().x(x).y(y));
        let a = &self.atoms;
        for state in [a.net_wm_state_above, a.net_wm_state_skip_taskbar, a.net_wm_state_skip_pager] {
            let ev = xproto::ClientMessageEvent::new(32, w, a.net_wm_state, [1, state, 0, 1, 0]);
            let _ = self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev);
        }
        let _ = self.conn.flush();
    }

    /// 鼠标所在位置（根窗口坐标）
    pub fn pointer(&self) -> Option<(i32, i32)> {
        let r = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        Some((r.root_x as i32, r.root_y as i32))
    }

    /// 把窗口激活（_NET_ACTIVE_WINDOW 请求，来源为应用、带时间戳）
    pub fn activate(&self, w: u32, time: u32) {
        let ev = xproto::ClientMessageEvent::new(32, w, self.atoms.net_active_window, [1, time, 0, 0, 0]);
        let _ = self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev);
        let _ = self.conn.flush();
    }
}
