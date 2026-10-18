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
    // 不附加 desktop-entry：GNOME 在发送进程与该应用对应不上时（如从终端启动）不显示横幅
    let hints = glib::VariantDict::new(None);
    if transient {
        hints.insert("transient", true);
        // 低优先级（0）的通知在 GNOME 中不弹出横幅，因此用普通优先级
        hints.insert("urgency", 1u8);
    }
    let replaces = if transient { LAST.with(|c| c.get()) } else { 0 };
    // 逐项构造参数：直接把 VariantDict 放进元组会被包成 `v`，签名不再是 a{sv}
    let args = glib::Variant::tuple_from_iter([
        "Nekro Qantrip".to_variant(),
        replaces.to_variant(),
        crate::APP_ID.to_variant(),
        summary.to_variant(),
        body.to_variant(),
        Vec::<String>::new().to_variant(),
        hints.end(),
        timeout.to_variant(),
    ]);
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
