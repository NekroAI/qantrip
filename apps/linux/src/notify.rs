//! 提示：使用桌面通知（org.freedesktop.Notifications），不抢焦点，相当于 macOS / Windows 的 HUD。

use std::cell::Cell;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

thread_local! {
    /// 上一条提示的编号：新提示替换旧提示，避免堆积
    static LAST: Cell<u32> = const { Cell::new(0) };
}

/// 短暂提示（约 3 秒，不进入通知中心历史）
pub fn hud(text: &str) {
    send("Nekro Qantrip", text, true, 3000);
}

/// 需要用户留意的提示（会留在通知中心）
pub fn notice(summary: &str, body: &str) {
    send(summary, body, false, -1);
}

fn send(summary: &str, body: &str, transient: bool, timeout: i32) {
    let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return };
    let hints = glib::VariantDict::new(None);
    hints.insert("desktop-entry", crate::APP_ID);
    if transient {
        hints.insert("transient", true);
        hints.insert("urgency", 0u8);
    }
    let replaces = if transient { LAST.with(|c| c.get()) } else { 0 };
    let args = ("Nekro Qantrip", replaces, crate::APP_ID, summary, body, Vec::<String>::new(), hints.end(), timeout).to_variant();
    let transient_flag = transient;
    conn.call(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
        Some(&args),
        Some(glib::VariantTy::new("(u)").unwrap()),
        gio::DBusCallFlags::NONE,
        3000,
        gio::Cancellable::NONE,
        move |r| {
            if let Ok(v) = r
                && let Some((id,)) = v.get::<(u32,)>()
                && transient_flag
            {
                LAST.with(|c| c.set(id));
            }
        },
    );
}
