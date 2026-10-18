//! 新版本提示（不自动更新）：先请求 qantrip.nekro.ai/api/v1/update（服务端缓存 GitHub 最新正式版本，
//! 国内网络也能访问），失败时直接查 GitHub。启动约 10 秒后检查一次，之后每小时判断一次，
//! 自动检查之间至少间隔 12 小时，失败时静默。点击下载直接下载与当前安装方式对应的安装包。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::glib;

use crate::i18n::l;
use crate::settings::Settings;
use crate::t;

const SOURCES: [&str; 2] = ["https://qantrip.nekro.ai/api/v1/update", "https://api.github.com/repos/NekroAI/qantrip/releases/latest"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Failed,
}

/// 安装方式：决定下载哪个文件
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Package {
    AppImage,
    Deb,
    Rpm,
}

pub fn package_kind() -> Package {
    if std::env::var_os("APPIMAGE").is_some() {
        return Package::AppImage;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    if exe.starts_with("/usr") {
        if std::path::Path::new("/var/lib/dpkg/info/qantrip.list").exists() {
            return Package::Deb;
        }
        if std::path::Path::new("/var/lib/rpm").exists() || std::path::Path::new("/usr/lib/sysimage/rpm").exists() {
            return Package::Rpm;
        }
        return Package::Deb;
    }
    Package::AppImage
}

pub fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" }
}

/// 发布文件名（与发布工作流一致）
pub fn asset_name(version: Option<&str>) -> String {
    let ext = match package_kind() {
        Package::AppImage => "AppImage",
        Package::Deb => "deb",
        Package::Rpm => "rpm",
    };
    match version {
        Some(v) => format!("Qantrip-{v}-Linux-{}.{ext}", arch()),
        None => format!("Qantrip-Linux-{}.{ext}", arch()),
    }
}

/// 按数字逐段比较版本号（如 0.3.10 > 0.3.9）
pub fn is_newer(a: &str, b: &str) -> bool {
    let pa: Vec<u64> = a.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    let pb: Vec<u64> = b.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (pa.get(i).copied().unwrap_or(0), pb.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// 解析官网接口（version）或 GitHub 接口（tag_name）的返回
fn parse(body: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    if v.get("draft").and_then(|x| x.as_bool()) == Some(true) || v.get("prerelease").and_then(|x| x.as_bool()) == Some(true) {
        return None;
    }
    let tag = v.get("version").or_else(|| v.get("tag_name"))?.as_str()?;
    Some(Release { version: tag.trim_start_matches('v').to_string() })
}

fn fetch() -> Option<Release> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
    for url in SOURCES {
        let r = agent.get(url).header("Accept", "application/json").header("User-Agent", &format!("Qantrip/{} (Linux)", crate::VERSION)).call();
        if let Ok(mut resp) = r
            && resp.status() == 200
            && let Ok(body) = resp.body_mut().read_to_string()
            && let Some(rel) = parse(&body)
        {
            return Some(rel);
        }
    }
    None
}

pub struct UpdateChecker {
    settings: Rc<Settings>,
    pub available: RefCell<Option<Release>>,
    pub state: Cell<State>,
}

impl UpdateChecker {
    pub fn new(settings: Rc<Settings>) -> Rc<UpdateChecker> {
        Rc::new(UpdateChecker { settings, available: RefCell::new(None), state: Cell::new(State::Idle) })
    }

    fn notify(&self) {
        if let Some(app) = crate::app::try_app() {
            app.changed();
        }
    }

    pub fn auto_check(&self) -> bool {
        self.settings.auto_check_updates()
    }

    pub fn set_auto_check(&self, on: bool) {
        self.settings.set_bool("autoCheckUpdates", on);
    }

    pub fn skipped(&self) -> Option<String> {
        self.settings.string("skippedUpdateVersion")
    }

    pub fn skip(&self, version: &str) {
        self.settings.set_string("skippedUpdateVersion", Some(version));
        self.notify();
    }

    /// 需要提醒的新版本（未被跳过）
    pub fn pending(&self) -> Option<Release> {
        let a = self.available.borrow().clone()?;
        if Some(&a.version) == self.skipped().as_ref() { None } else { Some(a) }
    }

    pub fn start(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local_once(10, move || {
            if let Some(u) = weak.upgrade() {
                u.automatic_check();
            }
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(3600, move || match weak.upgrade() {
            Some(u) => {
                u.automatic_check();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
    }

    fn automatic_check(self: &Rc<Self>) {
        if !self.auto_check() {
            return;
        }
        let last = self.settings.f64("lastUpdateCheck", 0.0);
        if (crate::util::now_secs() as f64) - last < 12.0 * 3600.0 {
            return;
        }
        self.check(false);
    }

    /// 检查更新。手动检查会把结果显示在设置 › 关于中。
    pub fn check(self: &Rc<Self>, manual: bool) {
        if self.state.get() == State::Checking {
            return;
        }
        self.state.set(State::Checking);
        self.notify();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let r = gio::spawn_blocking(fetch).await.ok().flatten();
            match r {
                Some(rel) => this.finish(rel, manual),
                None => {
                    this.state.set(State::Failed);
                    crate::log::event("update_check_failed", serde_json::json!({"manual": manual}));
                }
            }
            this.notify();
        });
    }

    fn finish(&self, latest: Release, manual: bool) {
        self.settings.set_f64("lastUpdateCheck", crate::util::now_secs() as f64);
        crate::log::event("update_check", serde_json::json!({"latest": latest.version, "current": crate::VERSION, "manual": manual}));
        if is_newer(&latest.version, crate::VERSION) {
            let is_new = self.available.borrow().as_ref() != Some(&latest);
            *self.available.borrow_mut() = Some(latest.clone());
            self.state.set(State::Idle);
            if is_new && !manual && self.pending().is_some() {
                self.notify_once(&latest);
            }
        } else {
            *self.available.borrow_mut() = None;
            self.state.set(State::UpToDate);
        }
    }

    /// 每个版本只提醒一次
    fn notify_once(&self, r: &Release) {
        if self.settings.string("notifiedUpdateVersion").as_deref() == Some(r.version.as_str()) {
            return;
        }
        self.settings.set_string("notifiedUpdateVersion", Some(&r.version));
        let v = &r.version;
        crate::notify::notice(
            &t!("Qantrip 有新版本 {v}", "Qantrip {v} is available"),
            &l("在托盘菜单或设置 › 关于中下载。", "Download it from the tray menu or Settings › About."),
        );
    }

    /// 直接下载与当前安装方式对应的安装包（浏览器开始下载），不打开发布页
    pub fn download_url(&self) -> String {
        match self.available.borrow().as_ref() {
            Some(r) => format!("https://github.com/NekroAI/qantrip/releases/download/v{}/{}", r.version, asset_name(Some(&r.version))),
            None => format!("https://github.com/NekroAI/qantrip/releases/latest/download/{}", asset_name(None)),
        }
    }

    pub fn open_download(&self) {
        gtk::UriLauncher::new(&self.download_url()).launch(None::<&gtk::Window>, gio::Cancellable::NONE, |_| {});
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.3.10", "0.3.9"));
        assert!(!is_newer("0.3.6", "0.3.6"));
        assert!(is_newer("1.0", "0.9.9"));
    }

    #[test]
    fn parses_both_sources() {
        assert_eq!(parse(r#"{"version":"0.4.0","tag":"v0.4.0","url":"x"}"#), Some(Release { version: "0.4.0".into() }));
        assert_eq!(parse(r#"{"tag_name":"v0.4.1","html_url":"x","draft":false,"prerelease":false}"#), Some(Release { version: "0.4.1".into() }));
        assert_eq!(parse(r#"{"tag_name":"v0.5.0","prerelease":true}"#), None);
    }
}
