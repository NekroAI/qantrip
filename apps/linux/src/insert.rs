//! 跨应用插入：剪贴板 + 模拟粘贴（PRD 7.2，docs/decisions.md D37）。
//!
//! 流程：（X11）唤起面板时备份剪贴板全部格式 → 面板仍有焦点时写入 Prompt（附加 KDE 剪贴板历史的
//! 排除标记）→ 关闭面板 → 确认目标回到前台 → 发送粘贴按键 → 延迟后恢复原剪贴板（期间剪贴板被其他程序
//! 改动则不覆盖）。Wayland 下应用失去焦点后不能再写剪贴板，因此不恢复，Prompt 留在剪贴板中。
//! 只复制模式与无法自动粘贴时，Prompt 留在剪贴板，提示用户手动粘贴。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::autopaste::{self, AutoPaste, Combo, Method};
use crate::foreground::{self, AppRef};
use crate::i18n::l;
use crate::settings::{PasteKey, Settings};
use crate::t;
use crate::x11::X11;

/// KDE 剪贴板（Klipper）与多数剪贴板管理器据此不记录这一条（KeePassXC 等密码管理器的做法）
const HISTORY_HINT_MIME: &str = "x-kde-passwordManagerHint";
const SNAPSHOT_LIMIT: usize = 32 * 1024 * 1024;

pub struct Inserter {
    settings: Rc<Settings>,
    autopaste: Rc<AutoPaste>,
    x11: Option<Arc<X11>>,
    wayland: bool,
    /// 唤起面板时的剪贴板快照（X11）：(MIME, 内容)
    snapshot: RefCell<Option<Vec<(String, glib::Bytes)>>>,
}

pub enum Outcome {
    Pasted,
    Copied { reason: String },
}

/// 文本的各种 MIME 类型都直接给 UTF-8 字节：GTK 默认把字符串序列化为不带 charset 的 `text/plain` 时
/// 会把非 ASCII 字符转义（`\E4\BD\A0`），按 `text/plain` 读取的程序（如 wl-paste、部分终端）就会得到乱码
const TEXT_MIMES: [&str; 5] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "TEXT", "STRING"];

fn text_provider(text: &str, hint: bool) -> gdk::ContentProvider {
    let bytes = glib::Bytes::from(text.as_bytes());
    let mut parts: Vec<gdk::ContentProvider> = TEXT_MIMES.iter().map(|m| gdk::ContentProvider::for_bytes(m, &bytes)).collect();
    if hint {
        parts.push(gdk::ContentProvider::for_bytes(HISTORY_HINT_MIME, &glib::Bytes::from_static(b"secret")));
    }
    gdk::ContentProvider::new_union(&parts)
}

fn clipboard() -> Option<gdk::Clipboard> {
    gdk::Display::default().map(|d| d.clipboard())
}

/// 只复制：Prompt 就是用户要用的内容，正常写入剪贴板、不加排除标记。
pub fn copy_plain(text: &str) {
    if let Some(cb) = clipboard() {
        cb.set_content(Some(&text_provider(text, false))).ok();
    }
}

impl Inserter {
    pub fn new(settings: Rc<Settings>, autopaste: Rc<AutoPaste>, x11: Option<Arc<X11>>, wayland: bool) -> Rc<Inserter> {
        Rc::new(Inserter { settings, autopaste, x11, wayland, snapshot: RefCell::new(None) })
    }

    /// 面板打开时调用（X11）：异步备份剪贴板的全部格式，插入后恢复。
    pub fn begin(self: &Rc<Self>) {
        *self.snapshot.borrow_mut() = None;
        if self.wayland || self.settings.copy_only() {
            return;
        }
        let Some(cb) = clipboard() else { return };
        if cb.is_local() {
            crate::log::event("clipboard_snapshot_skipped", serde_json::json!({"reason": "local"}));
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let mimes: Vec<String> = cb.formats().mime_types().iter().map(|m| m.to_string()).collect();
            crate::log::event("clipboard_formats", serde_json::json!({"mimes": mimes}));
            let mut out = Vec::new();
            let mut total = 0usize;
            for mime in mimes {
                // 只备份真正的数据格式；X11 的 TARGETS 等元数据由系统生成
                if mime.is_empty() || mime == HISTORY_HINT_MIME {
                    continue;
                }
                let Ok((stream, _)) = cb.read_future(&[mime.as_str()], glib::Priority::DEFAULT).await else { continue };
                let mut data = Vec::new();
                loop {
                    match stream.read_bytes_future(64 * 1024, glib::Priority::DEFAULT).await {
                        Ok(b) if !b.is_empty() => {
                            data.extend_from_slice(&b);
                            if total + data.len() > SNAPSHOT_LIMIT {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                total += data.len();
                if total > SNAPSHOT_LIMIT {
                    // 太大（如大图）：放弃备份，插入后不恢复
                    *this.snapshot.borrow_mut() = None;
                    return;
                }
                out.push((mime, glib::Bytes::from_owned(data)));
            }
            crate::log::event("clipboard_snapshot", serde_json::json!({"formats": out.iter().map(|(m, b)| format!("{m}:{}", b.len())).collect::<Vec<_>>()}));
            *this.snapshot.borrow_mut() = Some(out);
        });
    }

    /// 选择粘贴按键
    pub fn combo_for(&self, target: Option<&AppRef>) -> Combo {
        match self.settings.paste_key() {
            PasteKey::CtrlV => autopaste::CTRL_V,
            PasteKey::CtrlShiftV => autopaste::CTRL_SHIFT_V,
            PasteKey::ShiftInsert => autopaste::SHIFT_INSERT,
            PasteKey::Auto => match target {
                Some(a) if foreground::is_terminal(&a.id) => autopaste::CTRL_SHIFT_V,
                Some(_) => autopaste::CTRL_V,
                // 识别不出前台应用（GNOME Wayland）：Shift+Insert 在 GTK、Qt、浏览器与终端中都粘贴
                None => autopaste::SHIFT_INSERT,
            },
        }
    }

    /// 插入。`hide` 在写入剪贴板之后、发送按键之前调用（关闭面板，焦点回到目标应用）。
    pub fn insert(self: &Rc<Self>, text: String, target: Option<AppRef>, expand: bool, hide: impl FnOnce() + 'static, done: impl FnOnce(Outcome) + 'static) {
        let started = crate::util::uptime();
        let expand = expand && self.settings.expand_paste_enabled();
        if self.settings.copy_only() {
            copy_plain(&text);
            hide();
            self.log("copy_only_mode", &text, started, serde_json::json!({}));
            done(Outcome::Copied { reason: l("只复制模式", "copy-only mode") });
            return;
        }
        let method = self.autopaste.method();
        if method == Method::None {
            copy_plain(&text);
            hide();
            self.log("no_autopaste", &text, started, serde_json::json!({}));
            done(Outcome::Copied { reason: l("未授权自动粘贴", "auto-paste not allowed") });
            return;
        }
        let combo = self.combo_for(target.as_ref());
        // Wayland：必须在面板仍有焦点时写入剪贴板
        let Some(cb) = clipboard() else {
            hide();
            done(Outcome::Copied { reason: l("无法访问剪贴板", "clipboard unavailable") });
            return;
        };
        let provider = text_provider(&text, true);
        cb.set_content(Some(&provider)).ok();
        if combo == autopaste::SHIFT_INSERT
            && let Some(d) = gdk::Display::default()
        {
            d.primary_clipboard().set_content(Some(&text_provider(&text, true))).ok();
        }
        let snapshot = self.snapshot.borrow_mut().take();
        hide();

        let this = self.clone();
        glib::spawn_future_local(async move {
            // 等待焦点回到目标应用
            let delay = if this.wayland { 160 } else { 40 };
            glib::timeout_future(Duration::from_millis(delay)).await;
            if let (Some(x), Some(xid)) = (&this.x11, target.as_ref().and_then(|t| t.xid)) {
                let mut ok = false;
                for _ in 0..15 {
                    if x.active_xid() == Some(xid) {
                        ok = true;
                        break;
                    }
                    glib::timeout_future(Duration::from_millis(20)).await;
                }
                if !ok {
                    copy_plain(&text);
                    this.log("target_changed", &text, started, serde_json::json!({}));
                    done(Outcome::Copied { reason: l("目标应用已切换", "target app changed") });
                    return;
                }
            }
            let gap = Duration::from_millis(this.settings.expand_paste_interval_ms() as u64);
            let paste_at = crate::util::uptime();
            let result = this.autopaste.send(combo, if expand { 2 } else { 1 }, gap).await;
            if let Err(e) = result {
                copy_plain(&text);
                this.log("paste_failed", &text, started, serde_json::json!({"error": e}));
                done(Outcome::Copied { reason: l("自动粘贴失败", "auto-paste failed") });
                return;
            }
            done(Outcome::Pasted);
            // 恢复原剪贴板（X11）
            let mut restored = false;
            if !this.wayland {
                glib::timeout_future(Duration::from_millis(500)).await;
                if let (Some(snap), Some(cb)) = (snapshot, clipboard())
                    && cb.is_local()
                    && !snap.is_empty()
                {
                    let mut parts: Vec<gdk::ContentProvider> = snap.iter().map(|(m, b)| gdk::ContentProvider::for_bytes(m, b)).collect();
                    parts.push(gdk::ContentProvider::for_bytes(HISTORY_HINT_MIME, &glib::Bytes::from_static(b"secret")));
                    cb.set_content(Some(&gdk::ContentProvider::new_union(&parts))).ok();
                    restored = true;
                }
            }
            this.log(
                "pasted",
                &text,
                started,
                serde_json::json!({
                    "combo": combo.label(),
                    "method": format!("{:?}", this.autopaste.method()),
                    "target": target.as_ref().map(|t| t.id.clone()).unwrap_or_default(),
                    "paste_ms": crate::util::ms(paste_at - started),
                    "expand": expand,
                    "restored": restored,
                }),
            );
        });
    }

    fn log(&self, mode: &str, text: &str, started: f64, extra: serde_json::Value) {
        let mut v = extra;
        if let Some(o) = v.as_object_mut() {
            o.insert("mode".into(), mode.into());
            o.insert("chars".into(), text.chars().count().into());
            o.insert("lines".into(), text.split('\n').count().into());
            o.insert("total_ms".into(), crate::util::ms(crate::util::uptime() - started).into());
        }
        crate::log::event("insert", v);
    }
}

/// 目标是 Qantrip 自己的窗口（引导页的练习框、模板编辑器）：直接写入当时的焦点输入框，不经过剪贴板。
pub fn insert_into_own(window: &gtk::Window, focus: Option<gtk::Widget>, text: &str) -> bool {
    window.present();
    let Some(w) = focus else { return false };
    if let Some(tv) = w.downcast_ref::<gtk::TextView>() {
        if !tv.is_editable() {
            return false;
        }
        let buf = tv.buffer();
        buf.delete_selection(true, true);
        buf.insert_at_cursor(text);
        tv.grab_focus();
        return true;
    }
    if let Some(e) = w.dynamic_cast_ref::<gtk::Editable>() {
        if !e.is_editable() {
            return false;
        }
        // 单行输入框：换行换成空格
        let line = text.replace("\r\n", " ").replace('\n', " ");
        e.delete_selection();
        let mut pos = e.position();
        e.insert_text(&line, &mut pos);
        e.set_position(pos);
        w.grab_focus();
        return true;
    }
    false
}

/// 复制后提示的文字
pub fn copied_message(reason: &str, combo: Combo) -> String {
    let key = if combo == autopaste::CTRL_SHIFT_V { "Ctrl+Shift+V" } else { "Ctrl+V" };
    t!("已复制到剪贴板（{reason}），按 {key} 粘贴", "Copied to clipboard ({reason}). Press {key} to paste")
}

#[allow(dead_code)]
fn _assert_gio(_: gio::Cancellable) {}
