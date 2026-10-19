//! 端到端加密同步的 Linux 外壳（docs/sync-design.md）。加密、花名册与协议都在 Rust 核心里；
//! 这里只负责：密钥存储（secrets.rs）、浏览器登录回调、在后台线程调用核心、按触发原因同步、
//! 把状态交给界面。与 macOS 的 SyncService.swift 逐项对应。

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gtk::gio;
use gtk::glib;

use qantrip_core::Locale;
use qantrip_core::sync::crypto::PhraseLanguage;
use qantrip_core::sync::{
    AccountDevice, AccountDevices, DeviceInfo, DeviceLabel, JoinProgress, JoinRequest, JoinRequestStatus, JoinStage, SyncClient, SyncError, SyncReport,
    SyncStatus, SyncTrigger,
};

use crate::i18n::{is_zh, l};
use crate::paths;
use crate::secrets::{Backend, LinuxSecretStore};
use crate::settings::Settings;
use crate::t;

/// 同步错误的界面文字（核心的错误信息是中文，这里按界面语言重新表述）
pub fn describe(e: &SyncError) -> String {
    match e {
        SyncError::NotSignedIn { .. } => l("需要重新登录", "Please sign in again"),
        SyncError::Network { .. } => l("无法连接同步服务器，请检查网络", "Can't reach the sync server. Check your connection."),
        SyncError::Server { status, code, message } => {
            if is_zh() {
                message.clone()
            } else {
                format!("Server error {status} ({code})")
            }
        }
        SyncError::NoVault { .. } => l("还没有保险库", "There is no vault yet"),
        SyncError::NotEnrolled { .. } => l("这台设备还没有加入保险库", "This device hasn't joined the vault"),
        SyncError::Security { message } => {
            if is_zh() {
                message.clone()
            } else {
                "Security check failed. Sync has been paused for safety.".into()
            }
        }
        SyncError::CodeMismatch { .. } => l(
            "选的码和新设备上显示的不一样，这次请求已取消。请在新设备上重新开始。",
            "That isn't the code shown on the new device, so the request was cancelled. Start again on the new device.",
        ),
        SyncError::RecoveryRequired { .. } => l("这个操作需要恢复短语", "This needs your recovery phrase"),
        SyncError::Conflict { .. } => l("其他设备同时在操作，请稍后重试", "Another device is busy. Try again shortly."),
        SyncError::InvalidInput { message } => {
            if is_zh() {
                message.clone()
            } else {
                "Invalid input".into()
            }
        }
        SyncError::InvalidState { message } => {
            if is_zh() {
                message.clone()
            } else {
                "Not possible right now".into()
            }
        }
        SyncError::Storage { message } => {
            if is_zh() {
                message.clone()
            } else {
                "Couldn't read or write the keyring".into()
            }
        }
        SyncError::DeviceLimit { max_devices } => {
            t!("此账号最多同时在 {max_devices} 台设备上同步", "This account can sync on up to {max_devices} devices at a time")
        }
    }
}

/// 本机设备信息（登录与刷新令牌时上报）
fn device_label() -> DeviceLabel {
    let name = std::fs::read_to_string("/proc/sys/kernel/hostname").map(|s| s.trim().to_string()).unwrap_or_default();
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let field = |k: &str| os.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(|v| v.trim_matches('"').to_string()).unwrap_or_default();
    let distro = format!("{} {}", field("NAME"), field("VERSION_ID")).trim().to_string();
    let platform = if distro.is_empty() { "Linux".to_string() } else { format!("Linux · {distro}") };
    DeviceLabel { name: if name.is_empty() { "Linux".into() } else { name }, platform, app_version: crate::VERSION.into() }
}

// MARK: - 浏览器登录回调

/// 在 127.0.0.1 的随机端口上接收一次 `/callback?code=…&state=…`（PKCE，RFC 8252）
struct Loopback {
    cancel: Arc<AtomicBool>,
}

enum LoopbackResult {
    Ok { code: String, state: String },
    Denied,
}

fn query_value(target: &str, name: &str) -> Option<String> {
    let q = target.split_once('?')?.1;
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == name {
            return Some(percent_decode(v));
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Loopback {
    /// 开始监听，返回 (回调地址, 结果通道)
    fn start() -> std::io::Result<(Loopback, String, async_channel::Receiver<LoopbackResult>)> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = async_channel::bounded(1);
        let c = cancel.clone();
        let zh = is_zh();
        std::thread::spawn(move || {
            while !c.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(150));
                        continue;
                    }
                    Err(_) => return,
                };
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = vec![0u8; 16 * 1024];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let target = req.lines().next().and_then(|l| l.split(' ').nth(1)).unwrap_or("").to_string();
                let is_cb = target.starts_with("/callback");
                if !is_cb {
                    // 无关请求（如 favicon）：404，继续等待
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let code = query_value(&target, "code");
                let state = query_value(&target, "state");
                let denied = query_value(&target, "error").is_some();
                let ok = code.is_some() && state.is_some();
                let page = match (ok, zh) {
                    (true, true) => "登录完成，可以关闭此页面并回到 Qantrip。",
                    (true, false) => "Signed in. You can close this tab and return to Qantrip.",
                    (false, true) => "登录没有完成，请回到 Qantrip 重试。",
                    (false, false) => "Sign-in didn't complete. Return to Qantrip and try again.",
                };
                let body = format!(
                    "<!doctype html><meta charset=utf-8><title>Qantrip</title><body style=\"font:16px system-ui,sans-serif;background:#0b0b0c;color:#ececee;display:grid;place-items:center;height:90vh\"><p>{page}</p>"
                );
                let resp =
                    format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(resp.as_bytes());
                if ok {
                    let _ = tx.send_blocking(LoopbackResult::Ok { code: code.unwrap(), state: state.unwrap() });
                    return;
                }
                if denied {
                    let _ = tx.send_blocking(LoopbackResult::Denied);
                    return;
                }
            }
        });
        Ok((Loopback { cancel }, format!("http://127.0.0.1:{port}/callback"), rx))
    }

    fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

// MARK: - 同步服务

pub struct SyncService {
    settings: Rc<Settings>,
    client: RefCell<Option<Arc<SyncClient>>>,
    server: RefCell<String>,
    pub backend: Cell<Option<Backend>>,
    pub status: RefCell<Option<SyncStatus>>,
    pub remote_vault: RefCell<Option<bool>>,
    pub join_requests: RefCell<Vec<JoinRequest>>,
    pub devices: RefCell<Vec<DeviceInfo>>,
    pub join: RefCell<Option<JoinProgress>>,
    /// 本机刚批准的加入请求：提示用户到新设备上确认一致
    pub approved_joins: RefCell<BTreeSet<String>>,
    pub working: RefCell<Option<String>>,
    pub error: RefCell<Option<String>>,
    /// 刚创建保险库时的恢复短语，只显示一次
    pub phrase: RefCell<Option<String>>,
    pub last_report: RefCell<Option<SyncReport>>,
    pub account_devices: RefCell<Option<AccountDevices>>,
    /// 设置页打开时为真：此时才轮询加入请求、刷新设备列表
    pub settings_visible: Cell<bool>,
    asleep: Cell<bool>,
    timer: RefCell<Option<glib::SourceId>>,
    push_timer: RefCell<Option<glib::SourceId>>,
    pending: RefCell<Option<glib::SourceId>>,
    join_timer: RefCell<Option<glib::SourceId>>,
    loopback: RefCell<Option<Loopback>>,
    observers: Cell<bool>,
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
    /// 同步拉取到新内容后调用（重新加载模板）
    on_templates_changed: RefCell<Option<Rc<dyn Fn()>>>,
}

impl SyncService {
    pub fn new(settings: Rc<Settings>) -> Rc<SyncService> {
        Rc::new(SyncService {
            settings,
            client: RefCell::new(None),
            server: RefCell::new(String::new()),
            backend: Cell::new(None),
            status: RefCell::new(None),
            remote_vault: RefCell::new(None),
            join_requests: RefCell::new(Vec::new()),
            devices: RefCell::new(Vec::new()),
            join: RefCell::new(None),
            approved_joins: RefCell::new(BTreeSet::new()),
            working: RefCell::new(None),
            error: RefCell::new(None),
            phrase: RefCell::new(None),
            last_report: RefCell::new(None),
            account_devices: RefCell::new(None),
            settings_visible: Cell::new(false),
            asleep: Cell::new(false),
            timer: RefCell::new(None),
            push_timer: RefCell::new(None),
            pending: RefCell::new(None),
            join_timer: RefCell::new(None),
            loopback: RefCell::new(None),
            observers: Cell::new(false),
            listeners: RefCell::new(Vec::new()),
            on_templates_changed: RefCell::new(None),
        })
    }

    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    pub fn set_on_templates_changed(&self, f: impl Fn() + 'static) {
        *self.on_templates_changed.borrow_mut() = Some(Rc::new(f));
    }

    fn notify(&self) {
        let list: Vec<_> = self.listeners.borrow().clone();
        for f in list {
            f();
        }
    }

    fn templates_changed(&self) {
        if let Some(f) = self.on_templates_changed.borrow().clone() {
            f();
        }
    }

    pub fn signed_in(&self) -> bool {
        self.status.borrow().as_ref().is_some_and(|s| s.signed_in)
    }

    pub fn enrolled(&self) -> bool {
        self.status.borrow().as_ref().is_some_and(|s| s.has_vault && s.enrolled)
    }

    /// 托盘提示：有设备等待加入
    pub fn pending_join_count(&self) -> usize {
        let pending = self.status.borrow().as_ref().map(|s| s.pending_joins as usize).unwrap_or(0);
        let listed = self.join_requests.borrow().iter().filter(|r| r.status != JoinRequestStatus::HandledElsewhere).count();
        pending.max(listed)
    }

    /// 启动或切换服务端时调用
    pub fn start(self: &Rc<Self>) {
        let url = self.settings.sync_server();
        if *self.server.borrow() == url && self.client.borrow().is_some() {
            return;
        }
        *self.server.borrow_mut() = url.clone();
        let host = url.split("://").nth(1).unwrap_or(&url).split(['/', ':']).next().unwrap_or("default").to_string();
        let data_dir = paths::data_dir().join("sync").join(&host);
        let _ = std::fs::create_dir_all(&data_dir);
        let store = LinuxSecretStore::open(&host, &data_dir);
        self.backend.set(Some(store.backend()));
        let client = SyncClient::new(url, data_dir.to_string_lossy().into(), paths::templates_dir().to_string_lossy().into(), store, device_label());
        *self.status.borrow_mut() = Some(client.status());
        *self.client.borrow_mut() = Some(client);
        *self.remote_vault.borrow_mut() = None;
        self.join_requests.borrow_mut().clear();
        self.devices.borrow_mut().clear();
        *self.join.borrow_mut() = None;
        *self.account_devices.borrow_mut() = None;
        if let Some(id) = self.timer.borrow_mut().take() {
            id.remove();
        }
        // 每分钟让核心判断一次是否到期；没到期时不发任何请求（间隔由服务端下发的同步策略决定）
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_seconds_local(60, move || match weak.upgrade() {
            Some(s) => {
                s.trigger(SyncTrigger::Timer);
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        *self.timer.borrow_mut() = Some(id);
        self.install_observers();
        self.refresh();
        self.trigger(SyncTrigger::Launch);
        self.notify();
    }

    /// 锁屏 / 休眠时暂停，解锁 / 唤醒后补一次检查
    fn install_observers(self: &Rc<Self>) {
        if self.observers.replace(true) {
            return;
        }
        let set_asleep = |weak: std::rc::Weak<SyncService>, asleep: bool| {
            if let Some(s) = weak.upgrade() {
                if s.asleep.get() && !asleep {
                    s.asleep.set(false);
                    s.trigger(SyncTrigger::Wake);
                } else {
                    s.asleep.set(asleep);
                }
            }
        };
        if let Ok(system) = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) {
            let weak = Rc::downgrade(self);
            #[allow(deprecated)]
            system.signal_subscribe(
                Some("org.freedesktop.login1"),
                Some("org.freedesktop.login1.Manager"),
                Some("PrepareForSleep"),
                Some("/org/freedesktop/login1"),
                None,
                gio::DBusSignalFlags::NONE,
                move |_, _, _, _, _, params| {
                    if let Some((sleeping,)) = params.get::<(bool,)>() {
                        set_asleep(weak.clone(), sleeping);
                    }
                },
            );
            std::mem::forget(system);
        }
        if let Ok(session) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
            for iface in ["org.freedesktop.ScreenSaver", "org.gnome.ScreenSaver"] {
                let weak = Rc::downgrade(self);
                #[allow(deprecated)]
                session.signal_subscribe(None, Some(iface), Some("ActiveChanged"), None, None, gio::DBusSignalFlags::NONE, move |_, _, _, _, _, params| {
                    if let Some((active,)) = params.get::<(bool,)>() {
                        set_asleep(weak.clone(), active);
                    }
                });
            }
            std::mem::forget(session);
        }
    }

    /// 在后台线程执行一个核心调用。`label` 显示为进行中的操作。
    fn run<T: Send + 'static>(
        self: &Rc<Self>,
        label: Option<String>,
        work: impl FnOnce(&SyncClient) -> Result<T, SyncError> + Send + 'static,
        done: impl FnOnce(&Rc<SyncService>, T) + 'static,
    ) {
        let Some(client) = self.client.borrow().clone() else { return };
        let labeled = label.is_some();
        if let Some(l) = label {
            *self.working.borrow_mut() = Some(l);
        }
        *self.error.borrow_mut() = None;
        self.notify();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let joined = gio::spawn_blocking(move || {
                let r = work(&client);
                let s = client.status();
                (r, s)
            })
            .await;
            let Ok((result, status)) = joined else { return };
            if labeled {
                *this.working.borrow_mut() = None;
            }
            *this.status.borrow_mut() = Some(status);
            match result {
                Ok(v) => done(&this, v),
                Err(e) => {
                    if labeled {
                        *this.error.borrow_mut() = Some(describe(&e));
                    }
                }
            }
            this.notify();
        });
    }

    /// 按触发原因交给核心判断是否同步（docs/sync-design.md v0.4）；没到期时核心不发请求。
    pub fn trigger(self: &Rc<Self>, trigger: SyncTrigger) {
        let limited = self.status.borrow().as_ref().is_some_and(|s| s.device_limit.is_some());
        if !self.signed_in() || !(self.enrolled() || limited) {
            return;
        }
        if self.asleep.get() && trigger == SyncTrigger::Timer {
            return;
        }
        self.run(
            None,
            move |c| c.maybe_sync(trigger),
            |s, report| {
                if let Some(r) = report {
                    if r.pulled > 0 || r.replaced > 0 {
                        s.templates_changed();
                    }
                    *s.last_report.borrow_mut() = Some(r);
                }
                s.schedule_push();
                let pending = s.status.borrow().as_ref().map(|x| x.pending_joins).unwrap_or(0);
                if pending > 0 || s.settings_visible.get() {
                    s.poll_join_requests();
                }
            },
        );
    }

    /// 本地修改被节流时，在核心给出的时间点再试一次自动上传
    fn schedule_push(self: &Rc<Self>) {
        if let Some(id) = self.push_timer.borrow_mut().take() {
            id.remove();
        }
        let Some(at) = self.status.borrow().as_ref().and_then(|s| s.next_push_at) else { return };
        let delay = (at - crate::util::now_secs() + 1).max(1) as u64;
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_seconds_local_once(delay as u32, move || {
            if let Some(s) = weak.upgrade() {
                s.push_timer.borrow_mut().take();
                s.trigger(SyncTrigger::LocalChange);
            }
        });
        *self.push_timer.borrow_mut() = Some(id);
    }

    /// 唤出面板时调用：距上次检查足够久时在后台检查一次，面板不等待
    pub fn panel_opened(self: &Rc<Self>) {
        self.trigger(SyncTrigger::PanelOpened);
    }

    /// 读取服务端信息：是否有保险库、在册设备；设置页打开时再读账号设备与加入请求。
    pub fn refresh(self: &Rc<Self>) {
        let Some(client) = self.client.borrow().clone() else { return };
        let status = client.status();
        let signed_in = status.signed_in;
        let has_vault = status.has_vault;
        let limited = status.device_limit.is_some();
        *self.status.borrow_mut() = Some(status);
        if !signed_in {
            self.notify();
            return;
        }
        if !has_vault {
            self.run(None, |c| c.remote_vault_exists(), |s, v| *s.remote_vault.borrow_mut() = Some(v));
        } else {
            self.run(None, |c| c.devices(), |s, d| *s.devices.borrow_mut() = d);
        }
        if self.settings_visible.get() || limited {
            self.load_account_devices();
        }
    }

    pub fn load_account_devices(self: &Rc<Self>) {
        self.run(None, |c| c.account_devices(), |s, d| *s.account_devices.borrow_mut() = Some(d));
    }

    /// 解除一台设备的登录（超出设备上限时，或旧设备已丢失）
    pub fn release(self: &Rc<Self>, device: AccountDevice) {
        let grant = device.grant_id.clone();
        let is_self = device.is_self;
        self.run(
            Some(l("正在解除…", "Releasing…")),
            move |c| c.release_device(grant),
            move |s, _| {
                s.refresh();
                if !is_self {
                    s.trigger(SyncTrigger::Manual);
                }
            },
        );
    }

    /// 模板目录变化后稍等片刻，合并连续的保存，再交给核心按上传间隔决定是否上传。
    pub fn schedule_sync(self: &Rc<Self>) {
        if !self.enrolled() {
            return;
        }
        if let Some(id) = self.pending.borrow_mut().take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_seconds_local_once(8, move || {
            if let Some(s) = weak.upgrade() {
                s.pending.borrow_mut().take();
                s.trigger(SyncTrigger::LocalChange);
            }
        });
        *self.pending.borrow_mut() = Some(id);
    }

    pub fn sync_now(self: &Rc<Self>) {
        self.run(
            Some(l("正在同步…", "Syncing…")),
            |c| c.sync_now(),
            |s, report| {
                if report.pulled > 0 || report.replaced > 0 {
                    s.templates_changed();
                }
                *s.last_report.borrow_mut() = Some(report);
                s.refresh();
            },
        );
    }

    // MARK: 登录

    pub fn sign_in(self: &Rc<Self>) {
        let Some(client) = self.client.borrow().clone() else { return };
        *self.error.borrow_mut() = None;
        let (lb, redirect, rx) = match Loopback::start() {
            Ok(x) => x,
            Err(_) => {
                *self.error.borrow_mut() = Some(l("无法监听本机端口", "Couldn't open a local port"));
                self.notify();
                return;
            }
        };
        if let Some(old) = self.loopback.borrow_mut().replace(lb) {
            old.stop();
        }
        *self.working.borrow_mut() = Some(l("请在浏览器中完成登录…", "Finish signing in in your browser…"));
        self.notify();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let url = gio::spawn_blocking(move || client.begin_sign_in(redirect)).await;
            match url {
                Ok(Ok(authorize)) => {
                    #[cfg(all(feature = "debug-hooks", debug_assertions))]
                    if let Some(cookie) = crate::debug::sync_cookie() {
                        // 仅调试构建：对本地测试服务端，用预置的网页会话代替浏览器完成授权
                        gio::spawn_blocking(move || crate::debug::authorize_with_cookie(&authorize, &cookie));
                    } else {
                        gtk::UriLauncher::new(&authorize).launch(None::<&gtk::Window>, gio::Cancellable::NONE, |_| {});
                    }
                    #[cfg(not(all(feature = "debug-hooks", debug_assertions)))]
                    gtk::UriLauncher::new(&authorize).launch(None::<&gtk::Window>, gio::Cancellable::NONE, |_| {});
                }
                Ok(Err(e)) => {
                    this.cancel_sign_in();
                    *this.error.borrow_mut() = Some(describe(&e));
                    this.notify();
                    return;
                }
                Err(_) => return,
            }
            match rx.recv().await {
                Ok(LoopbackResult::Ok { code, state }) => {
                    this.loopback.borrow_mut().take();
                    this.run(
                        Some(l("正在登录…", "Signing in…")),
                        move |c| c.finish_sign_in(code, state),
                        |s, _| {
                            s.refresh();
                            s.trigger(SyncTrigger::Launch);
                            crate::app::app().present_settings_if_open();
                        },
                    );
                }
                Ok(LoopbackResult::Denied) => {
                    this.loopback.borrow_mut().take();
                    *this.working.borrow_mut() = None;
                    *this.error.borrow_mut() = Some(l("已取消登录", "Sign-in was cancelled"));
                    this.notify();
                }
                Err(_) => {}
            }
        });
    }

    pub fn cancel_sign_in(&self) {
        if let Some(lb) = self.loopback.borrow_mut().take() {
            lb.stop();
        }
        *self.working.borrow_mut() = None;
        self.notify();
    }

    pub fn sign_out(self: &Rc<Self>) {
        self.run(
            Some(l("正在退出…", "Signing out…")),
            |c| c.sign_out(),
            |s, _| {
                *s.remote_vault.borrow_mut() = None;
                s.devices.borrow_mut().clear();
                s.join_requests.borrow_mut().clear();
                *s.account_devices.borrow_mut() = None;
            },
        );
    }

    pub fn reset_device(self: &Rc<Self>) {
        self.run(
            Some(l("正在重置…", "Resetting…")),
            |c| c.reset_device(),
            |s, _| {
                *s.remote_vault.borrow_mut() = None;
                s.devices.borrow_mut().clear();
                s.join_requests.borrow_mut().clear();
                *s.join.borrow_mut() = None;
            },
        );
    }

    // MARK: 保险库

    pub fn create_vault(self: &Rc<Self>) {
        let lang = if crate::i18n::ui_locale() == Locale::Zh { PhraseLanguage::ChineseSimplified } else { PhraseLanguage::English };
        self.run(
            Some(l("正在创建保险库…", "Creating vault…")),
            move |c| c.create_vault(lang),
            |s, phrase| {
                *s.phrase.borrow_mut() = Some(phrase);
                s.sync_now();
            },
        );
    }

    pub fn recover(self: &Rc<Self>, phrase: String) {
        self.run(Some(l("正在恢复…", "Recovering…")), move |c| c.recover(phrase), |s, _| s.sync_now());
    }

    // MARK: 新设备加入

    pub fn start_join(self: &Rc<Self>) {
        self.run(
            Some(l("正在发起加入请求…", "Requesting to join…")),
            |c| c.start_join(),
            |s, progress| {
                *s.join.borrow_mut() = Some(progress);
                if let Some(id) = s.join_timer.borrow_mut().take() {
                    id.remove();
                }
                let weak = Rc::downgrade(s);
                let id = glib::timeout_add_seconds_local(2, move || match weak.upgrade() {
                    Some(s) => {
                        s.poll_join();
                        glib::ControlFlow::Continue
                    }
                    None => glib::ControlFlow::Break,
                });
                *s.join_timer.borrow_mut() = Some(id);
            },
        );
    }

    fn stop_join_timer(&self) {
        if let Some(id) = self.join_timer.borrow_mut().take() {
            id.remove();
        }
    }

    fn poll_join(self: &Rc<Self>) {
        self.run(
            None,
            |c| c.poll_join(),
            |s, progress| s.join_progressed(progress),
        );
    }

    fn join_progressed(self: &Rc<Self>, progress: JoinProgress) {
        let stage = progress.stage;
        *self.join.borrow_mut() = Some(progress);
        match stage {
            JoinStage::Completed => {
                self.stop_join_timer();
                *self.join.borrow_mut() = None;
                self.sync_now();
            }
            JoinStage::Expired => self.stop_join_timer(),
            _ => {}
        }
    }

    /// 新设备：用户确认两台设备上的校验码一致。另一台设备已选好时立即加入，否则继续等待。
    pub fn confirm_join(self: &Rc<Self>) {
        self.run(Some(l("正在确认…", "Confirming…")), |c| c.confirm_join(), |s, progress| s.join_progressed(progress));
    }

    pub fn cancel_join(self: &Rc<Self>) {
        self.stop_join_timer();
        *self.join.borrow_mut() = None;
        self.run(
            None,
            |c| {
                c.cancel_join();
                Ok(())
            },
            |_, _| {},
        );
    }

    // MARK: 已登录设备批准加入

    pub fn poll_join_requests(self: &Rc<Self>) {
        if !self.enrolled() {
            return;
        }
        self.run(None, |c| c.list_join_requests(), |s, r| *s.join_requests.borrow_mut() = r);
    }

    /// 已登录设备：用户在三个候选码中选出新设备上显示的那个。选错时核心取消这次请求。
    pub fn approve(self: &Rc<Self>, join_id: String, code: String) {
        let id = join_id.clone();
        self.run(
            Some(l("正在批准…", "Approving…")),
            move |c| c.approve_join(join_id, code),
            move |s, _| {
                s.approved_joins.borrow_mut().insert(id);
                s.refresh();
            },
        );
        self.poll_join_requests();
    }

    pub fn reject(self: &Rc<Self>, join_id: String) {
        self.run(None, move |c| c.reject_join(join_id), |s, _| s.refresh());
    }

    pub fn dismiss_approved(&self, join_id: &str) {
        self.approved_joins.borrow_mut().remove(join_id);
        self.notify();
    }

    /// 历史版本目录：同步时被较新版本覆盖的模板备份在这里（保留 30 天）。目录不存在时创建。
    pub fn history_dir(&self) -> Option<std::path::PathBuf> {
        let dir = std::path::PathBuf::from(self.client.borrow().as_ref()?.history_dir());
        let _ = std::fs::create_dir_all(&dir);
        Some(dir)
    }

    // MARK: 设备管理

    /// 撤销设备。需要恢复短语时调用 `needs_phrase`。
    pub fn revoke(self: &Rc<Self>, device_id: String, phrase: Option<String>, needs_phrase: impl FnOnce() + 'static) {
        let Some(client) = self.client.borrow().clone() else { return };
        *self.working.borrow_mut() = Some(l("正在撤销并轮换密钥…", "Removing and rotating keys…"));
        *self.error.borrow_mut() = None;
        self.notify();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok((result, status)) = gio::spawn_blocking(move || {
                let r = client.revoke_device(device_id, phrase);
                (r, client.status())
            })
            .await
            else {
                return;
            };
            *this.working.borrow_mut() = None;
            *this.status.borrow_mut() = Some(status);
            match result {
                Ok(_) => this.refresh(),
                Err(SyncError::RecoveryRequired { .. }) => needs_phrase(),
                Err(e) => *this.error.borrow_mut() = Some(describe(&e)),
            }
            this.notify();
        });
    }

    pub fn rotate_keys(self: &Rc<Self>) {
        self.run(Some(l("正在轮换密钥…", "Rotating keys…")), |c| c.rotate_keys(), |s, _| s.refresh());
    }

    /// 重新验证：服务端恢复正常后解除停止写入
    pub fn reverify(self: &Rc<Self>) {
        self.run(Some(l("正在重新验证…", "Re-verifying…")), |c| c.reverify(), |s, _| s.refresh());
    }

    pub fn dismiss_alerts(self: &Rc<Self>) {
        self.run(
            None,
            |c| {
                c.dismiss_alerts();
                Ok(())
            },
            |_, _| {},
        );
    }

    pub fn take_phrase(&self) -> Option<String> {
        self.phrase.borrow().clone()
    }

    pub fn clear_phrase(&self) {
        *self.phrase.borrow_mut() = None;
        self.notify();
    }
}
