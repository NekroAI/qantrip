//! 调试钩子（与 macOS 的 QANTRIP_DEBUG_HOOKS 对应）：只编译进 debug 构建且打开 `debug-hooks` 特性时，
//! 并且运行中的实例以环境变量 `QANTRIP_DEBUG_HOOKS=1` 启动才生效。经 `qantrip debug <操作>` 转交给运行中的实例。
//! 它允许本机其他进程驱动界面、插入任意文本，正式构建不包含。
//!
//! 操作：
//! - `toggle` / `type <文本> [下移次数]` / `key enter|ctrl-enter|tab|up|down|esc` / `param <名称> toggle|clear|begin|set:<值>|choice:<值>`
//! - `insert <文本>`：按面板相同的流程插入到当前前台应用
//! - `window settings[:页面] | library[:模板名] | onboarding`
//! - `editor new|save|mode:source|mode:form|desc:<文本>|try:<文本>`（模板库中当前编辑的模板）
//! - `sync signin|create|join|confirm:<码>|approve:<码>|sync|reverify|refresh|status`
//! - `paste authorize|forget`：请求 / 撤销 RemoteDesktop 授权
//! - `hotkey retry|configure`
//! - `shot panel|library|onboarding <PNG 路径>`：把窗口渲染为图片
//! - `status`：输出当前状态（JSON）

use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use crate::app::App;
use crate::hotkey::Press;

pub fn enabled() -> bool {
    std::env::var("QANTRIP_DEBUG_HOOKS").as_deref() == Ok("1")
}

/// 把窗口内容渲染为 PNG
pub fn render_window(w: &gtk::Window, path: &str) -> Result<(), String> {
    let (width, height) = (w.width(), w.height());
    if width <= 0 || height <= 0 {
        return Err("window not realized".into());
    }
    let paintable = gtk::WidgetPaintable::new(Some(w));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, width as f64, height as f64);
    let node = snapshot.to_node().ok_or("empty")?;
    let renderer = w.renderer().ok_or("no renderer")?;
    let texture = renderer.render_texture(&node, None);
    texture.save_to_png(path).map_err(|e| e.to_string())
}

pub fn handle(app: &Rc<App>, args: &[String], cmd: &gio::ApplicationCommandLine) -> u8 {
    if !enabled() {
        cmd.printerr_literal("debug hooks disabled\n");
        return 1;
    }
    let op = args.first().map(String::as_str).unwrap_or("");
    let arg = args.get(1).cloned().unwrap_or_default();
    let panel = app.panel();
    match op {
        "toggle" => panel.toggle(&Press { source: "debug", timestamp: 0, token: None }),
        "type" => panel.debug_type(&arg, args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0)),
        "key" => panel.debug_key(&arg),
        "param" => panel.debug_param(&arg, args.get(2).map(String::as_str).unwrap_or("")),
        "insert" => {
            let target = app.foreground.external();
            let inserter = app.inserter.clone();
            inserter.insert(
                arg,
                target,
                false,
                || {},
                |o| {
                    if let crate::insert::Outcome::Copied { reason } = o {
                        crate::log::event("debug_insert_copied", serde_json::json!({"reason": reason}));
                    }
                },
            );
        }
        "window" => {
            let (name, sub) = arg.split_once(':').map(|(a, b)| (a.to_string(), Some(b.to_string()))).unwrap_or((arg.clone(), None));
            match name.as_str() {
                "settings" => app.show_settings(sub.as_deref()),
                "library" => app.show_library(sub),
                "onboarding" => app.show_onboarding(),
                _ => {}
            }
        }
        "editor" => crate::library::debug_editor(app, &arg),
        "sync" => debug_sync(app, &arg),
        "paste" => match arg.as_str() {
            "authorize" => app.autopaste.authorize(),
            "forget" => app.autopaste.forget(),
            _ => {}
        },
        "hotkey" => match arg.as_str() {
            "retry" => app.hotkey.retry(),
            "configure" => {
                app.hotkey.configure();
            }
            _ => {}
        },
        "shot" => {
            let path = args.get(2).cloned().unwrap_or_else(|| "/tmp/qantrip-shot.png".into());
            let win: Option<gtk::Window> = match arg.as_str() {
                "panel" => Some(panel.window().clone()),
                "library" | "settings" => app.library_gtk_window(),
                "onboarding" => app.onboarding_gtk_window(),
                _ => None,
            };
            let r = win.ok_or_else(|| "no window".to_string()).and_then(|w| render_window(&w, &path));
            match r {
                Ok(_) => cmd.print_literal(&format!("{path}\n")),
                Err(e) => {
                    cmd.printerr_literal(&format!("{e}\n"));
                    return 1;
                }
            }
        }
        "status" => {
            let s = serde_json::json!({
                "session": app.session.label(),
                "hotkey_mode": format!("{:?}", app.hotkey.mode()),
                "hotkey_state": format!("{:?}", app.hotkey.state()),
                "hotkey_paused": app.hotkey.paused(),
                "autopaste": format!("{:?}", app.autopaste.method()),
                "autopaste_portal": format!("{:?}", app.autopaste.portal_state()),
                "foreground": app.foreground.current().map(|a| a.id),
                "foreground_source": format!("{:?}", app.foreground.source()),
                "panel_visible": panel.is_visible(),
                "query": panel.model.borrow().query.clone(),
                "tray": app.has_tray(),
                "sync_signed_in": app.sync.signed_in(),
                "sync_backend": format!("{:?}", app.sync.backend.get()),
                "rss_mb": rss_mb(),
            });
            cmd.print_literal(&format!("{s}\n"));
        }
        _ => {
            cmd.printerr_literal("unknown debug op\n");
            return 2;
        }
    }
    0
}

fn debug_sync(app: &Rc<App>, op: &str) {
    let s = &app.sync;
    let (name, arg) = op.split_once(':').map(|(a, b)| (a, b.to_string())).unwrap_or((op, String::new()));
    match name {
        "signin" => s.sign_in(),
        "create" => s.create_vault(),
        "join" => s.start_join(),
        "confirm" => s.confirm_join(arg),
        "approve" => {
            let id = s.join_requests.borrow().iter().find(|r| r.status == qantrip_core::sync::JoinRequestStatus::ReadyForCode).map(|r| r.join_id.clone());
            if let Some(id) = id {
                s.approve(id, arg);
            }
        }
        "sync" => s.sync_now(),
        "reverify" => s.reverify(),
        "refresh" => s.refresh(),
        _ => {}
    }
}

/// 常驻内存（MB）
pub fn rss_mb() -> f64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: f64 = s.split_whitespace().nth(1).and_then(|x| x.parse().ok()).unwrap_or(0.0);
    (pages * 4096.0 / 1_048_576.0 * 10.0).round() / 10.0
}
