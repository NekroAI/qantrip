//! Nekro Qantrip 的 Linux 外壳：GTK4 + libadwaita，直接调用 Rust 核心（docs/decisions.md D37）。

// 回调多以 `Rc<dyn Fn…>` 保存在 `RefCell` 中，类型较长但含义直接
#![allow(clippy::type_complexity)]

mod app;
mod autopaste;
mod autostart;
mod cli;
#[cfg(all(feature = "debug-hooks", debug_assertions))]
mod debug;
mod foreground;
mod hotkey;
mod i18n;
mod ime;
mod insert;
mod library;
mod log;
mod notify;
mod onboarding;
mod panel;
mod paths;
mod secrets;
mod session;
mod settings;
mod settings_ui;
mod sync;
mod sync_ui;
mod theme;
mod tray;
mod update;
mod util;
mod x11;

pub const APP_ID: &str = "ai.nekro.qantrip";
pub const VERSION: &str = env!("QANTRIP_VERSION");

fn main() -> gtk::glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // 不需要界面的命令直接在本进程完成
    if let Some(code) = cli::run(&args) {
        return gtk::glib::ExitCode::from(code as u8);
    }
    theme::register_fonts();
    // 常驻的小工具优先内存占用：没有指定渲染器时用 Cairo（在没有 GPU 的机器上，GL / Vulkan 渲染器经软件实现
    // 常驻 300MB 以上，Cairo 约 140MB；面板与设置界面用 Cairo 渲染足够流畅）。用户设置了 GSK_RENDERER 时尊重其选择。
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY：此时还没有启动任何线程
        unsafe { std::env::set_var("GSK_RENDERER", "cairo") };
        unsafe { std::env::set_var("QANTRIP_SET_GSK_RENDERER", "1") };
    }
    app::run()
}
