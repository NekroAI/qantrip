//! 应用：单实例（GApplication，D-Bus 名称 ai.nekro.qantrip）、命令行、各组件的组装与协作。
//! `qantrip toggle` 等命令由新进程经 D-Bus 转交给已运行的实例后立即退出。

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;
use qantrip_core::Engine;

use crate::autopaste::AutoPaste;
use crate::foreground::{AppRef, Foreground, Source};
use crate::hotkey::{self, HotKey, Press};
use crate::i18n::{self, l};
use crate::ime::Ime;
use crate::insert::Inserter;
use crate::library::LibraryWindow;
use crate::panel::Panel;
use crate::session::{Desktop, Session};
use crate::settings::Settings;
use crate::sync::SyncService;
use crate::update::UpdateChecker;
use crate::x11::{X11, XEvent};
use crate::{APP_ID, paths};

pub struct App {
    pub gapp: adw::Application,
    pub settings: Rc<Settings>,
    pub engine: Arc<Engine>,
    pub session: Session,
    pub x11: Option<Arc<X11>>,
    pub foreground: Rc<Foreground>,
    pub hotkey: Rc<HotKey>,
    pub autopaste: Rc<AutoPaste>,
    pub inserter: Rc<Inserter>,
    pub ime: Rc<Ime>,
    pub sync: Rc<SyncService>,
    pub updates: Rc<UpdateChecker>,
    panel: OnceCell<Rc<Panel>>,
    library: RefCell<Option<Rc<LibraryWindow>>>,
    onboarding: RefCell<Option<Rc<crate::onboarding::Onboarding>>>,
    tray: RefCell<Option<crate::tray::Tray>>,
    watcher: RefCell<Option<gio::FileMonitor>>,
    reload_source: RefCell<Option<glib::SourceId>>,
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
    _hold: RefCell<Option<gio::ApplicationHoldGuard>>,
}

thread_local! {
    static APP: OnceCell<Rc<App>> = const { OnceCell::new() };
}

/// 全局应用对象（仅主线程）
pub fn app() -> Rc<App> {
    APP.with(|a| a.get().cloned()).expect("应用尚未初始化")
}

pub fn try_app() -> Option<Rc<App>> {
    APP.with(|a| a.get().cloned())
}

pub fn run() -> glib::ExitCode {
    let gapp = adw::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE).build();
    gapp.connect_startup(|gapp| {
        let app = App::new(gapp.clone());
        APP.with(|a| {
            let _ = a.set(app.clone());
        });
        app.start();
    });
    gapp.connect_command_line(|_, cmd| {
        let args: Vec<String> = cmd.arguments().iter().skip(1).map(|a| a.to_string_lossy().to_string()).collect();
        let code = app().handle_command_line(&args, cmd);
        glib::ExitCode::from(code)
    });
    gapp.connect_activate(|_| app().show_settings(None));
    gapp.connect_shutdown(|_| {
        if let Some(a) = try_app() {
            a.shutdown();
        }
    });
    gapp.run()
}

impl App {
    fn new(gapp: adw::Application) -> Rc<App> {
        let settings = Settings::load();
        i18n::set_ui_locale(settings.ui_locale());
        crate::theme::install();
        let session = Session::detect();
        let engine = Engine::new(Some(paths::templates_dir().to_string_lossy().into()), Some(paths::usage_file().to_string_lossy().into()));
        engine.set_locale(settings.ui_locale(), settings.template_locale());

        // X11 会话：快捷键、前台窗口、XTest 都经同一个连接
        let (xtx, xrx) = async_channel::unbounded::<XEvent>();
        let x11 = if session.is_x11() { X11::connect(xtx) } else { None };
        let foreground = Foreground::new();
        if x11.is_some() {
            foreground.set_source(Source::X11);
        }
        let mode = if session.is_x11() && x11.is_some() {
            hotkey::Mode::X11
        } else if session.is_wayland() && matches!(session.desktop, Desktop::Gnome | Desktop::Kde | Desktop::Other | Desktop::Wlroots) {
            hotkey::Mode::Portal
        } else {
            hotkey::Mode::Manual
        };
        let hotkey = HotKey::new(mode, settings.hotkey(), x11.clone());
        let autopaste = AutoPaste::new(x11.clone(), session.is_wayland(), session.desktop == Desktop::Wlroots, session.desktop == Desktop::Kde);
        let inserter = Inserter::new(settings.clone(), autopaste.clone(), x11.clone(), session.is_wayland());
        let ime = Ime::new(settings.clone());
        let sync = SyncService::new(settings.clone());
        let updates = UpdateChecker::new(settings.clone());

        let app = Rc::new(App {
            gapp,
            settings,
            engine,
            session,
            x11,
            foreground,
            hotkey,
            autopaste,
            inserter,
            ime,
            sync,
            updates,
            panel: OnceCell::new(),
            library: RefCell::new(None),
            onboarding: RefCell::new(None),
            tray: RefCell::new(None),
            watcher: RefCell::new(None),
            reload_source: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            _hold: RefCell::new(None),
        });

        // X11 事件
        let weak = Rc::downgrade(&app);
        glib::spawn_future_local(async move {
            while let Ok(ev) = xrx.recv().await {
                let Some(app) = weak.upgrade() else { return };
                match ev {
                    XEvent::HotKey(time) => app.hotkey.press(Press { source: "hotkey", timestamp: time, token: None }),
                    XEvent::Active(info) => app.foreground.update(info.map(|w| AppRef {
                        name: crate::foreground::pretty_name(&w.class, &w.title),
                        id: w.class,
                        pid: w.pid,
                        xid: Some(w.window),
                    })),
                }
            }
        });
        app
    }

    /// 托盘、设置页等需要随状态刷新的界面
    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    pub fn changed(&self) {
        let list: Vec<_> = self.listeners.borrow().clone();
        for f in list {
            f();
        }
    }

    fn start(self: &Rc<Self>) {
        *self._hold.borrow_mut() = Some(self.gapp.hold());
        let panel = Panel::new(self.clone());
        let _ = self.panel.set(panel);

        // 快捷键
        let weak = Rc::downgrade(self);
        self.hotkey.connect_press(move |p| {
            if let Some(app) = weak.upgrade() {
                app.on_hotkey(p);
            }
        });
        let weak = Rc::downgrade(self);
        self.hotkey.connect_state(move || {
            if let Some(app) = weak.upgrade() {
                app.changed();
            }
        });
        let weak = Rc::downgrade(self);
        self.autopaste.connect_changed(move || {
            if let Some(app) = weak.upgrade() {
                app.changed();
            }
        });

        // 门户：先登记应用 ID（非沙盒应用），再注册快捷键与检查自动粘贴
        let this = self.clone();
        glib::spawn_future_local(async move {
            if this.session.is_wayland()
                && let Ok(id) = ashpd::AppID::try_from(APP_ID)
            {
                let r = ashpd::register_host_app(id).await;
                crate::log::event("portal_register", serde_json::json!({"ok": r.is_ok()}));
            }
            this.hotkey.start();
            this.autopaste.probe();
        });

        // 前台应用（排除的应用 D35）
        if self.session.is_wayland()
            && self.session.desktop == Desktop::Kde
            && let Some(conn) = self.gapp.dbus_connection()
        {
            self.foreground.start_kwin(&conn);
        }
        let weak = Rc::downgrade(self);
        self.foreground.connect_changed(move || {
            if let Some(app) = weak.upgrade() {
                app.apply_exclusion();
                app.refresh_tray();
            }
        });

        // 设置变化
        let weak = Rc::downgrade(self);
        self.settings.connect_changed(move |key| {
            let Some(app) = weak.upgrade() else { return };
            match key {
                "interfaceLanguage" | "templateLanguage" => app.language_changed(),
                "excludedApps" => app.apply_exclusion(),
                "syncServer" => {}
                _ => {}
            }
            app.changed();
        });

        // 同步
        let weak = Rc::downgrade(self);
        self.sync.set_on_templates_changed(move || {
            if let Some(app) = weak.upgrade() {
                app.engine.reload();
                app.library_reload();
            }
        });
        let weak = Rc::downgrade(self);
        self.sync.connect_changed(move || {
            if let Some(app) = weak.upgrade() {
                app.changed();
            }
        });
        self.sync.start();
        self.updates.start();
        self.watch_templates();
        *self.tray.borrow_mut() = crate::tray::Tray::start(self.clone());
        crate::autostart::refresh_if_enabled();
        crate::autostart::integrate_appimage();

        crate::log::event(
            "launch",
            serde_json::json!({
                "version": crate::VERSION,
                "core_version": qantrip_core::core_version(),
                "session": self.session.label(),
                "hotkey_mode": format!("{:?}", self.hotkey.mode()),
                "autopaste": format!("{:?}", self.autopaste.method()),
                "templates": self.engine.templates().len(),
                "template_problems": self.engine.problems().len(),
                "tray": self.tray.borrow().is_some(),
            }),
        );
        #[cfg(all(feature = "debug-hooks", debug_assertions))]
        if crate::debug::enabled() {
            crate::log::event("debug_hooks", serde_json::json!({"enabled": true}));
        }
    }

    pub fn panel(&self) -> Rc<Panel> {
        self.panel.get().cloned().expect("面板尚未创建")
    }

    fn on_hotkey(self: &Rc<Self>, p: &Press) {
        if let Some(o) = self.onboarding.borrow().clone() {
            o.hotkey_pressed();
        }
        // Wayland 下无法让按键穿透：前台是排除的应用时不打开面板（X11 下快捷键已暂停，到不了这里）
        if p.source != "command"
            && !self.panel().is_visible()
            && let Some(a) = self.foreground.external()
            && self.settings.is_excluded(Some(&a.id))
        {
            crate::log::event("hotkey_excluded", serde_json::json!({"app": a.id}));
            return;
        }
        self.panel().toggle(p);
    }

    /// 前台应用在排除列表中时暂停快捷键（Qantrip 自己的窗口除外）
    fn apply_exclusion(&self) {
        let excluded = self.foreground.external().is_some_and(|a| self.settings.is_excluded(Some(&a.id)));
        self.hotkey.set_paused(excluded);
    }

    fn language_changed(self: &Rc<Self>) {
        i18n::set_ui_locale(self.settings.ui_locale());
        self.engine.set_locale(self.settings.ui_locale(), self.settings.template_locale());
        self.panel().language_changed();
        self.library_reload();
        if let Some(lib) = self.library.borrow().clone() {
            lib.language_changed();
        }
        self.refresh_tray();
    }

    /// 模板目录变化（包括外部编辑器修改）时自动重新加载
    fn watch_templates(self: &Rc<Self>) {
        let dir = gio::File::for_path(paths::templates_dir());
        let Ok(mon) = dir.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else { return };
        let weak = Rc::downgrade(self);
        mon.connect_changed(move |_, _, _, _| {
            let Some(app) = weak.upgrade() else { return };
            if let Some(id) = app.reload_source.borrow_mut().take() {
                id.remove();
            }
            let weak2 = Rc::downgrade(&app);
            let id = glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                if let Some(app) = weak2.upgrade() {
                    app.reload_source.borrow_mut().take();
                    app.engine.reload();
                    app.library_reload();
                    app.sync.schedule_sync();
                    app.changed();
                }
            });
            *app.reload_source.borrow_mut() = Some(id);
        });
        *self.watcher.borrow_mut() = Some(mon);
    }

    // MARK: 窗口

    fn library_window(self: &Rc<Self>) -> Rc<LibraryWindow> {
        if let Some(w) = self.library.borrow().clone() {
            return w;
        }
        let w = LibraryWindow::new(self.clone());
        *self.library.borrow_mut() = Some(w.clone());
        w
    }

    pub fn show_library(self: &Rc<Self>, select: Option<String>) {
        self.library_window().show(select);
    }

    /// 设置与模板库在同一个窗口：`page` 为 general / insert / templates / sync / privacy / about
    pub fn show_settings(self: &Rc<Self>, page: Option<&str>) {
        self.library_window().show_settings(page.unwrap_or("general"));
    }

    pub fn library_reload(&self) {
        if let Some(w) = self.library.borrow().clone() {
            w.reload();
        }
    }

    /// 登录完成后把设置窗口带到前台
    pub fn present_settings_if_open(&self) {
        if let Some(w) = self.library.borrow().clone()
            && w.is_visible()
        {
            w.present();
        }
    }

    pub fn show_onboarding(self: &Rc<Self>) {
        let o = match self.onboarding.borrow().clone() {
            Some(o) => o,
            None => crate::onboarding::Onboarding::new(self.clone()),
        };
        *self.onboarding.borrow_mut() = Some(o.clone());
        o.show();
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn library_gtk_window(&self) -> Option<gtk::Window> {
        self.library.borrow().as_ref().map(|l| l.window().clone().upcast())
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn onboarding_gtk_window(&self) -> Option<gtk::Window> {
        self.onboarding.borrow().as_ref().map(|o| o.window().clone())
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn library_handle(&self) -> Option<Rc<LibraryWindow>> {
        self.library.borrow().clone()
    }

    pub fn onboarding_closed(&self) {
        *self.onboarding.borrow_mut() = None;
    }

    pub fn refresh_tray(&self) {
        if let Ok(t) = self.tray.try_borrow()
            && let Some(t) = t.as_ref()
        {
            t.refresh();
        }
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn has_tray(&self) -> bool {
        self.tray.try_borrow().map(|t| t.is_some()).unwrap_or(false)
    }

    pub fn quit(&self) {
        self.gapp.quit();
    }

    fn shutdown(&self) {
        if let Some(conn) = self.gapp.dbus_connection() {
            self.foreground.stop(Some(&conn));
        }
        if let Some(t) = self.tray.borrow_mut().take() {
            t.stop();
        }
        if let Some(x) = &self.x11 {
            x.ungrab();
        }
    }

    // MARK: 命令行

    fn handle_command_line(self: &Rc<Self>, args: &[String], cmd: &gio::ApplicationCommandLine) -> u8 {
        let first = args.first().map(String::as_str).unwrap_or("");
        let remote = cmd.is_remote();
        match first {
            "" | "--background" | "--gapplication-service" => {
                if !self.settings.onboarding_done() {
                    self.show_onboarding();
                } else if first.is_empty() && (remote || !self.launched_by_autostart()) {
                    // 从应用菜单再次打开：显示设置（GNOME 没有托盘时的入口）
                    self.show_settings(None);
                }
                0
            }
            "toggle" => {
                self.hotkey.press(Press { source: "command", timestamp: 0, token: None });
                0
            }
            "show" | "panel" => {
                if !self.panel().is_visible() {
                    self.panel().show(&Press { source: "command", timestamp: 0, token: None });
                }
                0
            }
            "settings" | "--settings" => {
                self.show_settings(args.get(1).map(String::as_str));
                0
            }
            "library" | "--library" => {
                self.show_library(args.get(1).cloned());
                0
            }
            "onboarding" | "welcome" => {
                self.show_onboarding();
                0
            }
            "quit" => {
                self.quit();
                0
            }
            #[cfg(all(feature = "debug-hooks", debug_assertions))]
            "debug" => crate::debug::handle(self, &args[1..], cmd),
            _ => {
                cmd.printerr_literal(&format!("{}\n\n{}", l("未知命令", "Unknown command"), crate::cli::help()));
                2
            }
        }
    }

    fn launched_by_autostart(&self) -> bool {
        std::env::args().any(|a| a == "--background")
    }
}
