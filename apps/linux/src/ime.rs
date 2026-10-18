//! 输入法（docs/decisions.md D9）：唤起面板时切到英文，关闭后恢复。设置中可关闭。
//!
//! - Fcitx5：D-Bus `org.fcitx.Fcitx5` 的 `/controller`：面板中 `State()` 为 2（激活）时 `Deactivate()`。
//!   Fcitx5 的中英状态按输入上下文保存，原应用的状态不受影响，关闭面板后无需恢复。
//! - IBus：读取当前引擎；是中文等非键盘布局引擎时切到对应的 xkb 引擎（GNOME 输入源列表中的键盘布局），
//!   关闭面板后切回。IBus 的引擎是全局的，GNOME 会随之更新输入源指示器。
//!
//! 输入中文附加要求时，用户手动切换即可（与 macOS 相同）。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::settings::Settings;

enum Restore {
    Fcitx,
    Ibus(String),
}

pub struct Ime {
    settings: Rc<Settings>,
    restore: RefCell<Option<Restore>>,
    active: std::cell::Cell<bool>,
}

fn session_bus() -> Option<gio::DBusConnection> {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()
}

fn fcitx_call(conn: &gio::DBusConnection, method: &str) -> Option<glib::Variant> {
    conn.call_sync(
        Some("org.fcitx.Fcitx5"),
        "/controller",
        "org.fcitx.Fcitx.Controller1",
        method,
        None,
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        300,
        gio::Cancellable::NONE,
    )
    .ok()
}

fn ibus(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("ibus").args(args).stderr(std::process::Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// GNOME 输入源中的第一个键盘布局（如 `us`、`de+nodeadkeys`），没有时为 `us`
fn keyboard_layout() -> (String, String) {
    let source = gio::SettingsSchemaSource::default().and_then(|s| s.lookup("org.gnome.desktop.input-sources", true));
    if source.is_some() {
        let s = gio::Settings::new("org.gnome.desktop.input-sources");
        if let Some(list) = s.value("sources").get::<Vec<(String, String)>>() {
            for (kind, id) in list {
                if kind == "xkb" {
                    let (layout, variant) = id.split_once('+').unwrap_or((&id, ""));
                    return (layout.to_string(), variant.to_string());
                }
            }
        }
    }
    ("us".into(), String::new())
}

/// 对应的 IBus 键盘引擎名（如 `xkb:us::eng`），在后台线程中调用
fn ibus_keyboard_engine(layout: &str, variant: &str) -> String {
    let names: Vec<String> = ibus(&["list-engine"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().split(" - ").next().map(|n| n.trim().to_string()))
        .filter(|n| n.starts_with("xkb:"))
        .collect();
    let prefix = format!("xkb:{layout}:{variant}:");
    names
        .iter()
        .find(|n| n.starts_with(&prefix))
        .or_else(|| names.iter().find(|n| n.starts_with("xkb:us::")))
        .cloned()
        .unwrap_or_else(|| "xkb:us::eng".to_string())
}

impl Ime {
    pub fn new(settings: Rc<Settings>) -> Rc<Ime> {
        Rc::new(Ime { settings, restore: RefCell::new(None), active: std::cell::Cell::new(false) })
    }

    /// 面板打开：切到英文。Fcitx5 需在面板输入框获得焦点后执行，因此稍后调用；IBus 命令在后台线程执行。
    pub fn engage(self: &Rc<Self>) {
        *self.restore.borrow_mut() = None;
        self.active.set(true);
        if !self.settings.force_ascii_input() {
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            glib::timeout_future(std::time::Duration::from_millis(60)).await;
            if !this.active.get() {
                return;
            }
            if let Some(conn) = session_bus()
                && let Some(v) = fcitx_call(&conn, "State")
            {
                crate::log::event("ime", serde_json::json!({"im": "fcitx5", "state": v.get::<(i32,)>().map(|t| t.0)}));
                if v.get::<(i32,)>().map(|t| t.0) == Some(2) {
                    fcitx_call(&conn, "Deactivate");
                    *this.restore.borrow_mut() = Some(Restore::Fcitx);
                }
                return;
            }
            // IBus：仅在正在使用非键盘布局引擎（如拼音）时切换
            let (layout, variant) = keyboard_layout();
            let prev = gio::spawn_blocking(move || {
                let engine = ibus(&["engine"])?;
                if engine.is_empty() || engine.starts_with("xkb:") {
                    return None;
                }
                let kb = ibus_keyboard_engine(&layout, &variant);
                ibus(&["engine", &kb]).map(|_| engine)
            })
            .await
            .ok()
            .flatten();
            crate::log::event("ime", serde_json::json!({"im": "ibus", "previous": prev.clone()}));
            if let Some(engine) = prev {
                if this.active.get() {
                    *this.restore.borrow_mut() = Some(Restore::Ibus(engine));
                } else {
                    // 面板已经关闭：立即切回
                    std::thread::spawn(move || {
                        let _ = ibus(&["engine", &engine]);
                    });
                }
            }
        });
    }

    /// 面板关闭：恢复原输入法
    pub fn restore(&self) {
        self.active.set(false);
        match self.restore.borrow_mut().take() {
            Some(Restore::Fcitx) => {
                // Fcitx5 的状态按输入上下文保存，面板关闭后原应用保持原状态，无需恢复
            }
            Some(Restore::Ibus(engine)) => {
                std::thread::spawn(move || {
                    let _ = ibus(&["engine", &engine]);
                });
            }
            None => {}
        }
    }
}
