//! 托盘（StatusNotifierItem / AppIndicator，KDE 原生支持，GNOME 需要 AppIndicator 扩展）。
//! 没有托盘宿主时应用照常运行：从应用菜单再次打开 Qantrip 即可进入模板库与设置。
//! 菜单与 macOS 菜单栏菜单一致。托盘在独立线程中运行，菜单操作经通道交给主线程。

use std::rc::Rc;

use gtk::glib;
use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};

use crate::app::App;
use crate::hotkey::{Press, State as HotKeyState};
use crate::i18n::l;
use crate::settings::ExcludedApp;
use crate::t;

#[derive(Clone, Debug)]
enum Action {
    OpenPanel,
    Library,
    Settings(Option<&'static str>),
    Update,
    ToggleExclusion(String, String),
    RetryHotkey,
    Onboarding,
    Quit,
}

/// 菜单显示所需的状态快照（托盘线程中使用）
#[derive(Clone, Default, PartialEq)]
struct Snapshot {
    shortcut: String,
    hotkey_problem: Option<String>,
    update: Option<String>,
    pending_joins: bool,
    /// (应用 ID, 名称, 是否已排除)
    current_app: Option<(String, String, bool)>,
    autopaste_off: bool,
    problems: usize,
    zh: bool,
}

struct QantripTray {
    snap: Snapshot,
    tx: async_channel::Sender<Action>,
    icon_path: String,
}

impl QantripTray {
    fn l(&self, zh: &str, en: &str) -> String {
        if self.snap.zh { zh.to_string() } else { en.to_string() }
    }

    fn item(&self, label: String, action: Action) -> MenuItem<Self> {
        let tx = self.tx.clone();
        MenuItem::Standard(StandardItem {
            label,
            activate: Box::new(move |_| {
                let _ = tx.try_send(action.clone());
            }),
            ..Default::default()
        })
    }
}

impl ksni::Tray for QantripTray {
    fn id(&self) -> String {
        "ai.nekro.qantrip".into()
    }

    fn title(&self) -> String {
        "Nekro Qantrip".into()
    }

    fn icon_name(&self) -> String {
        "qantrip-tray-symbolic".into()
    }

    fn icon_theme_path(&self) -> String {
        self.icon_path.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "Nekro Qantrip".into(), description: self.snap.shortcut.clone(), ..Default::default() }
    }

    /// 左键单击打开面板
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.try_send(Action::OpenPanel);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let s = &self.snap;
        let mut m = Vec::new();
        if let Some(v) = &s.update {
            m.push(self.item(if s.zh { format!("有新版本 {v}，前往下载…") } else { format!("Update Available: {v}…") }, Action::Update));
            m.push(MenuItem::Separator);
        }
        if s.pending_joins {
            m.push(self.item(self.l("有设备请求加入同步…", "A device wants to join sync…"), Action::Settings(Some("sync"))));
            m.push(MenuItem::Separator);
        }
        let open = if s.zh { format!("打开面板（{}）", s.shortcut) } else { format!("Open Panel ({})", s.shortcut) };
        m.push(self.item(open, Action::OpenPanel));
        // 打开托盘菜单时前台仍是用户刚才使用的应用：一键把它加入或移出排除列表
        if let Some((id, name, excluded)) = &s.current_app {
            let label = match (excluded, s.zh) {
                (true, true) => format!("在「{name}」中恢复快捷键"),
                (true, false) => format!("Re-enable Shortcut in “{name}”"),
                (false, true) => format!("在「{name}」中停用快捷键"),
                (false, false) => format!("Disable Shortcut in “{name}”"),
            };
            m.push(self.item(label, Action::ToggleExclusion(id.clone(), name.clone())));
        }
        m.push(self.item(self.l("模板库…", "Template Library…"), Action::Library));
        m.push(self.item(self.l("设置…", "Settings…"), Action::Settings(None)));
        m.push(MenuItem::Separator);
        if let Some(p) = &s.hotkey_problem {
            m.push(self.item(p.clone(), Action::RetryHotkey));
        }
        if s.autopaste_off {
            m.push(self.item(self.l("⚠︎ 未授权自动粘贴（只复制）", "⚠︎ Auto-paste not allowed (copy only)"), Action::Settings(Some("insert"))));
        }
        if s.problems > 0 {
            let n = s.problems;
            m.push(self.item(
                if s.zh {
                    format!("⚠︎ {n} 个模板加载问题")
                } else if n == 1 {
                    "⚠︎ 1 template problem".into()
                } else {
                    format!("⚠︎ {n} template problems")
                },
                Action::Settings(Some("templates")),
            ));
        }
        m.push(self.item(self.l("使用引导…", "Welcome Guide…"), Action::Onboarding));
        m.push(MenuItem::Separator);
        m.push(self.item(self.l("退出 Qantrip", "Quit Qantrip"), Action::Quit));
        m
    }
}

pub struct Tray {
    handle: ksni::blocking::Handle<QantripTray>,
    app: std::rc::Weak<App>,
    last: std::cell::RefCell<Snapshot>,
}

fn snapshot(app: &App) -> Snapshot {
    let shortcut = app.hotkey.display();
    let hotkey_problem = match app.hotkey.state() {
        HotKeyState::Failed { .. } => Some(t!("⚠︎ 快捷键 {shortcut} 注册失败，点此重试", "⚠︎ Shortcut {shortcut} could not be registered. Click to retry")),
        HotKeyState::Manual => Some(l("⚠︎ 需要在系统设置中绑定快捷键…", "⚠︎ Bind the shortcut in system settings…")),
        _ => None,
    };
    let current_app = app.foreground.last_external().map(|a| {
        let excluded = app.settings.is_excluded(Some(&a.id));
        (a.id, a.name, excluded)
    });
    Snapshot {
        shortcut,
        hotkey_problem,
        update: app.updates.pending().map(|r| r.version),
        pending_joins: app.sync.pending_join_count() > 0,
        current_app,
        autopaste_off: !app.settings.copy_only() && app.autopaste.method() == crate::autopaste::Method::None,
        problems: app.engine.problems().len(),
        zh: crate::i18n::is_zh(),
    }
}

impl Tray {
    /// 启动托盘；系统没有托盘宿主时返回 None（应用照常运行）
    pub fn start(app: Rc<App>) -> Option<Tray> {
        let (tx, rx) = async_channel::unbounded::<Action>();
        let icon_path = crate::paths::resource("icons").map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let first = snapshot(&app);
        let tray = QantripTray { snap: first.clone(), tx, icon_path };
        let handle = match tray.spawn() {
            Ok(h) => h,
            Err(e) => {
                crate::log::event("tray_unavailable", serde_json::json!({"error": e.to_string()}));
                return None;
            }
        };
        let weak = Rc::downgrade(&app);
        glib::spawn_future_local(async move {
            while let Ok(action) = rx.recv().await {
                let Some(app) = weak.upgrade() else { return };
                match action {
                    Action::OpenPanel => {
                        if !app.panel().is_visible() {
                            app.panel().show(&Press { source: "tray", timestamp: 0, token: None });
                        }
                    }
                    Action::Library => app.show_library(None),
                    Action::Settings(page) => app.show_settings(page),
                    Action::Update => app.updates.open_download(),
                    Action::ToggleExclusion(id, name) => {
                        let mut list = app.settings.excluded_apps();
                        if app.settings.is_excluded(Some(&id)) {
                            list.retain(|a| !a.id.eq_ignore_ascii_case(&id));
                        } else {
                            list.push(ExcludedApp { id, name });
                        }
                        app.settings.set_excluded_apps(&list);
                    }
                    Action::RetryHotkey => {
                        if app.hotkey.mode() == crate::hotkey::Mode::Manual {
                            app.show_settings(Some("general"));
                        } else {
                            app.hotkey.retry();
                        }
                    }
                    Action::Onboarding => app.show_onboarding(),
                    Action::Quit => app.quit(),
                }
            }
        });
        let t = Tray { handle, app: Rc::downgrade(&app), last: std::cell::RefCell::new(first) };
        let weak = Rc::downgrade(&app);
        app.connect_changed(move || {
            if let Some(app) = weak.upgrade() {
                app.refresh_tray();
            }
        });
        Some(t)
    }

    pub fn refresh(&self) {
        if let Some(app) = self.app.upgrade() {
            let snap = snapshot(&app);
            if *self.last.borrow() == snap {
                return;
            }
            *self.last.borrow_mut() = snap.clone();
            self.handle.update(move |t| t.snap = snap);
        }
    }

    pub fn stop(self) {
        let _ = self.handle.shutdown();
    }
}
