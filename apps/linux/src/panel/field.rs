//! 命令输入框：单行 GtkTextView，带语法高亮、错误波浪线与灰字补全（占位文字与灰字共用一个标签，
//! 按光标所在行对齐）。输入法组字期间不解析，也不处理回车 / 方向键（交给输入法）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;

use qantrip_core::{Analysis, Severity, TokenKind};

use crate::theme;
use crate::util;

pub struct CommandField {
    pub view: gtk::TextView,
    pub overlay: gtk::Overlay,
    ghost: gtk::Label,
    placeholder: RefCell<String>,
    ghost_text: RefCell<Option<String>>,
    /// 程序修改文本时不触发 on_change
    suppress: Cell<bool>,
    preedit: RefCell<String>,
    on_change: RefCell<Option<Rc<dyn Fn(String)>>>,
    /// 最近一次交给 on_change 的文本（组字结束后补发一次）
    last_sent: RefCell<String>,
}

const TAGS: &[&str] = &["command", "error", "option", "value", "dim", "diag-error", "diag-warning"];

impl CommandField {
    pub fn new() -> Rc<CommandField> {
        let view = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::None)
            .accepts_tab(false)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .input_hints(gtk::InputHints::NO_SPELLCHECK | gtk::InputHints::NO_EMOJI)
            .build();
        view.add_css_class("command-field");
        view.set_top_margin(2);
        view.set_bottom_margin(2);
        let ghost = gtk::Label::builder().halign(gtk::Align::Start).valign(gtk::Align::Start).can_target(false).xalign(0.0).single_line_mode(true).build();
        ghost.add_css_class("command-ghost");
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&view));
        overlay.add_overlay(&ghost);
        overlay.set_clip_overlay(&ghost, true);
        overlay.set_hexpand(true);

        let buf = view.buffer();
        let table = buf.tag_table();
        for name in TAGS {
            table.add(&gtk::TextTag::new(Some(name)));
        }

        let f = Rc::new(CommandField {
            view,
            overlay,
            ghost,
            placeholder: RefCell::new(String::new()),
            ghost_text: RefCell::new(None),
            suppress: Cell::new(false),
            preedit: RefCell::new(String::new()),
            on_change: RefCell::new(None),
            last_sent: RefCell::new(String::new()),
        });
        f.update_tag_colors();
        let weak = Rc::downgrade(&f);
        adw::StyleManager::default().connect_dark_notify(move |_| {
            if let Some(f) = weak.upgrade() {
                f.update_tag_colors();
            }
        });

        let weak = Rc::downgrade(&f);
        buf.connect_changed(move |b| {
            let Some(f) = weak.upgrade() else { return };
            if f.suppress.get() {
                return;
            }
            let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
            // 粘贴的多行文字：换行换成空格
            if text.contains('\n') || text.contains('\r') {
                let fixed = text.replace("\r\n", " ").replace(['\n', '\r'], " ");
                f.set_text(&fixed, None);
                f.emit(fixed);
                return;
            }
            if !f.preedit.borrow().is_empty() {
                f.ghost.set_visible(false);
                return;
            }
            f.emit(text);
        });
        let weak = Rc::downgrade(&f);
        f.view.connect_preedit_changed(move |_, s| {
            if let Some(f) = weak.upgrade() {
                *f.preedit.borrow_mut() = s.to_string();
                if s.is_empty() {
                    // 组字结束：提交的文字可能在组字状态清除前就已写入，补发一次
                    let text = f.text();
                    if *f.last_sent.borrow() != text {
                        f.emit(text);
                    }
                    f.update_ghost();
                } else {
                    f.ghost.set_visible(false);
                }
            }
        });
        // 光标移动、尺寸变化后重新放置灰字
        let weak = Rc::downgrade(&f);
        buf.connect_mark_set(move |_, _, mark| {
            if mark.name().as_deref() == Some("insert")
                && let Some(f) = weak.upgrade()
            {
                f.update_ghost();
            }
        });
        let weak = Rc::downgrade(&f);
        f.view.connect_map(move |_| {
            if let Some(f) = weak.upgrade() {
                let w2 = Rc::downgrade(&f);
                glib::idle_add_local_once(move || {
                    if let Some(f) = w2.upgrade() {
                        f.update_ghost();
                    }
                });
            }
        });
        f
    }

    fn update_tag_colors(&self) {
        let p = theme::palette();
        let table = self.view.buffer().tag_table();
        let set = |name: &str, f: &dyn Fn(&gtk::TextTag)| {
            if let Some(t) = table.lookup(name) {
                f(&t);
            }
        };
        set("command", &|t| {
            t.set_foreground_rgba(Some(&theme::rgba(p.accent)));
            t.set_weight(700);
        });
        set("error", &|t| t.set_foreground_rgba(Some(&theme::rgba(p.error))));
        set("option", &|t| t.set_foreground_rgba(Some(&theme::rgba(p.option))));
        set("value", &|t| t.set_foreground_rgba(Some(&theme::rgba(p.value))));
        set("dim", &|t| {
            let mut c = self.view.color();
            c.set_alpha(0.4);
            t.set_foreground_rgba(Some(&c));
        });
        set("diag-error", &|t| {
            t.set_underline(pango::Underline::Error);
            t.set_underline_rgba(Some(&theme::rgba(p.error)));
        });
        set("diag-warning", &|t| {
            t.set_underline(pango::Underline::Error);
            t.set_underline_rgba(Some(&theme::rgba(p.warning)));
        });
    }

    fn emit(&self, text: String) {
        *self.last_sent.borrow_mut() = text.clone();
        if let Some(cb) = self.on_change.borrow().clone() {
            cb(text);
        }
    }

    pub fn connect_change(&self, f: impl Fn(String) + 'static) {
        *self.on_change.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_placeholder(&self, s: &str) {
        *self.placeholder.borrow_mut() = s.to_string();
        self.update_ghost();
    }

    pub fn is_composing(&self) -> bool {
        !self.preedit.borrow().is_empty()
    }

    pub fn text(&self) -> String {
        let b = self.view.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    /// 设置文本与光标（UTF-16 偏移；None 为末尾），不触发 on_change
    pub fn set_text(&self, text: &str, caret_u16: Option<u32>) {
        *self.last_sent.borrow_mut() = text.to_string();
        self.suppress.set(true);
        let b = self.view.buffer();
        b.set_text(text);
        let chars = match caret_u16 {
            Some(c) => util::u16_to_char(text, c),
            None => text.chars().count(),
        };
        let it = b.iter_at_offset(chars as i32);
        b.place_cursor(&it);
        self.suppress.set(false);
        self.scroll_to_cursor();
        self.update_ghost();
    }

    pub fn caret_at_end(&self) -> bool {
        let b = self.view.buffer();
        let it = b.iter_at_mark(&b.get_insert());
        it.is_end() && !b.has_selection()
    }

    fn scroll_to_cursor(&self) {
        let b = self.view.buffer();
        self.view.scroll_mark_onscreen(&b.get_insert());
    }

    pub fn focus(&self) {
        self.view.grab_focus();
    }

    /// 按解析结果着色
    pub fn apply(&self, a: &Analysis) {
        *self.ghost_text.borrow_mut() = a.ghost.clone().filter(|g| !g.is_empty());
        if self.is_composing() {
            return;
        }
        let b = self.view.buffer();
        let text = self.text();
        let (s, e) = b.bounds();
        b.remove_all_tags(&s, &e);
        let range = |start: u32, end: u32| {
            let a = b.iter_at_offset(util::u16_to_char(&text, start) as i32);
            let z = b.iter_at_offset(util::u16_to_char(&text, end) as i32);
            (a, z)
        };
        for t in &a.tokens {
            let tag = match t.kind {
                TokenKind::Command => "command",
                TokenKind::CommandUnknown | TokenKind::Invalid => "error",
                TokenKind::OptionName | TokenKind::ShortFlag => "option",
                TokenKind::OptionValue | TokenKind::Positional => "value",
                TokenKind::ExtraMarker => "dim",
                TokenKind::Extra => continue,
            };
            let (x, y) = range(t.start, t.end);
            b.apply_tag_by_name(tag, &x, &y);
        }
        // 错误加波浪下划线；正在输入的词不标，避免边打字边报错
        for d in a.diagnostics.iter().filter(|d| !d.at_cursor && d.end > d.start) {
            let (x, y) = range(d.start, d.end);
            b.apply_tag_by_name(if d.severity == Severity::Error { "diag-error" } else { "diag-warning" }, &x, &y);
        }
        self.update_ghost();
    }

    /// 灰字补全 / 占位文字：放在文本末尾，与光标同一行
    pub fn update_ghost(&self) {
        let text = self.text();
        let empty = text.is_empty();
        let g = if empty { Some(self.placeholder.borrow().clone()) } else { self.ghost_text.borrow().clone() };
        let show = match &g {
            Some(g) if !g.is_empty() => (empty || self.caret_at_end()) && !self.is_composing(),
            _ => false,
        };
        if !show {
            self.ghost.set_visible(false);
            return;
        }
        let b = self.view.buffer();
        let rect = self.view.iter_location(&b.end_iter());
        let (x, y) = self.view.buffer_to_window_coords(gtk::TextWindowType::Widget, rect.x(), rect.y());
        // 超出可见宽度时不显示（占位文字总是显示）
        let width = self.view.width();
        if !empty && width > 0 && x > width - 8 {
            self.ghost.set_visible(false);
            return;
        }
        self.ghost.set_label(g.as_deref().unwrap_or(""));
        self.ghost.set_margin_start(x.max(0));
        self.ghost.set_margin_top(y.max(0));
        self.ghost.set_visible(true);
    }

    /// 键盘处理（捕获阶段，先于文本框自身）
    pub fn connect_keys(self: &Rc<Self>, handler: impl Fn(gdk::Key, gdk::ModifierType) -> bool + 'static) {
        let ctl = gtk::EventControllerKey::new();
        ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        ctl.connect_key_pressed(move |_, key, _, mods| {
            let Some(f) = weak.upgrade() else { return glib::Propagation::Proceed };
            // 组字期间按键交给输入法
            if f.is_composing() {
                return glib::Propagation::Proceed;
            }
            if handler(key, mods) { glib::Propagation::Stop } else { glib::Propagation::Proceed }
        });
        self.view.add_controller(ctl);
    }
}
