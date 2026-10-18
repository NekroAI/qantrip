//! 当前前台应用（插入目标、按应用排序、排除的应用、终端识别）。
//!
//! - X11：根窗口 `_NET_ACTIVE_WINDOW` + `WM_CLASS`（见 x11.rs）。
//! - KDE Plasma（Wayland）：加载一个 KWin 脚本，窗口激活时经 D-Bus 回调本应用（`resourceClass` 即 app_id）。
//! - GNOME（Wayland）等：系统不向应用公开前台窗口，取不到（界面中说明）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppRef {
    /// X11 为 WM_CLASS 类名，Wayland（KDE）为 app_id；排除列表按小写比较
    pub id: String,
    /// 显示名称
    pub name: String,
    pub pid: Option<u32>,
    /// X11 窗口（插入前确认目标仍在前台）
    pub xid: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    X11,
    KWin,
    /// 取不到前台应用
    None,
}

/// 桌面外壳自身的窗口（面板、桌面），不作为插入目标或排除对象
const SHELL_IDS: &[&str] = &[
    "plasmashell",
    "gnome-shell",
    "xfce4-panel",
    "xfdesktop",
    "org.kde.plasmashell",
    "krunner",
    "org.kde.krunner",
    "kwin_wayland",
    "kwin_x11",
    "desktop_window",
    "",
];

pub struct Foreground {
    source: Cell<Source>,
    current: RefCell<Option<AppRef>>,
    /// 最近一个既不是 Qantrip、也不是桌面外壳的前台应用（托盘菜单中"在当前应用中停用"使用）
    last_external: RefCell<Option<AppRef>>,
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
    kwin_loaded: Cell<bool>,
    /// 最近用过的应用（设置中"添加排除的应用"列表）
    recent: RefCell<Vec<AppRef>>,
}

const KWIN_PLUGIN: &str = "ai.nekro.qantrip.foreground";
pub const OBJECT_PATH: &str = "/ai/nekro/qantrip/Foreground";
const INTERFACE: &str = "ai.nekro.qantrip.Foreground";

const KWIN_SCRIPT: &str = r#"// Nekro Qantrip：把前台窗口告诉 Qantrip（用于排除的应用与插入目标）。只读取窗口类名、标题与进程号。
function report(w) {
    callDBus("ai.nekro.qantrip", "/ai/nekro/qantrip/Foreground", "ai.nekro.qantrip.Foreground", "Activated",
        w ? String(w.resourceClass || "") : "", w ? String(w.caption || "") : "", w ? Number(w.pid || 0) : 0);
}
if (workspace.windowActivated) {
    workspace.windowActivated.connect(report);
    report(workspace.activeWindow);
} else {
    workspace.clientActivated.connect(report);
    report(workspace.activeClient);
}
"#;

fn is_shell(id: &str) -> bool {
    let l = id.to_lowercase();
    SHELL_IDS.contains(&l.as_str())
}

pub fn is_own(app: &AppRef) -> bool {
    app.pid == Some(std::process::id()) || app.id.eq_ignore_ascii_case(crate::APP_ID) || app.id.eq_ignore_ascii_case("qantrip")
}

impl Foreground {
    pub fn new() -> Rc<Foreground> {
        Rc::new(Foreground {
            source: Cell::new(Source::None),
            current: RefCell::new(None),
            last_external: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            kwin_loaded: Cell::new(false),
            recent: RefCell::new(Vec::new()),
        })
    }

    pub fn source(&self) -> Source {
        self.source.get()
    }

    pub fn set_source(&self, s: Source) {
        self.source.set(s);
    }

    pub fn current(&self) -> Option<AppRef> {
        self.current.borrow().clone()
    }

    /// 当前前台应用（Qantrip 自己的窗口除外时返回 None）
    pub fn external(&self) -> Option<AppRef> {
        self.current().filter(|a| !is_own(a) && !is_shell(&a.id))
    }

    pub fn last_external(&self) -> Option<AppRef> {
        self.last_external.borrow().clone()
    }

    pub fn recent(&self) -> Vec<AppRef> {
        self.recent.borrow().clone()
    }

    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    pub fn update(&self, app: Option<AppRef>) {
        if *self.current.borrow() == app {
            return;
        }
        if let Some(a) = &app
            && !is_own(a)
            && !is_shell(&a.id)
        {
            *self.last_external.borrow_mut() = Some(a.clone());
            let mut r = self.recent.borrow_mut();
            r.retain(|x| !x.id.eq_ignore_ascii_case(&a.id));
            r.insert(0, a.clone());
            r.truncate(15);
        }
        *self.current.borrow_mut() = app;
        let list: Vec<_> = self.listeners.borrow().clone();
        for f in list {
            f();
        }
    }

    // MARK: KWin 脚本

    /// 在 KDE Plasma 中加载 KWin 脚本。需要应用已在会话总线上注册（GApplication 启动后）。
    pub fn start_kwin(self: &Rc<Self>, conn: &gio::DBusConnection) {
        // 接收脚本回调的对象
        let xml = format!(
            r#"<node><interface name="{INTERFACE}"><method name="Activated"><arg type="s" name="app" direction="in"/><arg type="s" name="caption" direction="in"/><arg type="i" name="pid" direction="in"/></method></interface></node>"#
        );
        let Ok(node) = gio::DBusNodeInfo::for_xml(&xml) else { return };
        let Some(iface) = node.lookup_interface(INTERFACE) else { return };
        let weak = Rc::downgrade(self);
        let reg = conn
            .register_object(OBJECT_PATH, &iface)
            .method_call(move |_conn, _sender, _path, _iface, method, params, invocation| {
                if method == "Activated"
                    && let Some((app, caption, pid)) = params.get::<(String, String, i32)>()
                    && let Some(this) = weak.upgrade()
                {
                    let a = if app.is_empty() && caption.is_empty() {
                        None
                    } else {
                        Some(AppRef { name: pretty_name(&app, &caption), id: app, pid: (pid > 0).then_some(pid as u32), xid: None })
                    };
                    this.update(a);
                }
                invocation.return_value(None);
            })
            .build();
        if reg.is_err() {
            return;
        }
        let path = crate::paths::state_dir().join("kwin-foreground.js");
        if crate::paths::write_atomic(&path, KWIN_SCRIPT.as_bytes()).is_err() {
            return;
        }
        let conn = conn.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let call = |method: &str, args: glib::Variant, obj: String, iface: &str| {
                conn.call_future(Some("org.kde.KWin"), &obj, iface, method, Some(&args), None, gio::DBusCallFlags::NONE, 2000)
            };
            // 上次异常退出可能残留同名脚本
            let _ = call("unloadScript", (KWIN_PLUGIN,).to_variant(), "/Scripting".into(), "org.kde.kwin.Scripting").await;
            let id =
                match call("loadScript", (path.to_string_lossy().to_string(), KWIN_PLUGIN).to_variant(), "/Scripting".into(), "org.kde.kwin.Scripting").await {
                    Ok(v) => v.get::<(i32,)>().map(|t| t.0).unwrap_or(-1),
                    Err(e) => {
                        crate::log::event("kwin_script_failed", serde_json::json!({"error": e.to_string()}));
                        return;
                    }
                };
            if id < 0 {
                return;
            }
            // Plasma 6：/Scripting/Script<id>；Plasma 5：/<id>
            let mut ok = false;
            for obj in [format!("/Scripting/Script{id}"), format!("/{id}")] {
                if call("run", ().to_variant(), obj, "org.kde.kwin.Script").await.is_ok() {
                    ok = true;
                    break;
                }
            }
            if ok {
                this.kwin_loaded.set(true);
                this.source.set(Source::KWin);
            }
            crate::log::event("kwin_script", serde_json::json!({"id": id, "running": ok}));
        });
    }

    /// 退出时卸载 KWin 脚本
    pub fn stop(&self, conn: Option<&gio::DBusConnection>) {
        if !self.kwin_loaded.get() {
            return;
        }
        if let Some(conn) = conn {
            let _ = conn.call_sync(
                Some("org.kde.KWin"),
                "/Scripting",
                "org.kde.kwin.Scripting",
                "unloadScript",
                Some(&(KWIN_PLUGIN,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                1000,
                gio::Cancellable::NONE,
            );
        }
    }
}

/// 显示名称：优先用 .desktop 文件中的名称，其次窗口类名
pub fn pretty_name(id: &str, title: &str) -> String {
    if !id.is_empty() {
        for candidate in [id.to_string(), id.to_lowercase(), format!("org.gnome.{id}"), format!("org.kde.{}", id.to_lowercase())] {
            if let Some(name) = desktop_name(&candidate) {
                return name;
            }
        }
        let base = id.rsplit('.').next().unwrap_or(id);
        let mut c = base.chars();
        return match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => title.to_string(),
        };
    }
    title.to_string()
}

/// 从 .desktop 文件读取应用名称（按当前语言）
fn desktop_name(id: &str) -> Option<String> {
    let mut dirs = vec![crate::paths::user_applications_dir()];
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data.split(':').filter(|s| !s.is_empty()).map(|d| std::path::Path::new(d).join("applications")));
    for d in dirs {
        let p = d.join(format!("{id}.desktop"));
        if p.exists() {
            let kf = glib::KeyFile::new();
            if kf.load_from_file(&p, glib::KeyFileFlags::NONE).is_ok()
                && let Ok(n) = kf.locale_string("Desktop Entry", "Name", None)
            {
                return Some(n.to_string());
            }
        }
    }
    None
}

/// 终端类应用（粘贴用 Ctrl+Shift+V）
pub fn is_terminal(id: &str) -> bool {
    let l = id.to_lowercase();
    const TERMINALS: &[&str] = &[
        "gnome-terminal",
        "gnome-terminal-server",
        "org.gnome.terminal",
        "org.gnome.console",
        "kgx",
        "org.gnome.ptyxis",
        "ptyxis",
        "konsole",
        "org.kde.konsole",
        "yakuake",
        "org.kde.yakuake",
        "xterm",
        "uxterm",
        "urxvt",
        "rxvt",
        "alacritty",
        "kitty",
        "foot",
        "footclient",
        "org.codeberg.dnkl.foot",
        "wezterm",
        "org.wezfurlong.wezterm",
        "tilix",
        "com.gexperts.tilix",
        "terminator",
        "xfce4-terminal",
        "lxterminal",
        "qterminal",
        "mate-terminal",
        "st",
        "st-256color",
        "sakura",
        "guake",
        "tabby",
        "warp",
        "dev.warp.warp",
        "ghostty",
        "com.mitchellh.ghostty",
        "blackbox",
        "com.raggesilver.blackbox",
        "terminology",
        "cool-retro-term",
        "contour",
        "rio",
        "io.elementary.terminal",
        "deepin-terminal",
        "kitty-quick-access",
        "org.gnome.prompt",
    ];
    TERMINALS.contains(&l.as_str()) || l.ends_with("terminal") || l.ends_with(".terminal")
}
