//! X11 会话：全局快捷键（XGrabKey）、前台窗口（_NET_ACTIVE_WINDOW + WM_CLASS）、模拟粘贴（XTest）、
//! 面板窗口的位置与窗口属性。全部经一个 x11rb 连接；事件在后台线程读取，经通道交给主线程。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::hotkey::{Failure, Phase};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _, EventMask, GrabMode, ModMask, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

/// 从后台线程交给主线程的事件
pub enum XEvent {
    /// 快捷键按下（X 服务器时间戳）
    HotKey {
        generation: u64,
        phase: Phase,
        timestamp: u32,
    },
    Status {
        generation: u64,
        result: Result<(), Failure>,
    },
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
    grabs: Mutex<Grabs>,
    revision: AtomicU64,
    registered: AtomicBool,
    detectable_repeat: bool,
    xwayland: bool,
    stopping: AtomicBool,
    wake_window: Window,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Grab {
    code: u8,
    mods: u16,
}

#[derive(Clone)]
struct Binding {
    keysym: u32,
    modifiers: Modifiers,
    combinations: Vec<Grab>,
}

#[derive(Default)]
struct Grabs {
    binding: Option<Binding>,
    generation: u64,
    held: Option<u8>,
    /// `held` 来自抓取建立时的键盘状态：这次按下发生在抓取之前，它的释放事件不会发给本程序
    /// （被动抓取只在按下时激活）。事件线程改为轮询键盘状态，直到这个键松开。
    held_unseen: bool,
}

/// Add first, remove last. A failed partial registration only removes newly added
/// combinations, never overlaps with (or removes) the still-working old binding.
fn replace_grabs(
    old: &[Grab],
    new: &[Grab],
    mut grab: impl FnMut(Grab) -> Result<(), Failure>,
    mut ungrab: impl FnMut(Grab) -> Result<(), Failure>,
) -> Result<(), Failure> {
    let mut added = Vec::new();
    for &combination in new {
        if old.contains(&combination) {
            continue;
        }
        if let Err(error) = grab(combination) {
            let mut cleanup = Ok(());
            for combination in added {
                if let Err(error) = ungrab(combination) {
                    cleanup = Err(error);
                }
            }
            cleanup?;
            return Err(error);
        }
        added.push(combination);
    }
    for &combination in old {
        if !new.contains(&combination) {
            ungrab(combination)?;
        }
    }
    Ok(())
}

fn lock_variants(mask: u16) -> Vec<u16> {
    let mut variants = vec![0];
    for bit in 0..8 {
        if mask & (1 << bit) != 0 {
            let more: Vec<_> = variants.iter().map(|v| v | (1 << bit)).collect();
            variants.extend(more);
        }
    }
    variants
}

fn reply_failure(error: x11rb::errors::ReplyError) -> Failure {
    match error {
        x11rb::errors::ReplyError::X11Error(e) if e.error_kind == x11rb::protocol::ErrorKind::Access => Failure::Conflict,
        x11rb::errors::ReplyError::ConnectionError(_) => Failure::Disconnected,
        _ => Failure::Temporary,
    }
}

fn atom(conn: &RustConnection, name: &str) -> u32 {
    conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom).unwrap_or(0)
}

impl X11 {
    /// 连接 X 服务器并启动事件线程。`tx` 接收快捷键与前台窗口变化。
    pub fn connect(tx: async_channel::Sender<XEvent>) -> Option<Arc<X11>> {
        let (conn, screen) = RustConnection::connect(None).ok()?;
        let xwayland = conn.query_extension(b"XWAYLAND").ok()?.reply().ok()?.present;
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
        // XKB detectable repeat emits repeated KeyPress without synthetic releases.
        let xkb_available = conn.xkb_use_extension(1, 0).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.supported);
        let detectable_repeat = xkb_available
            && conn
                .xkb_per_client_flags(
                    xkb::ID::USE_CORE_KBD.into(),
                    xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
                    xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
                    0u8.into(),
                    0u8.into(),
                    0u8.into(),
                )
                .ok()
                .and_then(|c| c.reply().ok())
                .is_some_and(|r| r.value.contains(xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT));
        if xkb_available {
            let _ = conn
                .xkb_select_events(
                    xkb::ID::USE_CORE_KBD.into(),
                    0u8.into(),
                    xkb::EventType::NEW_KEYBOARD_NOTIFY | xkb::EventType::MAP_NOTIFY,
                    xkb::MapPart::KEY_SYMS | xkb::MapPart::MODIFIER_MAP,
                    xkb::MapPart::KEY_SYMS | xkb::MapPart::MODIFIER_MAP,
                    &Default::default(),
                )
                .and_then(|c| c.check().map_err(|_| x11rb::errors::ConnectionError::UnknownError));
        }
        let wake_window = conn.generate_id().ok()?;
        conn.create_window(0, wake_window, root, 0, 0, 1, 1, 0, xproto::WindowClass::INPUT_ONLY, 0, &Default::default()).ok()?.check().ok()?;
        let _ = conn.flush();
        let x = Arc::new(X11 {
            conn: Arc::new(conn),
            root,
            atoms,
            grabs: Mutex::new(Grabs::default()),
            revision: AtomicU64::new(0),
            registered: AtomicBool::new(false),
            detectable_repeat,
            xwayland,
            stopping: AtomicBool::new(false),
            wake_window,
        });
        let reader = x.clone();
        std::thread::Builder::new().name("qantrip-x11".into()).spawn(move || reader.event_loop(tx)).ok()?;
        Some(x)
    }

    fn event_loop(&self, tx: async_channel::Sender<XEvent>) {
        let _ = tx.send_blocking(XEvent::Active(self.active_window()));
        let mut queued = None;
        while !self.stopping.load(Ordering::Acquire) {
            let next = match queued.take() {
                Some(event) => Ok(event),
                None if self.grabs.lock().unwrap().held_unseen => match self.conn.poll_for_event() {
                    Ok(Some(event)) => Ok(event),
                    Ok(None) => {
                        self.check_unseen_release();
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        continue;
                    }
                    Err(e) => Err(e),
                },
                None => self.conn.wait_for_event(),
            };
            let event = match next {
                Ok(event) => event,
                Err(_) => {
                    let mut state = self.grabs.lock().unwrap();
                    state.binding = None;
                    self.registered.store(false, Ordering::Release);
                    state.held = None;
                    self.advance_generation(&mut state);
                    let _ = tx.send_blocking(XEvent::Status { generation: state.generation, result: Err(Failure::Disconnected) });
                    return;
                }
            };
            match event {
                Event::KeyPress(key) => {
                    let mut state = self.grabs.lock().unwrap();
                    let mods = u16::from(key.state) & 0xff; // Button masks are not keyboard modifiers.
                    let matches = state.binding.as_ref().is_some_and(|b| b.combinations.contains(&Grab { code: key.detail, mods }));
                    if matches && state.held_unseen && state.held == Some(key.detail) {
                        // 抓取前就按着的键在自动重复：抓取已激活，之后的释放会发给本程序
                        state.held_unseen = false;
                    }
                    if matches && state.held.is_none() {
                        state.held = Some(key.detail);
                        if tx.send_blocking(XEvent::HotKey { generation: state.generation, phase: Phase::Pressed, timestamp: key.time }).is_err() {
                            return;
                        }
                    }
                }
                Event::KeyRelease(key) => {
                    if !self.detectable_repeat {
                        // Core repeat is a release/press pair with identical keycode and time.
                        // A round trip queues the following event even if it was split across reads.
                        let _ = self.conn.get_input_focus().ok().and_then(|c| c.reply().ok());
                        queued = self.conn.poll_for_event().ok().flatten();
                        if matches!(&queued, Some(Event::KeyPress(next)) if repeat_pair(key.detail, key.time, next.detail, next.time)) {
                            queued = None;
                            continue;
                        }
                    }
                    let mut state = self.grabs.lock().unwrap();
                    if state.held == Some(key.detail) {
                        state.held = None; // Release must work even if a modifier was released first.
                        state.held_unseen = false;
                        if tx.send_blocking(XEvent::HotKey { generation: state.generation, phase: Phase::Released, timestamp: key.time }).is_err() {
                            return;
                        }
                    }
                }
                Event::MappingNotify(_) | Event::XkbMapNotify(_) | Event::XkbNewKeyboardNotify(_) => {
                    let mut state = self.grabs.lock().unwrap();
                    if let Some(binding) = state.binding.clone() {
                        let result = self.replace_binding(&mut state, binding.keysym, binding.modifiers);
                        if result.is_err() {
                            self.clear_grabs(&mut state);
                        }
                        let result = result.map_err(|_| Failure::MappingChanged);
                        let _ = tx.send_blocking(XEvent::Status { generation: state.generation, result });
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

    pub fn grab(&self, keysym: u32, modifiers: Modifiers) -> Result<(), Failure> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Failure::Disconnected);
        }
        self.replace_binding(&mut self.grabs.lock().unwrap(), keysym, modifiers)
    }

    fn replace_binding(&self, state: &mut Grabs, keysym: u32, modifiers: Modifiers) -> Result<(), Failure> {
        let combinations = self.resolve_grabs(keysym, modifiers)?;
        let old = state.binding.as_ref().map(|b| b.combinations.as_slice()).unwrap_or(&[]);
        let result = replace_grabs(
            old,
            &combinations,
            |g| {
                self.conn
                    .grab_key(false, self.root, ModMask::from(g.mods), g.code, GrabMode::ASYNC, GrabMode::ASYNC)
                    .map_err(|_| Failure::Disconnected)?
                    .check()
                    .map_err(reply_failure)
            },
            |g| self.release_grab(g),
        );
        if let Err(error) = result {
            if error != Failure::Conflict {
                // An interrupted rollback/removal cannot be advertised as a valid old grab.
                for &combination in &combinations {
                    let _ = self.release_grab(combination);
                }
                self.clear_grabs(state);
            }
            return Err(error);
        }
        state.binding = Some(Binding { keysym, modifiers, combinations });
        // A rebind/resume while the key is held must wait for its release.
        state.held =
            self.conn.query_keymap().ok().and_then(|c| c.reply().ok()).and_then(|map| {
                state.binding.as_ref()?.combinations.iter().find(|g| map.keys[usize::from(g.code / 8)] & (1 << (g.code % 8)) != 0).map(|g| g.code)
            });
        state.held_unseen = state.held.is_some();
        if state.held_unseen {
            // 唤醒阻塞在 wait_for_event 的事件线程，让它改为轮询这个键何时松开
            let event = xproto::ClientMessageEvent::new(32, self.wake_window, AtomEnum::STRING, [0; 5]);
            let _ = self.conn.send_event(false, self.wake_window, EventMask::NO_EVENT, event);
        }
        self.advance_generation(state);
        if self.conn.flush().is_err() {
            self.clear_grabs(state);
            return Err(Failure::Disconnected);
        }
        self.registered.store(true, Ordering::Release);
        Ok(())
    }

    fn release_grab(&self, grab: Grab) -> Result<(), Failure> {
        self.conn.ungrab_key(grab.code, self.root, ModMask::from(grab.mods)).map_err(|_| Failure::Disconnected)?.check().map_err(reply_failure)
    }

    fn clear_grabs(&self, state: &mut Grabs) {
        self.registered.store(false, Ordering::Release);
        if let Some(binding) = state.binding.take() {
            for grab in binding.combinations {
                let _ = self.release_grab(grab);
            }
        }
        state.held = None;
        state.held_unseen = false;
        self.advance_generation(state);
        let _ = self.conn.flush();
    }

    /// 抓取前就按着的键松开后清除按住状态，下一次按下才能触发。
    fn check_unseen_release(&self) {
        let mut state = self.grabs.lock().unwrap();
        let Some(code) = state.held.filter(|_| state.held_unseen) else { return };
        let down = self.conn.query_keymap().ok().and_then(|c| c.reply().ok()).map(|map| map.keys[usize::from(code / 8)] & (1 << (code % 8)) != 0);
        if down == Some(false) {
            state.held = None;
            state.held_unseen = false;
        }
    }

    pub fn ungrab(&self) {
        self.clear_grabs(&mut self.grabs.lock().unwrap());
    }

    fn advance_generation(&self, state: &mut Grabs) {
        state.generation += 1;
        self.revision.store(state.generation, Ordering::Release);
    }

    pub fn is_xwayland(&self) -> bool {
        self.xwayland
    }

    pub fn is_grabbed(&self) -> bool {
        self.registered.load(Ordering::Acquire)
    }

    pub fn accepts(&self, generation: u64) -> bool {
        !self.stopping.load(Ordering::Acquire) && self.revision.load(Ordering::Acquire) == generation
    }

    /// Stop only on application exit: pauses never stop this shared connection.
    pub fn shutdown(&self) {
        if !self.stopping.swap(true, Ordering::AcqRel) {
            let event = xproto::ClientMessageEvent::new(32, self.wake_window, AtomEnum::STRING, [0; 5]);
            let _ = self.conn.send_event(false, self.wake_window, EventMask::NO_EVENT, event);
            let _ = self.conn.flush();
        }
    }

    fn resolve_grabs(&self, keysym: u32, modifiers: Modifiers) -> Result<Vec<Grab>, Failure> {
        let (min, per, syms) = self.keyboard_mapping().ok_or(Failure::Disconnected)?;
        if per == 0 {
            return Err(Failure::InvalidShortcut);
        }
        let mapping = self.conn.get_modifier_mapping().map_err(|_| Failure::Disconnected)?.reply().map_err(reply_failure)?;
        let per_mod = mapping.keycodes.len() / 8;
        if per_mod == 0 {
            return Err(Failure::InvalidShortcut);
        }
        let mut modifier_symbols = [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for (slot, codes) in mapping.keycodes.chunks(per_mod).enumerate() {
            for &code in codes {
                if code >= min {
                    let start = usize::from(code - min) * usize::from(per);
                    if let Some(symbols) = syms.get(start..start + usize::from(per)) {
                        modifier_symbols[slot].extend_from_slice(symbols);
                    }
                }
            }
        }
        resolve_mapping(min, per, &syms, &modifier_symbols, keysym, modifiers)
    }

    fn keyboard_mapping(&self) -> Option<(u8, u8, Vec<u32>)> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let r = self.conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
        if r.keysyms_per_keycode == 0 {
            return None;
        }
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

fn repeat_pair(release_code: u8, release_time: u32, press_code: u8, press_time: u32) -> bool {
    release_code == press_code && release_time == press_time
}

/// Resolve actual modifier slots (including remapped locks, Alt and Super).
fn resolve_mapping(min: u8, per: u8, syms: &[u32], modifier_symbols: &[Vec<u32>; 8], keysym: u32, modifiers: Modifiers) -> Result<Vec<Grab>, Failure> {
    let slots = |symbols: &[u32]| -> Vec<u16> {
        modifier_symbols.iter().enumerate().filter_map(|(i, slot)| slot.iter().any(|s| symbols.contains(s)).then_some(1 << i)).collect()
    };
    let lock_mask = slots(&[0xffe5, 0xffe6, 0xff7f, 0xff14]).into_iter().fold(0, |a, b| a | b);
    let mut masks = vec![0u16];
    for (enabled, symbols) in
        [(modifiers.control, [0xffe3, 0xffe4]), (modifiers.shift, [0xffe1, 0xffe2]), (modifiers.alt, [0xffe9, 0xffea]), (modifiers.super_key, [0xffeb, 0xffec])]
    {
        if enabled {
            let alternatives = slots(&symbols);
            if alternatives.is_empty() {
                return Err(Failure::InvalidShortcut);
            }
            masks = masks.iter().flat_map(|mask| alternatives.iter().map(move |m| mask | m)).collect();
        }
    }
    let mut grabs = std::collections::BTreeSet::new();
    for (i, symbols) in syms.chunks(usize::from(per)).enumerate() {
        // Core mapping exposes base/shift levels for each group. A shifted symbol
        // (e.g. '+') requires Shift even when the stored accelerator omits it.
        for (level, &symbol) in symbols.iter().enumerate() {
            if symbol != keysym {
                continue;
            }
            let implicit_shift = level % 2 == 1 && !(0x41..=0x5a).contains(&keysym);
            let shift_masks = if implicit_shift { slots(&[0xffe1, 0xffe2]) } else { vec![0] };
            for &mask in &masks {
                for &shift in &shift_masks {
                    let mask = mask | shift;
                    for lock in lock_variants(lock_mask & !mask) {
                        grabs.insert(Grab { code: min + i as u8, mods: mask | lock });
                    }
                }
            }
        }
    }
    if grabs.is_empty() { Err(Failure::InvalidShortcut) } else { Ok(grabs.into_iter().collect()) }
}

#[cfg(test)]
mod hotkey_tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    #[ignore = "requires an isolated X11 server; run tests/linux/hotkeys_smoke.py"]
    fn x11_server_regression() {
        assert_eq!(std::env::var("QANTRIP_TEST_X11").as_deref(), Ok("1"));
        let (tx, rx) = async_channel::unbounded();
        let x = X11::connect(tx).expect("isolated X server");
        let modifiers = Modifiers { control: true, ..Default::default() };
        let q = x.keycode_for(0x71).unwrap();
        let ctrl = x.keycode_for(0xffe3).unwrap();
        let target = x.conn.generate_id().unwrap();
        x.conn
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                target,
                x.root,
                0,
                0,
                100,
                100,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                0,
                &xproto::CreateWindowAux::new().override_redirect(1),
            )
            .unwrap()
            .check()
            .unwrap();
        x.conn.map_window(target).unwrap().check().unwrap();
        x.conn.set_input_focus(xproto::InputFocus::PARENT, target, x11rb::CURRENT_TIME).unwrap().check().unwrap();
        x.grab(0x71, modifiers).unwrap();

        let (other, _) = RustConnection::connect(None).unwrap();
        let proposed = x.resolve_grabs(0x77, modifiers).unwrap();
        let conflict = *proposed.last().unwrap();
        other.grab_key(false, x.root, conflict.mods.into(), conflict.code, GrabMode::ASYNC, GrabMode::ASYNC).unwrap().check().unwrap();
        assert_eq!(x.grab(0x77, modifiers), Err(Failure::Conflict));
        assert_eq!(x.grabs.lock().unwrap().binding.as_ref().unwrap().keysym, 0x71);
        for grab in proposed.iter().filter(|g| **g != conflict) {
            other.grab_key(false, x.root, grab.mods.into(), grab.code, GrabMode::ASYNC, GrabMode::ASYNC).unwrap().check().unwrap();
            other.ungrab_key(grab.code, x.root, grab.mods.into()).unwrap().check().unwrap();
        }
        if x.is_xwayland() {
            // Xwayland routes XTest via the compositor. On a virtual seat use
            // controlled wire events; this is not a real physical-key test.
            assert!(x.detectable_repeat);
            let event = xproto::KeyPressEvent {
                response_type: xproto::KEY_PRESS_EVENT,
                detail: q,
                sequence: 0,
                time: 123,
                root: x.root,
                event: x.wake_window,
                child: 0,
                root_x: 0,
                root_y: 0,
                event_x: 0,
                event_y: 0,
                state: xproto::KeyButMask::CONTROL,
                same_screen: true,
            };
            for _ in 0..4 {
                x.conn.send_event(false, x.wake_window, EventMask::NO_EVENT, event).unwrap().check().unwrap();
            }
            x.conn
                .send_event(
                    false,
                    x.wake_window,
                    EventMask::NO_EVENT,
                    xproto::KeyReleaseEvent { response_type: xproto::KEY_RELEASE_EVENT, state: 0u8.into(), time: 124, ..event },
                )
                .unwrap()
                .check()
                .unwrap();
        } else {
            // On a native isolated X server, exercise the actual autorepeat timer.
            x.conn.xtest_fake_input(2, ctrl, 0, x.root, 0, 0, 0).unwrap();
            x.conn.xtest_fake_input(2, q, 0, x.root, 0, 0, 0).unwrap();
            x.conn.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(850));
            x.conn.xtest_fake_input(3, ctrl, 0, x.root, 0, 0, 0).unwrap();
            x.conn.xtest_fake_input(3, q, 0, x.root, 0, 0, 0).unwrap();
            x.conn.flush().unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut phases = Vec::new();
        while std::time::Instant::now() < deadline {
            while let Ok(event) = rx.try_recv() {
                if let XEvent::HotKey { phase, timestamp, .. } = event {
                    assert_ne!(timestamp, 0);
                    phases.push(phase);
                }
            }
            if phases.last() == Some(&Phase::Released) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(phases, [Phase::Pressed, Phase::Released]);
        other.ungrab_key(conflict.code, x.root, conflict.mods.into()).unwrap().check().unwrap();
        x.grab(0x77, modifiers).unwrap();
        x.ungrab();
        // Pause frees all combinations without breaking the shared connection.
        for grab in proposed {
            other.grab_key(false, x.root, grab.mods.into(), grab.code, GrabMode::ASYNC, GrabMode::ASYNC).unwrap().check().unwrap();
        }
        assert!(x.pointer().is_some());
        assert!(x.keycode_for(0x71).is_some());
        let _ = x.active_window();
        // Exercise the main-thread coordinator and serial worker against the same
        // isolated server, including recording rollback and pause/resume.
        gtk::init().unwrap();
        let context = gtk::glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    use crate::hotkey::{HotKey, Mode, State};
                    use crate::settings::DEFAULT_HOTKEY;
                    assert_eq!(crate::hotkey::portal_trigger(DEFAULT_HOTKEY).as_deref(), Some("CTRL+SHIFT+q"));
                    let hotkey = HotKey::new(Mode::X11, DEFAULT_HOTKEY.into(), Some(x.clone()));
                    hotkey.start();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                    while matches!(hotkey.state(), State::Pending) {
                        assert!(std::time::Instant::now() < deadline);
                        gtk::glib::timeout_future(std::time::Duration::from_millis(5)).await;
                    }
                    assert!(matches!(hotkey.state(), State::Active { .. }));
                    // Changing away from the new default and restoring it both
                    // go through the same registration path as the settings UI.
                    hotkey.suspend();
                    hotkey.try_register("<Control>q").await.unwrap();
                    assert_eq!(hotkey.accel(), "<Control>q");
                    hotkey.try_register(DEFAULT_HOTKEY).await.unwrap();
                    assert_eq!(hotkey.accel(), DEFAULT_HOTKEY);
                    assert_eq!(hotkey.try_register("<Control>w").await, Err(Failure::Conflict));
                    assert_eq!(hotkey.accel(), DEFAULT_HOTKEY);
                    assert!(matches!(hotkey.state(), State::Active { .. }));
                    hotkey.suspend();
                    assert_eq!(hotkey.try_register("<Control>w").await, Err(Failure::Conflict));
                    assert_eq!(hotkey.accel(), DEFAULT_HOTKEY);
                    assert!(matches!(hotkey.state(), State::Active { .. }));
                    hotkey.set_paused(true);
                    hotkey.set_paused(false);
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                    while matches!(hotkey.state(), State::Pending) {
                        assert!(std::time::Instant::now() < deadline);
                        gtk::glib::timeout_future(std::time::Duration::from_millis(5)).await;
                    }
                    assert!(matches!(hotkey.state(), State::Active { .. }));
                    // A scheduled resume cannot re-register after shutdown.
                    hotkey.suspend();
                    hotkey.resume();
                    hotkey.shutdown().await;
                    assert_eq!(hotkey.state(), State::Closed);
                    assert!(!x.is_grabbed());
                })
            })
            .unwrap();
        x.shutdown();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !rx.is_closed() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(rx.is_closed(), "X11 reader did not stop");
        x.conn.destroy_window(target).unwrap().check().unwrap();
    }

    #[test]
    fn remapped_locks_and_modifiers_are_discovered() {
        let mut map: [Vec<u32>; 8] = Default::default();
        map[0] = vec![0xffe1];
        map[2] = vec![0xffe3];
        map[3] = vec![0xff7f]; // NumLock on Mod1, not Mod2
        map[5] = vec![0xffe9]; // Alt on Mod3
        map[7] = vec![0xffe5]; // CapsLock on Mod5
        let grabs = resolve_mapping(24, 2, &[0x71, 0x51], &map, 0x71, Modifiers { control: true, alt: true, ..Default::default() }).unwrap();
        assert_eq!(grabs.iter().map(|g| g.mods).collect::<Vec<_>>(), vec![36, 44, 164, 172]);
        assert!(grabs.iter().all(|g| g.code == 24));
    }

    #[test]
    fn failed_partial_rebind_keeps_old_and_cleans_only_new_combinations() {
        let old = vec![Grab { code: 24, mods: 4 }, Grab { code: 24, mods: 6 }];
        let new = vec![old[1], Grab { code: 25, mods: 4 }, Grab { code: 25, mods: 6 }];
        let server = RefCell::new(old.clone());
        let result = replace_grabs(
            &old,
            &new,
            |g| {
                if g == new[2] {
                    return Err(Failure::Conflict);
                }
                server.borrow_mut().push(g);
                Ok(())
            },
            |g| {
                server.borrow_mut().retain(|v| *v != g);
                Ok(())
            },
        );
        assert_eq!(result, Err(Failure::Conflict));
        assert_eq!(*server.borrow(), old);
    }

    #[test]
    fn duplicate_registration_is_a_noop_on_the_server() {
        let grabs = vec![Grab { code: 24, mods: 4 }];
        assert_eq!(replace_grabs(&grabs, &grabs, |_| panic!("duplicate grab"), |_| panic!("removed old grab")), Ok(()));
    }

    #[test]
    fn successful_rebind_removes_old_combinations() {
        let old = vec![Grab { code: 24, mods: 4 }];
        let new = vec![Grab { code: 25, mods: 4 }];
        let server = RefCell::new(old.clone());
        replace_grabs(
            &old,
            &new,
            |g| {
                server.borrow_mut().push(g);
                Ok(())
            },
            |g| {
                server.borrow_mut().retain(|v| *v != g);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*server.borrow(), new);
    }

    #[test]
    fn core_repeat_pair_requires_both_key_and_timestamp() {
        assert!(repeat_pair(24, 100, 24, 100));
        assert!(!repeat_pair(24, 100, 24, 101));
        assert!(!repeat_pair(24, 100, 25, 100));
        assert_eq!(lock_variants(0), vec![0]);
        assert_eq!(lock_variants(0x90), vec![0, 16, 128, 144]);
    }
}
