//! 命令面板（PRD 3.2）：唤起、搜索、参数化、预览与插入。
//!
//! 窗口形态（docs/decisions.md D37）：
//! - Wayland：应用不能指定窗口位置，面板窗口最大化且透明，卡片位于屏幕上方约 1/3 处；点击卡片外部关闭。
//! - X11：普通无边框窗口，置顶、不进任务栏，放到鼠标所在屏幕的上方约 1/3 处。
//!
//! 失去焦点时关闭（与 Spotlight 一致）。

mod field;
pub mod model;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use qantrip_core::{Candidate, CandidateKind, HighlightKind, ParamEdit, ParamKind, ParamState, Phase, Preview, Severity};

use qantrip_core::command::CommandOutcome;

use crate::app::App;
use crate::hotkey::Press;
use crate::i18n::l;
use crate::insert::{self, Outcome};
use crate::t;
use crate::theme;
use crate::util;

use field::CommandField;
use model::{Effect, Model};

const CARD_W: i32 = 840;
const CARD_H: i32 = 500;
/// 卡片外边距（阴影）
const SHADOW: i32 = 24;

pub struct Panel {
    app: Rc<App>,
    pub model: RefCell<Model>,
    window: gtk::Window,
    card: gtk::Overlay,
    field: Rc<CommandField>,
    wand: gtk::Image,
    target_chip: gtk::Label,
    list_box: gtk::Box,
    list_scroll: gtk::ScrolledWindow,
    rows: RefCell<Vec<gtk::Widget>>,
    row_keys: RefCell<Vec<String>>,
    list_kind: RefCell<String>,
    card_right: gtk::Box,
    status: gtk::Box,
    footer_keys: gtk::Box,
    /// 预览文字与高亮（D15 闪光）
    preview_view: RefCell<Option<gtk::TextView>>,
    last_template: RefCell<Option<String>>,
    last_highlights: RefCell<Vec<(HighlightKind, u32, u32)>>,
    last_params: RefCell<HashMap<String, String>>,
    flash_source: RefCell<Option<glib::SourceId>>,
    single_click: RefCell<Option<glib::SourceId>>,
    /// 唤起前 Qantrip 自己的活动窗口与其中的焦点控件（插入时直接写入）
    own_target: RefCell<Option<(gtk::Window, Option<gtk::Widget>)>>,
    /// 程序主动隐藏 / 正在插入时不响应失焦
    hiding: Cell<bool>,
    visible: Cell<bool>,
    shown_at: Cell<f64>,
}

fn keycap(key: &str, label: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let k = gtk::Label::new(Some(key));
    k.add_css_class("keycap");
    let t = gtk::Label::new(Some(label));
    t.add_css_class("key-label");
    b.append(&k);
    b.append(&t);
    b
}

fn icon(name: &str, size: i32) -> gtk::Image {
    let i = gtk::Image::from_icon_name(name);
    i.set_pixel_size(size);
    i
}

/// 首屏调用记录的简单着色：命令词、--参数、=取值
fn invocation_markup(s: &str) -> String {
    let p = theme::palette();
    let mut out = String::new();
    for (i, word) in s.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let esc = |t: &str| glib::markup_escape_text(t).to_string();
        if i == 0 {
            out += &format!("<span foreground=\"{}\">{}</span>", p.accent, esc(word));
        } else if word.starts_with('-') {
            if let Some(eq) = word.find('=') {
                out +=
                    &format!("<span foreground=\"{}\">{}</span><span foreground=\"{}\">{}</span>", p.option, esc(&word[..=eq]), p.value, esc(&word[eq + 1..]));
            } else {
                out += &format!("<span foreground=\"{}\">{}</span>", p.option, esc(word));
            }
        } else {
            out += &esc(word);
        }
    }
    out
}

impl Panel {
    pub fn new(app: Rc<App>) -> Rc<Panel> {
        let window = gtk::Window::builder().decorated(false).resizable(false).title("Qantrip").build();
        window.add_css_class("qantrip-panel");
        window.set_hide_on_close(true);

        // 顶栏：魔杖 + 命令行 + 插入目标 + 快捷入口
        let field = CommandField::new();
        let wand = icon("qantrip-wand-symbolic", 20);
        wand.add_css_class("wand");
        let target_chip = gtk::Label::new(None);
        target_chip.add_css_class("target-chip");
        target_chip.set_ellipsize(gtk::pango::EllipsizeMode::End);
        target_chip.set_max_width_chars(18);
        target_chip.set_visible(false);
        let lib_btn = gtk::Button::builder().icon_name("qantrip-library-symbolic").tooltip_text(l("模板库（Ctrl+L）", "Template Library (Ctrl+L)")).build();
        let set_btn = gtk::Button::builder().icon_name("qantrip-settings-symbolic").tooltip_text(l("设置（Ctrl+,）", "Settings (Ctrl+,)")).build();
        for b in [&lib_btn, &set_btn] {
            b.add_css_class("flat");
            b.add_css_class("header-icon");
            b.set_valign(gtk::Align::Center);
            b.set_focusable(false);
        }
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        header.set_margin_start(18);
        header.set_margin_end(14);
        header.set_margin_top(14);
        header.set_margin_bottom(14);
        header.append(&wand);
        header.append(&field.overlay);
        header.append(&target_chip);
        let icons = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        icons.append(&lib_btn);
        icons.append(&set_btn);
        header.append(&icons);

        // 左栏：候选
        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        list_box.add_css_class("cand-list");
        let list_scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&list_box).width_request(300).build();
        list_scroll.set_vexpand(true);

        // 右栏：咒文卡
        let card_right = gtk::Box::new(gtk::Orientation::Vertical, 12);
        card_right.set_margin_start(16);
        card_right.set_margin_end(16);
        card_right.set_margin_top(16);
        card_right.set_margin_bottom(16);
        card_right.set_hexpand(true);

        let middle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        middle.set_vexpand(true);
        middle.append(&list_scroll);
        middle.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        middle.append(&card_right);

        // 底栏
        let status = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        status.set_hexpand(true);
        let footer_keys = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        footer.add_css_class("footer");
        footer.append(&status);
        footer.append(&footer_keys);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&header);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&middle);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&footer);

        let glow = gtk::Box::new(gtk::Orientation::Vertical, 0);
        glow.add_css_class("glow");
        glow.set_valign(gtk::Align::Start);
        glow.set_size_request(-1, 140);
        glow.set_can_target(false);
        let card = gtk::Overlay::new();
        card.add_css_class("qantrip-card");
        card.set_child(Some(&glow));
        card.add_overlay(&content);
        card.set_size_request(CARD_W, CARD_H);
        card.set_overflow(gtk::Overflow::Hidden);

        let engine = app.engine.clone();
        let panel = Rc::new(Panel {
            app: app.clone(),
            model: RefCell::new(Model::new(engine)),
            window,
            card,
            field,
            wand,
            target_chip,
            list_box,
            list_scroll,
            rows: RefCell::new(Vec::new()),
            row_keys: RefCell::new(Vec::new()),
            list_kind: RefCell::new(String::new()),
            card_right,
            status,
            footer_keys,
            preview_view: RefCell::new(None),
            last_template: RefCell::new(None),
            last_highlights: RefCell::new(Vec::new()),
            last_params: RefCell::new(HashMap::new()),
            flash_source: RefCell::new(None),
            single_click: RefCell::new(None),
            own_target: RefCell::new(None),
            hiding: Cell::new(false),
            visible: Cell::new(false),
            shown_at: Cell::new(0.0),
        });
        panel.layout_window();

        let weak = Rc::downgrade(&panel);
        lib_btn.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.open_window(|app| app.show_library(None));
            }
        });
        let weak = Rc::downgrade(&panel);
        set_btn.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.open_window(|app| app.show_settings(None));
            }
        });
        let weak = Rc::downgrade(&panel);
        panel.field.connect_change(move |text| {
            if let Some(p) = weak.upgrade() {
                p.text_changed(&text);
            }
        });
        let weak = Rc::downgrade(&panel);
        panel.field.connect_keys(move |key, mods| weak.upgrade().map(|p| p.key(key, mods)).unwrap_or(false));
        // 点击面板外部 / 切换到其他窗口时关闭
        let weak = Rc::downgrade(&panel);
        panel.window.connect_is_active_notify(move |w| {
            if let Some(p) = weak.upgrade()
                && !w.is_active()
                && p.visible.get()
                && !p.hiding.get()
                && util::uptime() - p.shown_at.get() > 0.25
            {
                p.hide();
            }
        });
        let weak = Rc::downgrade(&panel);
        panel.window.connect_close_request(move |_| {
            if let Some(p) = weak.upgrade() {
                p.hide();
            }
            glib::Propagation::Stop
        });
        panel.update_placeholder();
        panel.refresh(true);
        panel
    }

    /// Wayland：全屏透明窗口中居上放置卡片；X11：窗口即卡片（含阴影边距）
    fn layout_window(self: &Rc<Self>) {
        if self.app.session.is_wayland() {
            let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
            root.set_hexpand(true);
            root.set_vexpand(true);
            self.card.set_halign(gtk::Align::Center);
            self.card.set_valign(gtk::Align::Start);
            root.append(&self.card);
            // 点击卡片以外的区域关闭
            let click = gtk::GestureClick::new();
            let weak = Rc::downgrade(self);
            click.connect_pressed(move |g, _, x, y| {
                let Some(p) = weak.upgrade() else { return };
                if let Some(b) = p.card.compute_bounds(&g.widget().unwrap())
                    && !b.contains_point(&gtk::graphene::Point::new(x as f32, y as f32))
                {
                    p.hide();
                }
            });
            root.add_controller(click);
            // KWin 不会最大化不可调整大小的窗口
            self.window.set_resizable(true);
            self.window.set_child(Some(&root));
        } else {
            self.window.set_default_size(CARD_W + SHADOW * 2, CARD_H + SHADOW * 2);
            self.window.set_child(Some(&self.card));
        }
    }

    fn update_placeholder(&self) {
        self.field.set_placeholder(&l("施放一个戏法…  如 r security 重点检查认证", "Cast a spell…  e.g. r security focus on auth"));
    }

    /// 界面语言变化后刷新文字
    pub fn language_changed(self: &Rc<Self>) {
        self.update_placeholder();
        let q = self.model.borrow().query.clone();
        self.model.borrow_mut().text_did_change(&q);
        self.refresh(true);
    }

    pub fn is_visible(&self) -> bool {
        self.visible.get()
    }

    pub fn toggle(self: &Rc<Self>, press: &Press) {
        if self.visible.get() { self.hide() } else { self.show(press) }
    }

    pub fn show(self: &Rc<Self>, press: &Press) {
        let t1 = util::uptime();
        if self.visible.get() {
            self.window.present();
            self.field.focus();
            return;
        }
        let app = self.app.clone();
        app.sync.panel_opened();
        // 在 Qantrip 自己的窗口里唤起（如引导页的练习框）：记住原来的窗口与焦点控件，插入时直接写入
        let own = app.gapp.active_window().filter(|w| w != &self.window && w.is_active()).map(|w| {
            let focus = gtk::prelude::RootExt::focus(&w);
            (w, focus)
        });
        let target = if own.is_some() { None } else { app.foreground.external() };
        *self.own_target.borrow_mut() = own;
        app.ime.engage();
        app.inserter.begin();
        self.model.borrow_mut().begin(target.clone());
        self.field.set_text("", None);
        match &target {
            Some(t) => {
                self.target_chip.set_label(&format!("→ {}", t.name));
                self.target_chip.set_visible(true);
            }
            None => self.target_chip.set_visible(false),
        }
        self.refresh(true);
        self.hiding.set(false);
        self.visible.set(true);
        self.shown_at.set(util::uptime());

        if let Some(token) = &press.token {
            self.window.set_startup_id(token);
        }
        if app.session.is_wayland() {
            // 全屏窗口在 GNOME 中会被铺上黑色背景，最大化的透明窗口则不会
            self.position_wayland();
            self.window.maximize();
            self.window.present();
        } else {
            WidgetExt::realize(&self.window);
            self.position_x11(press.timestamp);
            #[allow(deprecated)]
            self.window.present_with_time(press.timestamp);
            let weak = Rc::downgrade(self);
            let ts = press.timestamp;
            glib::idle_add_local_once(move || {
                if let Some(p) = weak.upgrade() {
                    p.position_x11(ts);
                }
            });
        }
        self.field.focus();
        theme::retrigger(&self.card, "entering");
        let t2 = util::uptime();
        let weak = Rc::downgrade(self);
        let source = press.source;
        glib::idle_add_local_once(move || {
            let Some(p) = weak.upgrade() else { return };
            crate::log::event(
                "show",
                serde_json::json!({
                    "source": source,
                    "target": p.model.borrow().target.as_ref().map(|t| t.id.clone()).unwrap_or_default(),
                    "handler_to_shown_ms": util::ms(t2 - t1),
                    "handler_to_idle_ms": util::ms(util::uptime() - t1),
                    "is_active": p.window.is_active(),
                }),
            );
        });
    }

    fn position_wayland(&self) {
        // 卡片中心位于屏幕高度约 32% 处（与 macOS 相同）
        let h =
            self.window.upcast_ref::<gtk::Widget>().display().monitors().item(0).and_downcast::<gdk::Monitor>().map(|m| m.geometry().height()).unwrap_or(1080);
        let top = ((h as f64) * 0.32 - (CARD_H as f64) / 2.0 - SHADOW as f64).max(0.0) as i32;
        self.card.set_margin_top(top);
    }

    fn position_x11(&self, time: u32) {
        let Some(x) = &self.app.x11 else { return };
        let Some(surface) = self.window.surface() else { return };
        let Some(xs) = surface.downcast_ref::<gdk4_x11::X11Surface>() else { return };
        let xid = xs.xid() as u32;
        let display = WidgetExt::display(&self.window);
        let (px, py) = x.pointer().unwrap_or((0, 0));
        let monitors = display.monitors();
        let mut geo = None;
        for i in 0..monitors.n_items() {
            if let Some(m) = monitors.item(i).and_downcast::<gdk::Monitor>() {
                let g = m.geometry();
                if geo.is_none() || (px >= g.x() && px < g.x() + g.width() && py >= g.y() && py < g.y() + g.height()) {
                    geo = Some(g);
                }
            }
        }
        let Some(g) = geo else { return };
        let w = CARD_W + SHADOW * 2;
        let h = CARD_H + SHADOW * 2;
        let left = g.x() + (g.width() - w) / 2;
        let top = g.y() + ((g.height() as f64) * 0.32) as i32 - h / 2;
        x.place_panel(xid, left, top.max(g.y()));
        if time != 0 {
            x.activate(xid, time);
        }
    }

    pub fn hide(self: &Rc<Self>) {
        if !self.visible.get() {
            return;
        }
        self.hiding.set(true);
        self.visible.set(false);
        self.window.set_visible(false);
        self.app.ime.restore();
        // 在自己的窗口里唤起时，关闭面板后把键盘焦点还给那个窗口
        let own = self.own_target.borrow().clone();
        if let Some((w, focus)) = own {
            w.present();
            if let Some(f) = focus {
                f.grab_focus();
            }
        }
        self.hiding.set(false);
    }

    /// 关闭面板后打开 Qantrip 的窗口（不把焦点还给原应用）
    fn open_window(self: &Rc<Self>, open: impl FnOnce(&Rc<App>) + 'static) {
        *self.own_target.borrow_mut() = None;
        self.hide();
        let app = self.app.clone();
        glib::idle_add_local_once(move || open(&app));
    }

    // MARK: 输入

    fn text_changed(self: &Rc<Self>, text: &str) {
        let mut m = self.model.borrow_mut();
        m.text_did_change(text);
        let wants = m.wants_clipboard();
        if wants {
            m.mark_clipboard_requested();
        }
        drop(m);
        self.refresh(false);
        if wants {
            // `/new 名称` 不写正文时使用剪贴板文字（只读取一次）
            if let Some(d) = gdk::Display::default() {
                let weak = Rc::downgrade(self);
                d.clipboard().read_text_async(gtk::gio::Cancellable::NONE, move |r| {
                    if let Some(p) = weak.upgrade() {
                        let text = r.ok().flatten().map(|s| s.to_string());
                        p.model.borrow_mut().set_clipboard_text(text);
                        p.refresh(false);
                    }
                });
            }
        }
    }

    fn key(self: &Rc<Self>, key: gdk::Key, mods: gdk::ModifierType) -> bool {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let alt = mods.contains(gdk::ModifierType::ALT_MASK);
        match key {
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                self.confirm(ctrl);
                true
            }
            gdk::Key::Tab | gdk::Key::ISO_Left_Tab | gdk::Key::KP_Tab => {
                let e = self.model.borrow_mut().complete();
                self.apply_effect(e);
                true
            }
            gdk::Key::Up | gdk::Key::KP_Up => {
                self.model.borrow_mut().move_selection(-1);
                self.refresh(false);
                true
            }
            gdk::Key::Down | gdk::Key::KP_Down => {
                self.model.borrow_mut().move_selection(1);
                self.refresh(false);
                true
            }
            gdk::Key::Escape => {
                self.hide();
                true
            }
            gdk::Key::Right | gdk::Key::End | gdk::Key::KP_Right if !ctrl && !alt => {
                // 光标在末尾时 → 接受灰字补全（类似 fish shell）
                if self.field.caret_at_end() {
                    let e = self.model.borrow_mut().accept_ghost();
                    if !matches!(e, Effect::None) {
                        self.apply_effect(e);
                        return true;
                    }
                }
                false
            }
            gdk::Key::l | gdk::Key::L if ctrl && !alt => {
                self.open_window(|app| app.show_library(None));
                true
            }
            gdk::Key::comma if ctrl && !alt => {
                self.open_window(|app| app.show_settings(None));
                true
            }
            _ => false,
        }
    }

    fn confirm(self: &Rc<Self>, expand: bool) {
        let record = self.app.settings.record_usage();
        let e = self.model.borrow_mut().confirm(expand, record);
        self.apply_effect(e);
    }

    fn apply_effect(self: &Rc<Self>, e: Effect) {
        match e {
            Effect::None => self.refresh(false),
            Effect::SetInput { text, caret } => {
                self.field.set_text(&text, Some(caret));
                self.refresh(false);
                self.field.focus();
            }
            Effect::Submit { text, expand } => self.submit(text, expand),
            Effect::Command(outcome) => self.finish_command(outcome),
            Effect::Reject => {
                theme::retrigger(&self.card, "shake");
                theme::retrigger(&self.wand, "bounce");
                self.refresh(false);
            }
        }
    }

    fn submit(self: &Rc<Self>, text: String, expand: bool) {
        let own = self.own_target.borrow_mut().take();
        if let Some((w, focus)) = own {
            self.hide();
            if !insert::insert_into_own(&w, focus, &text) {
                insert::copy_plain(&text);
                crate::notify::hud(&l("焦点不在输入框中，已复制到剪贴板", "No text field is focused. Copied to clipboard"));
            }
            crate::log::event("insert_own", serde_json::json!({"chars": text.chars().count()}));
            return;
        }
        let target = self.model.borrow().target.clone();
        let inserter = self.app.inserter.clone();
        let combo = inserter.combo_for(target.as_ref());
        let weak = Rc::downgrade(self);
        self.hiding.set(true);
        inserter.insert(
            text,
            target,
            expand,
            move || {
                if let Some(p) = weak.upgrade() {
                    p.hide();
                }
            },
            move |outcome| {
                if let Outcome::Copied { reason } = outcome {
                    crate::notify::hud(&insert::copied_message(&reason, combo));
                }
            },
        );
    }

    /// 指令完成：关闭面板，删除的文件移到回收站，需要时打开模板库，最后提示（D31）。
    fn finish_command(self: &Rc<Self>, outcome: CommandOutcome) {
        self.hide();
        let app = self.app.clone();
        if let Some(path) = &outcome.trash_path {
            crate::library::trash(std::path::Path::new(path));
            app.engine.reload();
        }
        app.library_reload();
        if let Some(name) = &outcome.open_editor {
            app.show_library(Some(name.clone()));
        }
        let mut message = outcome.message.clone();
        match outcome.action.as_deref().unwrap_or("") {
            "sync" => {
                if app.sync.enrolled() {
                    app.sync.sync_now();
                    message = l("正在同步…", "Syncing…");
                } else {
                    app.show_settings(Some("sync"));
                }
            }
            "settings" => app.show_settings(None),
            "library" => app.show_library(None),
            a if a.starts_with("lang:") => {
                app.settings.set_string("templateLanguage", Some(&a[5..]));
            }
            _ => {}
        }
        if !message.is_empty() {
            crate::notify::hud(&message);
        }
    }

    // MARK: 界面刷新

    /// `full`：重建候选列表（唤起、语言变化）
    pub fn refresh(self: &Rc<Self>, full: bool) {
        let m = self.model.borrow();
        let a = m.analysis.clone();
        let kind = m.list_kind();
        let selected = m.selected_index;
        let armed = m.armed_command.is_some();
        let command_mode = m.is_command_mode();
        let first_error = m.first_error().cloned();
        drop(m);

        self.field.apply(&a);
        if a.locked {
            self.card.add_css_class("locked")
        } else {
            self.card.remove_css_class("locked")
        }
        // 命令锁定时魔杖弹跳
        let locked_name = if a.locked { a.template.as_ref().map(|t| t.name.clone()) } else { None };
        if locked_name.is_some() && *self.last_template.borrow() != locked_name && !full {
            theme::retrigger(&self.wand, "bounce");
        }

        // 左栏
        let keys: Vec<String> = a.candidates.iter().map(|c| format!("{}|{:?}|{}|{}", c.section, c.kind, c.key, c.active)).collect();
        let kind_changed = *self.list_kind.borrow() != kind;
        if full || kind_changed || *self.row_keys.borrow() != keys {
            self.rebuild_list(&a.candidates, a.phase, full, kind_changed);
            *self.row_keys.borrow_mut() = keys;
            *self.list_kind.borrow_mut() = kind;
        }
        for (i, r) in self.rows.borrow().iter().enumerate() {
            if i == selected { r.add_css_class("selected") } else { r.remove_css_class("selected") }
        }
        self.scroll_to_selected();

        // 右栏
        self.rebuild_card(&a, command_mode);

        // 底栏
        while let Some(c) = self.status.first_child() {
            self.status.remove(&c);
        }
        let status_label = |text: &str, class: &str, icon_name: Option<&str>| {
            if let Some(n) = icon_name {
                let i = icon(n, 14);
                i.add_css_class("status");
                i.add_css_class(class);
                self.status.append(&i);
            }
            let lbl = gtk::Label::new(Some(text));
            lbl.add_css_class("status");
            lbl.add_css_class(class);
            lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            lbl.set_xalign(0.0);
            lbl.set_hexpand(true);
            self.status.append(&lbl);
        };
        if armed {
            status_label(&l("再按一次 ↩ 确认", "Press ↩ again to confirm"), "warning", Some("dialog-warning-symbolic"));
        } else if let Some(e) = first_error {
            status_label(&e.message, if e.at_cursor { "dim" } else { "error" }, Some("dialog-error-symbolic"));
        } else if let Some(w) = a.diagnostics.iter().find(|d| d.severity == Severity::Warning) {
            status_label(&w.message, "warning", Some("dialog-warning-symbolic"));
        } else if let (Some(c), true) = (&a.canonical, a.locked) {
            status_label(c, "canonical", None);
            if let Some(l) = self.status.last_child().and_downcast::<gtk::Label>() {
                l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            }
        }
        while let Some(c) = self.footer_keys.first_child() {
            self.footer_keys.remove(&c);
        }
        let expand_enabled = self.app.settings.expand_paste_enabled();
        let key_button = |key: &str, label: &str, f: Box<dyn Fn(&Rc<Panel>)>| {
            let b = gtk::Button::new();
            b.set_child(Some(&keycap(key, label)));
            b.add_css_class("key-button");
            b.set_focusable(false);
            let weak = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    f(&p);
                }
            });
            self.footer_keys.append(&b);
        };
        key_button("↩", &if command_mode { l("执行", "Run") } else { l("插入", "Insert") }, Box::new(|p| p.confirm(false)));
        if expand_enabled && !command_mode {
            key_button("Ctrl+↩", &l("展开粘贴", "Expand paste"), Box::new(|p| p.confirm(true)));
        }
        let tab = keycap("Tab", &l("补全", "Complete"));
        tab.add_css_class("key-button");
        self.footer_keys.append(&tab);
        let arrows = keycap("↑↓", &l("选择", "Select"));
        arrows.add_css_class("key-button");
        self.footer_keys.append(&arrows);
        key_button("Esc", &l("关闭", "Close"), Box::new(|p| p.hide()));
    }

    fn rebuild_list(self: &Rc<Self>, candidates: &[Candidate], phase: Phase, entering: bool, kind_changed: bool) {
        while let Some(c) = self.list_box.first_child() {
            self.list_box.remove(&c);
        }
        let mut rows = Vec::new();
        if candidates.is_empty() {
            let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
            b.set_vexpand(true);
            b.set_valign(gtk::Align::Center);
            let extra = phase == Phase::Extra;
            let i = icon(if extra { "document-edit-symbolic" } else { "qantrip-wand-symbolic" }, 22);
            i.set_opacity(0.35);
            b.append(&i);
            let t = gtk::Label::new(Some(&if extra {
                l("正在输入附加要求", "Entering extra instructions")
            } else {
                l("没有可补全的内容", "Nothing to complete")
            }));
            t.add_css_class("empty-hint");
            b.append(&t);
            b.set_margin_top(120);
            self.list_box.append(&b);
            *self.rows.borrow_mut() = rows;
            return;
        }
        let try_section = |s: &str| s == "试试看" || s == "Try these";
        let mut section: Option<String> = None;
        let mut n = 0usize;
        for (i, c) in candidates.iter().enumerate() {
            if !c.section.is_empty() && section.as_deref() != Some(c.section.as_str()) {
                let h = gtk::Label::new(Some(&c.section));
                h.add_css_class("section-header");
                if section.is_none() {
                    h.add_css_class("first");
                }
                h.set_xalign(0.0);
                self.list_box.append(&h);
                section = Some(c.section.clone());
                n += 1;
            }
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.add_css_class("cand-row");
            if c.kind == CandidateKind::Invocation {
                let ic = icon(if try_section(&c.section) { "qantrip-lightbulb-symbolic" } else { "qantrip-history-symbolic" }, 12);
                ic.set_opacity(0.45);
                row.append(&ic);
                let t = gtk::Label::new(None);
                t.set_markup(&invocation_markup(&c.title));
                t.add_css_class("cand-title");
                t.set_ellipsize(gtk::pango::EllipsizeMode::End);
                t.set_xalign(0.0);
                t.set_hexpand(true);
                row.append(&t);
            } else {
                let t = gtk::Label::new(Some(&c.title));
                t.add_css_class("cand-title");
                t.add_css_class(match c.kind {
                    CandidateKind::Template => "template",
                    CandidateKind::Param => "param",
                    _ => "choice",
                });
                t.set_xalign(0.0);
                row.append(&t);
                let sub = gtk::Label::new(Some(&c.subtitle));
                sub.add_css_class("cand-sub");
                sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
                sub.set_xalign(0.0);
                sub.set_hexpand(true);
                sub.set_visible(!c.subtitle.is_empty());
                row.append(&sub);
                if c.subtitle.is_empty() {
                    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                    spacer.set_hexpand(true);
                    row.append(&spacer);
                }
            }
            if c.active {
                let ck = icon("object-select-symbolic", 12);
                ck.add_css_class("cand-check");
                row.append(&ck);
            }
            if c.kind == CandidateKind::Template && !c.detail.is_empty() {
                let d = gtk::Label::new(Some(&c.detail));
                d.add_css_class("cand-detail");
                d.set_ellipsize(gtk::pango::EllipsizeMode::End);
                d.set_max_width_chars(14);
                row.append(&d);
            }
            if !c.detail.is_empty() {
                row.set_tooltip_text(Some(&c.detail));
            }
            if entering && theme::motion() {
                row.add_css_class("enter");
                row.add_css_class(&format!("d{}", n.min(12)));
            }
            let click = gtk::GestureClick::new();
            let weak = Rc::downgrade(self);
            click.connect_pressed(move |_, n_press, _, _| {
                let Some(p) = weak.upgrade() else { return };
                if let Some(id) = p.single_click.borrow_mut().take() {
                    id.remove();
                }
                if n_press >= 2 {
                    let record = p.app.settings.record_usage();
                    let e = p.model.borrow_mut().double_click(i, record);
                    p.apply_effect(e);
                } else {
                    // 等待可能的第二次点击（双击 = 直接插入）
                    let weak2 = Rc::downgrade(&p);
                    let id = glib::timeout_add_local_once(Duration::from_millis(220), move || {
                        if let Some(p) = weak2.upgrade() {
                            p.single_click.borrow_mut().take();
                            let e = p.model.borrow_mut().click(i);
                            p.apply_effect(e);
                            p.field.focus();
                        }
                    });
                    *p.single_click.borrow_mut() = Some(id);
                }
            });
            row.add_controller(click);
            self.list_box.append(&row);
            rows.push(row.upcast::<gtk::Widget>());
            n += 1;
        }
        if kind_changed && !entering {
            theme::retrigger(&self.list_box, "fade");
        }
        *self.rows.borrow_mut() = rows;
    }

    fn scroll_to_selected(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            let Some(p) = weak.upgrade() else { return };
            let idx = p.model.borrow().selected_index;
            let rows = p.rows.borrow();
            let Some(row) = rows.get(idx) else { return };
            let Some(b) = row.compute_bounds(&p.list_box) else { return };
            let adj = p.list_scroll.vadjustment();
            let (top, bottom) = (b.y() as f64, (b.y() + b.height()) as f64);
            if top < adj.value() {
                adj.set_value((top - 8.0).max(0.0));
            } else if bottom > adj.value() + adj.page_size() {
                adj.set_value(bottom - adj.page_size() + 8.0);
            }
        });
    }

    fn rebuild_card(self: &Rc<Self>, a: &qantrip_core::Analysis, command_mode: bool) {
        while let Some(c) = self.card_right.first_child() {
            self.card_right.remove(&c);
        }
        let prev_template = self.last_template.borrow().clone();
        let current = a.template.as_ref().map(|t| t.name.clone());
        *self.last_template.borrow_mut() = current.clone();
        *self.preview_view.borrow_mut() = None;

        if let Some(t) = &a.template {
            let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let name = gtk::Label::new(Some(&t.name));
            name.add_css_class("spell-name");
            if prev_template.as_deref() != Some(t.name.as_str()) && theme::motion() {
                name.add_css_class("slide");
            }
            head.append(&name);
            let desc = gtk::Label::new(Some(&t.description));
            desc.add_css_class("spell-desc");
            desc.set_ellipsize(gtk::pango::EllipsizeMode::End);
            desc.set_xalign(0.0);
            desc.set_hexpand(true);
            desc.set_valign(gtk::Align::Baseline);
            name.set_valign(gtk::Align::Baseline);
            head.append(&desc);
            if !a.hint.is_empty() {
                let hint = gtk::Label::new(Some(&a.hint));
                hint.add_css_class("spell-hint");
                hint.set_valign(gtk::Align::Baseline);
                head.append(&hint);
            }
            self.card_right.append(&head);

            if !a.params.is_empty() {
                let flow = gtk::FlowBox::new();
                flow.set_selection_mode(gtk::SelectionMode::None);
                flow.set_column_spacing(6);
                flow.set_row_spacing(6);
                flow.set_max_children_per_line(20);
                flow.set_homogeneous(false);
                let prev = self.last_params.borrow().clone();
                let same_template = prev_template == current;
                let mut now = HashMap::new();
                for p in &a.params {
                    let chip = self.param_chip(p);
                    if same_template && prev.get(&p.name).is_some_and(|d| d != &p.display) && theme::motion() {
                        chip.add_css_class("bounce");
                    }
                    now.insert(p.name.clone(), p.display.clone());
                    let child = gtk::FlowBoxChild::new();
                    child.set_child(Some(&chip));
                    child.set_focusable(false);
                    flow.insert(&child, -1);
                }
                *self.last_params.borrow_mut() = now;
                self.card_right.append(&flow);
            } else {
                self.last_params.borrow_mut().clear();
            }
            if let Some(p) = &a.preview {
                self.preview_box(p, prev_template == current);
            }
        } else if let (true, Some(preview)) = (command_mode, a.preview.as_ref()) {
            // 指令模式：还没确定指令时显示用法说明
            let title = gtk::Label::new(Some(&l("指令", "Commands")));
            title.add_css_class("spell-name");
            title.set_xalign(0.0);
            self.card_right.append(&title);
            let help = gtk::Label::new(Some(&preview.text));
            help.add_css_class("commands-help");
            help.set_xalign(0.0);
            help.set_wrap(true);
            help.set_selectable(false);
            self.card_right.append(&help);
            self.last_highlights.borrow_mut().clear();
        } else {
            let empty = gtk::Label::new(Some(&l("没有匹配的模板", "No matching templates")));
            empty.add_css_class("empty-hint");
            empty.set_vexpand(true);
            self.card_right.append(&empty);
            self.last_highlights.borrow_mut().clear();
        }
    }

    /// 参数标签：显示当前取值，点击可修改（D16）
    fn param_chip(self: &Rc<Self>, p: &ParamState) -> gtk::Widget {
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let name = gtk::Label::new(Some(&p.name));
        name.add_css_class("chip-name");
        let value = gtk::Label::new(Some(&p.display));
        value.add_css_class("chip-value");
        content.append(&name);
        content.append(&value);
        let name_s = p.name.clone();
        let widget: gtk::Widget = match p.kind {
            ParamKind::Enum | ParamKind::Multi => {
                let mb = gtk::MenuButton::new();
                mb.set_child(Some(&content));
                mb.set_always_show_arrow(false);
                let pop = gtk::Popover::new();
                let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
                for c in &p.choices {
                    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                    let check = icon("object-select-symbolic", 12);
                    check.set_opacity(if c.on { 1.0 } else { 0.0 });
                    row.append(&check);
                    row.append(&gtk::Label::new(Some(&c.label)));
                    let v = gtk::Label::new(Some(&c.value));
                    v.add_css_class("dim-label");
                    v.add_css_class("mono");
                    row.append(&v);
                    let b = gtk::Button::new();
                    b.set_child(Some(&row));
                    b.add_css_class("flat");
                    let weak = Rc::downgrade(self);
                    let multi = p.kind == ParamKind::Multi;
                    let value = c.value.clone();
                    let n = name_s.clone();
                    let pop2 = pop.clone();
                    b.connect_clicked(move |_| {
                        pop2.popdown();
                        if let Some(panel) = weak.upgrade() {
                            let edit = if multi { ParamEdit::ToggleChoice { value: value.clone() } } else { ParamEdit::Set { value: value.clone() } };
                            let e = panel.model.borrow_mut().edit_param(&n, edit);
                            panel.apply_effect(e);
                        }
                    });
                    list.append(&b);
                }
                if p.is_set {
                    list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                    let b = gtk::Button::with_label(&l("恢复默认", "Restore default"));
                    b.add_css_class("flat");
                    let weak = Rc::downgrade(self);
                    let n = name_s.clone();
                    let pop2 = pop.clone();
                    b.connect_clicked(move |_| {
                        pop2.popdown();
                        if let Some(panel) = weak.upgrade() {
                            let e = panel.model.borrow_mut().edit_param(&n, ParamEdit::Clear);
                            panel.apply_effect(e);
                        }
                    });
                    list.append(&b);
                }
                pop.set_child(Some(&list));
                mb.set_popover(Some(&pop));
                mb.upcast()
            }
            _ => {
                let b = gtk::Button::new();
                b.set_child(Some(&content));
                let weak = Rc::downgrade(self);
                let toggle = p.kind == ParamKind::Bool;
                b.connect_clicked(move |_| {
                    if let Some(panel) = weak.upgrade() {
                        let e = panel.model.borrow_mut().edit_param(&name_s, if toggle { ParamEdit::Toggle } else { ParamEdit::Begin });
                        panel.apply_effect(e);
                    }
                });
                b.upcast()
            }
        };
        widget.add_css_class("chip");
        widget.set_focusable(false);
        if p.is_set {
            widget.add_css_class("set");
        }
        if !p.description.is_empty() {
            widget.set_tooltip_text(Some(&p.description));
        }
        widget
    }

    fn preview_box(self: &Rc<Self>, p: &Preview, same_template: bool) {
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::new(Some(&l("预览", "Preview")));
        title.add_css_class("preview-title");
        head.append(&title);
        let (chars, lines) = (p.chars, p.lines);
        let meta = gtk::Label::new(Some(&t!("{chars} 字 · {lines} 行", "{chars} chars · {lines} lines")));
        meta.add_css_class("preview-meta");
        meta.set_hexpand(true);
        meta.set_xalign(0.0);
        head.append(&meta);
        if p.collapses_in_claude_code && self.app.settings.expand_paste_enabled() {
            let note = gtk::Label::new(Some(&l("Claude Code 中会折叠 · Ctrl+↩ 展开粘贴", "Collapsed in Claude Code · Ctrl+↩ to expand")));
            note.add_css_class("preview-meta");
            head.append(&note);
        }
        self.card_right.append(&head);

        let view = gtk::TextView::builder().editable(false).cursor_visible(false).wrap_mode(gtk::WrapMode::WordChar).build();
        view.add_css_class("preview-text");
        view.set_left_margin(12);
        view.set_right_margin(12);
        view.set_top_margin(12);
        view.set_bottom_margin(12);
        view.set_pixels_inside_wrap(3);
        view.set_focusable(false);
        let buf = view.buffer();
        buf.set_text(&p.text);
        let pal = theme::palette();
        let mk = |name: &str, color: &str, alpha: f32| {
            let mut c = theme::rgba(color);
            c.set_alpha(alpha);
            let tag = gtk::TextTag::builder().name(name).background_rgba(&c).build();
            buf.tag_table().add(&tag);
            tag
        };
        mk("param", pal.accent, 0.26);
        mk("extra", pal.value, 0.26);
        let flash = mk("flash", pal.accent, 0.76);
        let mut now = Vec::new();
        for h in &p.highlights {
            let a = buf.iter_at_offset(util::u16_to_char(&p.text, h.start) as i32);
            let b = buf.iter_at_offset(util::u16_to_char(&p.text, h.end) as i32);
            buf.apply_tag_by_name(if h.kind == HighlightKind::Extra { "extra" } else { "param" }, &a, &b);
            now.push((h.kind, h.start, h.end));
        }
        // 同一模板下新出现的高亮片段先亮起，再淡成常驻高亮（D15）
        let before = self.last_highlights.borrow().clone();
        let fresh: Vec<_> = now.iter().filter(|h| !before.contains(h)).cloned().collect();
        *self.last_highlights.borrow_mut() = now;
        if let Some(id) = self.flash_source.borrow_mut().take() {
            id.remove();
        }
        if same_template && !fresh.is_empty() && theme::motion() {
            for (_, s, e) in &fresh {
                let a = buf.iter_at_offset(util::u16_to_char(&p.text, *s) as i32);
                let b = buf.iter_at_offset(util::u16_to_char(&p.text, *e) as i32);
                buf.apply_tag(&flash, &a, &b);
            }
            let start = util::uptime();
            let base = theme::rgba(pal.accent);
            let weak = Rc::downgrade(self);
            let id = glib::timeout_add_local(Duration::from_millis(16), move || {
                let t = (util::uptime() - start) / 0.45;
                let level = (1.0 - t * t).max(0.0) as f32;
                let mut c = base;
                c.set_alpha(0.26 + 0.5 * level);
                flash.set_background_rgba(Some(&c));
                if t >= 1.0 {
                    if let Some(p) = weak.upgrade() {
                        p.flash_source.borrow_mut().take();
                    }
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
            *self.flash_source.borrow_mut() = Some(id);
        }
        let scroll = gtk::ScrolledWindow::builder().child(&view).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        scroll.add_css_class("preview-box");
        scroll.set_overflow(gtk::Overflow::Hidden);
        self.card_right.append(&scroll);
        *self.preview_view.borrow_mut() = Some(view);
    }

    // MARK: 调试

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn debug_type(self: &Rc<Self>, text: &str, down: usize) {
        if !self.visible.get() {
            self.show(&Press { source: "debug", timestamp: 0, token: None });
        }
        if !text.is_empty() {
            self.field.set_text(text, None);
            self.text_changed(text);
        }
        for _ in 0..down {
            self.model.borrow_mut().move_selection(1);
        }
        self.refresh(false);
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn debug_key(self: &Rc<Self>, name: &str) {
        match name {
            "enter" => self.confirm(false),
            "ctrl-enter" => self.confirm(true),
            "tab" => {
                let e = self.model.borrow_mut().complete();
                self.apply_effect(e);
            }
            "down" => {
                self.model.borrow_mut().move_selection(1);
                self.refresh(false);
            }
            "up" => {
                self.model.borrow_mut().move_selection(-1);
                self.refresh(false);
            }
            "esc" => self.hide(),
            _ => {}
        }
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn debug_param(self: &Rc<Self>, name: &str, op: &str) {
        let edit = match op {
            "toggle" => ParamEdit::Toggle,
            "clear" => ParamEdit::Clear,
            "begin" => ParamEdit::Begin,
            o if o.starts_with("set:") => ParamEdit::Set { value: o[4..].to_string() },
            o if o.starts_with("choice:") => ParamEdit::ToggleChoice { value: o[7..].to_string() },
            _ => return,
        };
        let e = self.model.borrow_mut().edit_param(name, edit);
        self.apply_effect(e);
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn window(&self) -> &gtk::Window {
        &self.window
    }
}
