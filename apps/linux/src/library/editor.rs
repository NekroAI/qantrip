//! 单个模板的编辑器：表单与源码互相同步（表单修改 → 重新生成源码；源码修改 → 能解析时更新表单），
//! 右侧为试用与实时预览。frontmatter 由表单重新生成，手写的 YAML 注释不会保留（D19）。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use qantrip_core::{Choice, ParamDoc, ParamKind, Severity, SourceCheck, TemplateDoc, TemplateEntry};

use crate::app::App;
use crate::i18n::l;
use crate::t;

use super::LibraryWindow;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Form,
    Source,
}

const KINDS: [ParamKind; 6] = [ParamKind::Bool, ParamKind::Enum, ParamKind::Multi, ParamKind::String, ParamKind::Number, ParamKind::Text];

fn kind_label(k: ParamKind) -> String {
    match k {
        ParamKind::Bool => l("开关", "Switch"),
        ParamKind::Enum => l("单选", "Single choice"),
        ParamKind::Multi => l("多选", "Multiple choice"),
        ParamKind::String => l("文本", "Text"),
        ParamKind::Number => l("数字", "Number"),
        ParamKind::Text => l("多行文本", "Multiline text"),
    }
}

fn split_list(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

/// 展开行中的控件；内置模板只读展示
fn add_row(row: &adw::ExpanderRow, ro: bool, w: &impl IsA<gtk::Widget>) {
    if ro {
        w.set_sensitive(false);
    }
    row.add_row(w);
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { v.to_string() }
}

pub struct Editor {
    app: Rc<App>,
    lib: Weak<LibraryWindow>,
    entry: TemplateEntry,
    source: RefCell<String>,
    doc: RefCell<Option<TemplateDoc>>,
    check: RefCell<SourceCheck>,
    try_input: RefCell<String>,
    dirty: Cell<bool>,
    mode: Cell<Mode>,
    save_error: RefCell<Option<String>>,
    expanded: Cell<Option<usize>>,
    /// 程序填充控件时不把变化写回文档
    filling: Cell<bool>,
    root: gtk::Box,
    stack: gtk::Stack,
    form_scroll: gtk::ScrolledWindow,
    source_view: gtk::TextView,
    try_entry: gtk::Entry,
    try_content: gtk::Box,
    save_btn: gtk::Button,
    form_btn: gtk::ToggleButton,
    source_btn: gtk::ToggleButton,
}

impl Editor {
    pub fn new(app: Rc<App>, lib: Weak<LibraryWindow>, entry: TemplateEntry) -> Rc<Editor> {
        let source = entry.source.clone();
        let doc = qantrip_core::parse_template_doc(source.clone()).ok();
        let check = app.engine.check_source(source.clone(), String::new());

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        let form_scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
        stack.add_named(&form_scroll, Some("form"));
        let source_view = gtk::TextView::builder()
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(12)
            .bottom_margin(12)
            .left_margin(12)
            .right_margin(12)
            .build();
        source_view.add_css_class("code-view");
        let source_scroll = gtk::ScrolledWindow::builder().child(&source_view).vexpand(true).build();
        stack.add_named(&source_scroll, Some("source"));

        // 试用面板
        let try_title = gtk::Label::new(Some(&l("试用", "Try it")));
        try_title.add_css_class("caption-heading");
        try_title.add_css_class("dim-label");
        try_title.set_xalign(0.0);
        let try_entry = gtk::Entry::new();
        try_entry.add_css_class("try-input");
        let try_content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let try_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        try_box.set_margin_start(14);
        try_box.set_margin_end(14);
        try_box.set_margin_top(14);
        try_box.set_margin_bottom(14);
        try_box.append(&try_title);
        try_box.append(&try_entry);
        try_box.append(&try_content);
        let try_scroll = gtk::ScrolledWindow::builder().child(&try_box).hscrollbar_policy(gtk::PolicyType::Never).width_request(380).build();

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_start_child(Some(&stack));
        paned.set_end_child(Some(&try_scroll));
        paned.set_resize_end_child(false);
        paned.set_shrink_end_child(false);
        paned.set_shrink_start_child(false);
        paned.set_vexpand(true);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&paned);

        let form_btn = gtk::ToggleButton::builder().label(l("表单", "Form")).active(true).build();
        let source_btn = gtk::ToggleButton::builder().label(l("源码", "Source")).group(&form_btn).build();
        let save_btn = gtk::Button::builder().label(l("保存", "Save")).tooltip_text(l("保存（Ctrl+S）", "Save (Ctrl+S)")).build();
        save_btn.add_css_class("suggested-action");

        let e = Rc::new(Editor {
            app,
            lib,
            entry,
            source: RefCell::new(source),
            doc: RefCell::new(doc),
            check: RefCell::new(check),
            try_input: RefCell::new(String::new()),
            dirty: Cell::new(false),
            mode: Cell::new(Mode::Form),
            save_error: RefCell::new(None),
            expanded: Cell::new(None),
            filling: Cell::new(false),
            root,
            stack,
            form_scroll,
            source_view,
            try_entry,
            try_content,
            save_btn,
            form_btn,
            source_btn,
        });
        e.try_entry.set_placeholder_text(Some(&{
            let n = e.check.borrow().name.clone().unwrap_or_else(|| "r".into());
            t!("输入命令试用，如 {n} --strict", "Try a command, e.g. {n} --strict")
        }));

        // 只读提示条（内置 / 已删除）
        if e.read_only() {
            let bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            bar.add_css_class("banner-bar");
            let msg = gtk::Label::new(None);
            msg.set_wrap(true);
            msg.set_xalign(0.0);
            msg.set_hexpand(true);
            bar.append(&gtk::Image::from_icon_name(if e.entry.deleted { "user-trash-symbolic" } else { "changes-prevent-symbolic" }));
            bar.append(&msg);
            if e.entry.deleted {
                bar.add_css_class("deleted");
                msg.set_label(&l(
                    "这个内置模板已删除，它的名称和别名可以给你自己的模板使用。",
                    "This built-in template is deleted; its name and aliases are free for your own templates.",
                ));
                let b = gtk::Button::with_label(&l("恢复", "Restore"));
                let weak = Rc::downgrade(&e);
                b.connect_clicked(move |_| {
                    if let Some(e) = weak.upgrade()
                        && let Some(lib) = e.lib.upgrade()
                    {
                        lib.restore(&e.entry);
                    }
                });
                bar.append(&b);
            } else {
                msg.set_label(&l(
                    "内置模板为只读。自定义后会保存为你自己的模板并覆盖内置版本，随时可以恢复。",
                    "Built-in templates are read-only. Customizing saves your own copy that overrides the built-in version; you can restore it anytime.",
                ));
                let c = gtk::Button::with_label(&l("自定义此模板", "Customize This Template"));
                c.add_css_class("suggested-action");
                let d = gtk::Button::with_label(&l("复制为新模板", "Duplicate as New Template"));
                let weak = Rc::downgrade(&e);
                c.connect_clicked(move |_| {
                    if let Some(e) = weak.upgrade()
                        && let Some(lib) = e.lib.upgrade()
                    {
                        lib.customize(&e.entry);
                    }
                });
                let weak = Rc::downgrade(&e);
                d.connect_clicked(move |_| {
                    if let Some(e) = weak.upgrade()
                        && let Some(lib) = e.lib.upgrade()
                    {
                        lib.duplicate(&e.entry);
                    }
                });
                bar.append(&c);
                bar.append(&d);
            }
            e.root.append(&bar);
        }

        e.source_view.buffer().set_text(&e.source.borrow());
        e.source_view.set_editable(!e.read_only());
        let weak = Rc::downgrade(&e);
        e.source_view.buffer().connect_changed(move |b| {
            let Some(e) = weak.upgrade() else { return };
            if e.filling.get() {
                return;
            }
            e.set_source(b.text(&b.start_iter(), &b.end_iter(), false).to_string());
        });
        let weak = Rc::downgrade(&e);
        e.try_entry.connect_changed(move |en| {
            if let Some(e) = weak.upgrade() {
                *e.try_input.borrow_mut() = en.text().to_string();
                e.recheck();
            }
        });
        let weak = Rc::downgrade(&e);
        e.form_btn.connect_toggled(move |b| {
            if let Some(e) = weak.upgrade()
                && b.is_active()
            {
                e.set_mode(Mode::Form);
            }
        });
        let weak = Rc::downgrade(&e);
        e.source_btn.connect_toggled(move |b| {
            if let Some(e) = weak.upgrade()
                && b.is_active()
            {
                e.set_mode(Mode::Source);
            }
        });
        let weak = Rc::downgrade(&e);
        e.save_btn.connect_clicked(move |_| {
            if let Some(e) = weak.upgrade() {
                e.save();
            }
        });
        e.rebuild_form();
        e.render_try();
        e.update_save();
        e
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn header_widgets(&self) -> Vec<gtk::Widget> {
        let modes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        modes.add_css_class("linked");
        modes.append(&self.form_btn);
        modes.append(&self.source_btn);
        let mut out = vec![modes.upcast::<gtk::Widget>()];
        if !self.read_only() {
            out.push(self.save_btn.clone().upcast());
        }
        out
    }

    pub fn entry(&self) -> &TemplateEntry {
        &self.entry
    }

    pub fn key(&self) -> String {
        super::key(&self.entry)
    }

    pub fn read_only(&self) -> bool {
        self.entry.builtin
    }

    pub fn dirty(&self) -> bool {
        self.dirty.get()
    }

    pub fn set_dirty(&self, d: bool) {
        self.dirty.set(d);
        self.update_save();
    }

    pub fn display_name(&self) -> String {
        self.doc.borrow().as_ref().map(|d| d.name.clone()).unwrap_or_else(|| self.entry.name.clone())
    }

    pub fn subtitle(&self) -> String {
        if let Some(e) = self.save_error.borrow().clone() {
            return e;
        }
        if self.dirty.get() {
            l("未保存", "Unsaved")
        } else if self.read_only() {
            l("内置模板", "Built-in template")
        } else {
            String::new()
        }
    }

    fn can_save(&self) -> bool {
        let c = self.check.borrow();
        self.dirty.get() && !self.read_only() && c.problems.is_empty() && c.name.is_some()
    }

    fn update_save(&self) {
        self.save_btn.set_sensitive(self.can_save());
        if let Some(lib) = self.lib.upgrade() {
            lib.update_title();
        }
    }

    fn set_mode(self: &Rc<Self>, m: Mode) {
        if self.mode.get() == m {
            return;
        }
        self.mode.set(m);
        match m {
            Mode::Form => {
                self.rebuild_form();
                self.stack.set_visible_child_name("form");
            }
            Mode::Source => {
                self.filling.set(true);
                self.source_view.buffer().set_text(&self.source.borrow());
                self.filling.set(false);
                self.stack.set_visible_child_name("source");
            }
        }
    }

    /// 源码模式下的编辑
    fn set_source(self: &Rc<Self>, s: String) {
        if *self.source.borrow() == s {
            return;
        }
        if let Ok(d) = qantrip_core::parse_template_doc(s.clone()) {
            *self.doc.borrow_mut() = Some(d);
        }
        *self.source.borrow_mut() = s;
        self.dirty.set(true);
        self.recheck();
    }

    /// 表单模式下的编辑；`structural` 为真时重建表单（增删参数、改类型等）
    fn update_doc(self: &Rc<Self>, structural: bool, change: impl FnOnce(&mut TemplateDoc)) {
        if self.filling.get() || self.read_only() {
            return;
        }
        let Some(mut d) = self.doc.borrow().clone() else { return };
        change(&mut d);
        *self.source.borrow_mut() = qantrip_core::serialize_template_doc(d.clone());
        *self.doc.borrow_mut() = Some(d);
        self.dirty.set(true);
        self.recheck();
        if structural {
            self.rebuild_form();
        }
    }

    fn recheck(self: &Rc<Self>) {
        let c = self.app.engine.check_source(self.source.borrow().clone(), self.try_input.borrow().clone());
        *self.check.borrow_mut() = c;
        self.render_try();
        self.update_save();
    }

    pub fn save(self: &Rc<Self>) {
        if !self.can_save() {
            return;
        }
        let Some(lib) = self.lib.upgrade() else { return };
        let Some(name) = self.check.borrow().name.clone() else { return };
        // 改名时不得与其他用户模板重名
        if name != self.entry.name && lib.user_entry(&name).is_some() {
            *self.save_error.borrow_mut() = Some(t!("已存在名为「{name}」的模板", "A template named “{name}” already exists"));
            self.update_save();
            return;
        }
        let src = self.source.borrow().clone();
        match lib.write(&src, &name, self.entry.path.as_deref()) {
            Ok(_) => {
                *self.save_error.borrow_mut() = None;
                self.dirty.set(false);
                lib.reload();
                lib.select(None);
                lib.select(Some(name));
            }
            Err(e) => {
                *self.save_error.borrow_mut() = Some(e.to_string());
                self.update_save();
            }
        }
    }

    // MARK: 表单

    fn rebuild_form(self: &Rc<Self>) {
        let vadj = self.form_scroll.vadjustment().value();
        let Some(d) = self.doc.borrow().clone() else {
            let s = adw::StatusPage::builder()
                .icon_name("dialog-warning-symbolic")
                .title(l("源码无法解析", "Source can’t be parsed"))
                .description(l("请在「源码」中修正格式错误后再使用表单。", "Fix the errors in Source before using the form."))
                .build();
            self.form_scroll.set_child(Some(&s));
            return;
        };
        self.filling.set(true);
        let page = gtk::Box::new(gtk::Orientation::Vertical, 18);
        page.set_margin_start(24);
        page.set_margin_end(24);
        page.set_margin_top(18);
        page.set_margin_bottom(24);
        let ro = self.read_only();

        // 基本信息
        let basics = adw::PreferencesGroup::builder().title(l("基本信息", "Basics")).build();
        if ro {
            let row = |title: String, value: String| {
                let r = adw::ActionRow::builder().title(title).subtitle(if value.is_empty() { "—".into() } else { value }).subtitle_selectable(true).build();
                r.add_css_class("property");
                r
            };
            basics.add(&row(l("名称", "Name"), d.name.clone()));
            basics.add(&row(l("别名", "Aliases"), d.aliases.join(", ")));
            basics.add(&row(l("描述", "Description"), d.description.clone()));
            basics.add(&row(l("分类", "Category"), d.category.clone()));
        } else {
            let entry = |title: String, value: &str, f: Box<dyn Fn(&mut TemplateDoc, String)>| {
                let r = adw::EntryRow::builder().title(title).text(value).build();
                let weak = Rc::downgrade(self);
                r.connect_changed(move |r| {
                    if let Some(e) = weak.upgrade() {
                        let v = r.text().to_string();
                        e.update_doc(false, |d| f(d, v));
                    }
                });
                r
            };
            let name = entry(l("名称", "Name"), &d.name, Box::new(|d, v| d.name = v));
            name.add_css_class("mono");
            basics.add(&name);
            let aliases = entry(
                l("别名（用逗号分隔，如 r, rev）", "Aliases (comma-separated, e.g. r, rev)"),
                &d.aliases.join(", "),
                Box::new(|d, v| d.aliases = split_list(&v)),
            );
            basics.add(&aliases);
            basics.add(&entry(l("描述", "Description"), &d.description, Box::new(|d, v| d.description = v)));
            basics.add(&entry(l("分类（如 编程、日常）", "Category (e.g. Coding, Daily)"), &d.category, Box::new(|d, v| d.category = v)));
        }
        page.append(&basics);

        // 参数
        let params = adw::PreferencesGroup::builder()
            .title(l("参数", "Parameters"))
            .description(l(
                "参数顺序即命令补全和规范形式中的顺序。至多一个单选 / 多选参数可设为位置参数（可直接输入候选值，如 r security）。",
                "Parameter order determines the order in completion and the canonical form. At most one single- or multi-choice parameter can be positional (its values can be typed directly, e.g. r security).",
            ))
            .build();
        for (i, p) in d.params.iter().enumerate() {
            params.add(&self.param_row(i, p, d.params.len()));
        }
        if !ro {
            let add = gtk::Button::builder().label(l("添加参数", "Add Parameter")).halign(gtk::Align::Start).margin_top(6).build();
            add.add_css_class("flat");
            let weak = Rc::downgrade(self);
            add.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() {
                    let n = e.doc.borrow().as_ref().map(|d| d.params.len()).unwrap_or(0);
                    e.expanded.set(Some(n));
                    e.update_doc(true, |d| {
                        let mut name = "param".to_string();
                        let mut k = 2;
                        while d.params.iter().any(|p| p.name == name) {
                            name = format!("param{k}");
                            k += 1;
                        }
                        d.params.push(ParamDoc {
                            name,
                            kind: ParamKind::Bool,
                            description: String::new(),
                            choices: vec![],
                            default_value: None,
                            positional: false,
                            short: None,
                            required: false,
                            requires: vec![],
                            conflicts: vec![],
                            min: None,
                            max: None,
                        });
                    });
                }
            });
            params.add(&add);
        }
        page.append(&params);

        // 正文
        let body = adw::PreferencesGroup::builder()
            .title(l("正文", "Body"))
            .description(l(
                "MiniJinja 语法：{{ 参数名 }}（参数名中的 - 写作 _）、{% if strict %}…{% endif %}、{% for x in focus %}…{% endfor %}；附加要求为 extra；{% include \"_extra\" %} 引用公共片段。",
                "MiniJinja syntax: {{ param_name }} (use _ in place of - in names), {% if strict %}…{% endif %}, {% for x in focus %}…{% endfor %}. Extra instructions are available as extra; {% include \"_extra\" %} pulls in a shared snippet.",
            ))
            .build();
        let tv = gtk::TextView::builder()
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .editable(!ro)
            .top_margin(10)
            .bottom_margin(10)
            .left_margin(10)
            .right_margin(10)
            .build();
        tv.add_css_class("code-view");
        tv.buffer().set_text(&d.body);
        tv.set_size_request(-1, 240);
        let frame = gtk::Frame::new(None);
        frame.set_child(Some(&tv));
        body.add(&frame);
        let weak = Rc::downgrade(self);
        tv.buffer().connect_changed(move |b| {
            if let Some(e) = weak.upgrade() {
                let v = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
                e.update_doc(false, |d| d.body = v);
            }
        });
        page.append(&body);

        let clamp = adw::Clamp::builder().maximum_size(820).child(&page).build();
        self.form_scroll.set_child(Some(&clamp));
        self.filling.set(false);
        let adj = self.form_scroll.vadjustment();
        glib::idle_add_local_once(move || adj.set_value(vadj));
    }

    fn param_row(self: &Rc<Self>, i: usize, p: &ParamDoc, count: usize) -> adw::ExpanderRow {
        let ro = self.read_only();
        let mut flags = vec![kind_label(p.kind)];
        if let Some(s) = p.short.as_ref().filter(|s| !s.is_empty()) {
            flags.push(format!("-{s}"));
        }
        if p.positional {
            flags.push(l("位置", "Positional"));
        }
        if p.required {
            flags.push(l("必填", "Required"));
        }
        if !p.description.is_empty() {
            flags.push(p.description.clone());
        }
        let pal = crate::theme::palette();
        let row = adw::ExpanderRow::builder()
            .title(format!("<span font_family=\"monospace\" foreground=\"{}\">--{}</span>", pal.option, glib::markup_escape_text(&p.name)))
            .use_markup(true)
            .subtitle(glib::markup_escape_text(&flags.join(" · ")).to_string())
            .expanded(self.expanded.get() == Some(i))
            .build();
        let weak = Rc::downgrade(self);
        row.connect_expanded_notify(move |r| {
            if let Some(e) = weak.upgrade() {
                if r.is_expanded() {
                    e.expanded.set(Some(i));
                } else if e.expanded.get() == Some(i) {
                    e.expanded.set(None);
                }
            }
        });

        let set = move |f: Box<dyn Fn(&mut ParamDoc)>, structural: bool, weak: Weak<Editor>| {
            if let Some(e) = weak.upgrade() {
                e.update_doc(structural, move |d| {
                    if let Some(p) = d.params.get_mut(i) {
                        f(p);
                    }
                });
            }
        };

        // 名称
        let name = adw::EntryRow::builder().title(l("参数名", "Name")).text(&p.name).build();
        name.add_css_class("mono");
        let weak = Rc::downgrade(self);
        let r2 = row.clone();
        name.connect_changed(move |r| {
            let v = r.text().to_string();
            let pal = crate::theme::palette();
            r2.set_title(&format!("<span font_family=\"monospace\" foreground=\"{}\">--{}</span>", pal.option, glib::markup_escape_text(&v)));
            set(Box::new(move |p| p.name = v.clone()), false, weak.clone());
        });
        add_row(&row, ro, &name);

        // 类型
        let labels: Vec<String> = KINDS.iter().map(|k| kind_label(*k)).collect();
        let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
        let kind =
            adw::ComboRow::builder().title(l("类型", "Type")).model(&model).selected(KINDS.iter().position(|k| *k == p.kind).unwrap_or(0) as u32).build();
        let weak = Rc::downgrade(self);
        kind.connect_selected_notify(move |c| {
            let k = KINDS[c.selected() as usize % KINDS.len()];
            set(
                Box::new(move |p| {
                    if p.kind == k {
                        return;
                    }
                    p.kind = k;
                    p.default_value = None;
                    if k != ParamKind::Bool {
                        p.short = None;
                    }
                    if k != ParamKind::Enum && k != ParamKind::Multi {
                        p.positional = false;
                        p.choices.clear();
                    }
                    if k != ParamKind::Number {
                        p.min = None;
                        p.max = None;
                    }
                }),
                true,
                weak.clone(),
            );
        });
        add_row(&row, ro, &kind);

        let desc = adw::EntryRow::builder().title(l("说明", "Description")).text(&p.description).build();
        let weak = Rc::downgrade(self);
        desc.connect_changed(move |r| {
            let v = r.text().to_string();
            set(Box::new(move |p| p.description = v.clone()), false, weak.clone());
        });
        add_row(&row, ro, &desc);

        let required = adw::SwitchRow::builder().title(l("必填", "Required")).active(p.required).build();
        let weak = Rc::downgrade(self);
        required.connect_active_notify(move |r| {
            let v = r.is_active();
            set(Box::new(move |p| p.required = v), false, weak.clone());
        });
        add_row(&row, ro, &required);

        match p.kind {
            ParamKind::Bool => {
                let short = adw::EntryRow::builder().title(l("短名（一个字母）", "Short name (one letter)")).text(p.short.clone().unwrap_or_default()).build();
                short.add_css_class("mono");
                let weak = Rc::downgrade(self);
                short.connect_changed(move |r| {
                    let v: String = r.text().chars().take(1).collect();
                    set(Box::new(move |p| p.short = if v.is_empty() { None } else { Some(v.clone()) }), false, weak.clone());
                });
                add_row(&row, ro, &short);
                let def = adw::SwitchRow::builder().title(l("默认开启", "On by default")).active(p.default_value.as_deref() == Some("true")).build();
                let weak = Rc::downgrade(self);
                def.connect_active_notify(move |r| {
                    let v = r.is_active();
                    set(Box::new(move |p| p.default_value = v.then(|| "true".to_string())), false, weak.clone());
                });
                add_row(&row, ro, &def);
            }
            ParamKind::Enum | ParamKind::Multi => {
                let pos = adw::SwitchRow::builder().title(l("位置参数", "Positional")).active(p.positional).build();
                let weak = Rc::downgrade(self);
                pos.connect_active_notify(move |r| {
                    let v = r.is_active();
                    set(Box::new(move |p| p.positional = v), false, weak.clone());
                });
                add_row(&row, ro, &pos);
                if p.kind == ParamKind::Enum {
                    let mut opts = vec![l("无", "None")];
                    opts.extend(p.choices.iter().map(|c| format!("{}（{}）", c.label, c.value)));
                    let m = gtk::StringList::new(&opts.iter().map(String::as_str).collect::<Vec<_>>());
                    let sel = p.default_value.as_ref().and_then(|v| p.choices.iter().position(|c| &c.value == v)).map(|x| x + 1).unwrap_or(0);
                    let def = adw::ComboRow::builder().title(l("默认值", "Default")).model(&m).selected(sel as u32).build();
                    let values: Vec<String> = p.choices.iter().map(|c| c.value.clone()).collect();
                    let weak = Rc::downgrade(self);
                    def.connect_selected_notify(move |c| {
                        let v = (c.selected() as usize).checked_sub(1).and_then(|i| values.get(i).cloned());
                        set(Box::new(move |p| p.default_value = v.clone()), false, weak.clone());
                    });
                    add_row(&row, ro, &def);
                } else {
                    let defrow = adw::ActionRow::builder().title(l("默认选中", "Selected by default")).build();
                    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                    bx.set_valign(gtk::Align::Center);
                    let on: Vec<String> = p.default_value.as_deref().map(split_list).unwrap_or_default();
                    for c in &p.choices {
                        let tb = gtk::ToggleButton::builder().label(&c.label).active(on.contains(&c.value)).build();
                        let weak = Rc::downgrade(self);
                        let value = c.value.clone();
                        tb.connect_toggled(move |b| {
                            let v = b.is_active();
                            let value = value.clone();
                            set(
                                Box::new(move |p| {
                                    let mut cur: Vec<String> = p.default_value.as_deref().map(split_list).unwrap_or_default();
                                    if v {
                                        cur.push(value.clone());
                                    } else {
                                        cur.retain(|x| x != &value);
                                    }
                                    let ordered: Vec<String> = p.choices.iter().map(|c| c.value.clone()).filter(|x| cur.contains(x)).collect();
                                    p.default_value = if ordered.is_empty() { None } else { Some(ordered.join(",")) };
                                }),
                                false,
                                weak.clone(),
                            );
                        });
                        bx.append(&tb);
                    }
                    defrow.add_suffix(&bx);
                    add_row(&row, ro, &defrow);
                }
                // 候选值
                for (ci, c) in p.choices.iter().enumerate() {
                    add_row(&row, ro, &self.choice_row(i, ci, c));
                }
                if !ro {
                    let add = gtk::Button::builder()
                        .label(l("添加候选值", "Add Choice"))
                        .halign(gtk::Align::Start)
                        .margin_top(4)
                        .margin_bottom(4)
                        .margin_start(8)
                        .build();
                    add.add_css_class("flat");
                    let weak = Rc::downgrade(self);
                    add.connect_clicked(move |_| {
                        set(
                            Box::new(|p| {
                                let mut n = "option".to_string();
                                let mut k = 2;
                                while p.choices.iter().any(|c| c.value == n) {
                                    n = format!("option{k}");
                                    k += 1;
                                }
                                p.choices.push(Choice { value: n.clone(), label: n, aliases: vec![] });
                            }),
                            true,
                            weak.clone(),
                        );
                    });
                    add_row(&row, ro, &add);
                }
            }
            ParamKind::Number => {
                let num_row = |title: String, value: Option<f64>, f: fn(&mut ParamDoc, Option<f64>), weak: Weak<Editor>| {
                    let r = adw::EntryRow::builder().title(title).text(value.map(fmt_num).unwrap_or_default()).input_purpose(gtk::InputPurpose::Number).build();
                    r.connect_changed(move |r| {
                        let v = r.text().trim().parse::<f64>().ok();
                        set(Box::new(move |p| f(p, v)), false, weak.clone());
                    });
                    r
                };
                add_row(
                    &row,
                    ro,
                    &num_row(
                        l("默认值", "Default"),
                        p.default_value.as_deref().and_then(|v| v.parse().ok()),
                        |p, v| p.default_value = v.map(fmt_num),
                        Rc::downgrade(self),
                    ),
                );
                add_row(&row, ro, &num_row(l("最小值", "Minimum"), p.min, |p, v| p.min = v, Rc::downgrade(self)));
                add_row(&row, ro, &num_row(l("最大值", "Maximum"), p.max, |p, v| p.max = v, Rc::downgrade(self)));
            }
            _ => {
                let def = adw::EntryRow::builder().title(l("默认值", "Default")).text(p.default_value.clone().unwrap_or_default()).build();
                let weak = Rc::downgrade(self);
                def.connect_changed(move |r| {
                    let v = r.text().to_string();
                    set(Box::new(move |p| p.default_value = if v.is_empty() { None } else { Some(v.clone()) }), false, weak.clone());
                });
                add_row(&row, ro, &def);
            }
        }

        if !ro {
            let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            actions.set_margin_top(6);
            actions.set_margin_bottom(6);
            actions.set_margin_start(8);
            actions.set_margin_end(8);
            let up = gtk::Button::from_icon_name("go-up-symbolic");
            up.set_tooltip_text(Some(&l("上移", "Move up")));
            up.set_sensitive(i > 0);
            let down = gtk::Button::from_icon_name("go-down-symbolic");
            down.set_tooltip_text(Some(&l("下移", "Move down")));
            down.set_sensitive(i + 1 < count);
            let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            spacer.set_hexpand(true);
            let del = gtk::Button::with_label(&l("删除参数", "Delete Parameter"));
            del.add_css_class("destructive-action");
            for b in [&up, &down] {
                b.add_css_class("flat");
            }
            actions.append(&up);
            actions.append(&down);
            actions.append(&spacer);
            actions.append(&del);
            let weak = Rc::downgrade(self);
            up.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() {
                    e.expanded.set(Some(i - 1));
                    e.update_doc(true, |d| d.params.swap(i, i - 1));
                }
            });
            let weak = Rc::downgrade(self);
            down.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() {
                    e.expanded.set(Some(i + 1));
                    e.update_doc(true, |d| d.params.swap(i, i + 1));
                }
            });
            let weak = Rc::downgrade(self);
            del.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() {
                    e.expanded.set(None);
                    e.update_doc(true, |d| {
                        d.params.remove(i);
                    });
                }
            });
            add_row(&row, ro, &actions);
        }
        row
    }

    fn choice_row(self: &Rc<Self>, pi: usize, ci: usize, c: &Choice) -> gtk::Box {
        let ro = self.read_only();
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        b.set_margin_start(8);
        b.set_margin_end(8);
        b.set_margin_top(4);
        b.set_margin_bottom(4);
        let value = gtk::Entry::builder().text(&c.value).placeholder_text(l("值", "Value")).width_chars(12).editable(!ro).build();
        value.add_css_class("mono");
        let label = gtk::Entry::builder().text(&c.label).placeholder_text(l("显示名", "Label")).width_chars(10).editable(!ro).build();
        let aliases = gtk::Entry::builder()
            .text(c.aliases.join(", "))
            .placeholder_text(l("别名（逗号分隔）", "Aliases (comma-separated)"))
            .hexpand(true)
            .editable(!ro)
            .build();
        b.append(&value);
        b.append(&label);
        b.append(&aliases);
        let upd = move |e: &Rc<Editor>, f: Box<dyn Fn(&mut Choice)>| {
            e.update_doc(false, move |d| {
                if let Some(c) = d.params.get_mut(pi).and_then(|p| p.choices.get_mut(ci)) {
                    f(c);
                }
            });
        };
        let weak = Rc::downgrade(self);
        value.connect_changed(move |en| {
            if let Some(e) = weak.upgrade() {
                let v = en.text().to_string();
                upd(&e, Box::new(move |c| c.value = v.clone()));
            }
        });
        let weak = Rc::downgrade(self);
        label.connect_changed(move |en| {
            if let Some(e) = weak.upgrade() {
                let v = en.text().to_string();
                upd(&e, Box::new(move |c| c.label = v.clone()));
            }
        });
        let weak = Rc::downgrade(self);
        aliases.connect_changed(move |en| {
            if let Some(e) = weak.upgrade() {
                let v = split_list(&en.text());
                upd(&e, Box::new(move |c| c.aliases = v.clone()));
            }
        });
        if !ro {
            let rm = gtk::Button::from_icon_name("list-remove-symbolic");
            rm.add_css_class("flat");
            rm.set_tooltip_text(Some(&l("删除候选值", "Remove choice")));
            let weak = Rc::downgrade(self);
            rm.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() {
                    e.update_doc(true, |d| {
                        if let Some(p) = d.params.get_mut(pi)
                            && ci < p.choices.len()
                        {
                            p.choices.remove(ci);
                        }
                    });
                }
            });
            b.append(&rm);
        }
        b
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn debug(self: &Rc<Self>, op: &str) {
        match op {
            "save" => self.save(),
            "mode:source" => self.source_btn.set_active(true),
            "mode:form" => self.form_btn.set_active(true),
            "customize" => {
                if let Some(lib) = self.lib.upgrade() {
                    lib.customize(&self.entry);
                }
            }
            o if o.starts_with("desc:") => {
                let v = o[5..].to_string();
                self.update_doc(true, move |d| d.description = v.clone());
            }
            o if o.starts_with("try:") => self.try_entry.set_text(&o[4..]),
            _ => {}
        }
        crate::log::event(
            "editor",
            serde_json::json!({"op": op, "dirty": self.dirty.get(), "problems": self.check.borrow().problems.clone(), "path": self.entry.path.clone().unwrap_or_default()}),
        );
    }

    // MARK: 试用与预览

    fn render_try(&self) {
        while let Some(c) = self.try_content.first_child() {
            self.try_content.remove(&c);
        }
        let check = self.check.borrow();
        if !check.problems.is_empty() {
            let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
            b.add_css_class("problem-box");
            for p in &check.problems {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let i = gtk::Image::from_icon_name("dialog-error-symbolic");
                i.add_css_class("error");
                let lbl = gtk::Label::new(Some(p));
                lbl.set_wrap(true);
                lbl.set_xalign(0.0);
                lbl.set_selectable(true);
                lbl.add_css_class("error");
                row.append(&i);
                row.append(&lbl);
                b.append(&row);
            }
            self.try_content.append(&b);
            return;
        }
        let Some(a) = &check.analysis else { return };
        if let Some(e) = a.diagnostics.iter().find(|d| d.severity == Severity::Error) {
            let lbl = gtk::Label::new(Some(&e.message));
            lbl.add_css_class("warning");
            lbl.set_wrap(true);
            lbl.set_xalign(0.0);
            self.try_content.append(&lbl);
        }
        if !a.params.is_empty() {
            let flow = gtk::FlowBox::new();
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_column_spacing(6);
            flow.set_row_spacing(6);
            for p in &a.params {
                let chip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                chip.add_css_class("chip");
                if p.is_set {
                    chip.add_css_class("set");
                }
                let n = gtk::Label::new(Some(&p.name));
                n.add_css_class("chip-name");
                let v = gtk::Label::new(Some(&p.display));
                v.add_css_class("chip-value");
                chip.append(&n);
                chip.append(&v);
                flow.insert(&chip, -1);
            }
            self.try_content.append(&flow);
        }
        if let Some(p) = &a.preview {
            let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let t = gtk::Label::new(Some(&l("预览", "Preview")));
            t.add_css_class("caption-heading");
            t.add_css_class("dim-label");
            t.set_hexpand(true);
            t.set_xalign(0.0);
            let (chars, lines) = (p.chars, p.lines);
            let meta = gtk::Label::new(Some(&t!("{chars} 字 · {lines} 行", "{chars} chars · {lines} lines")));
            meta.add_css_class("caption");
            meta.add_css_class("dim-label");
            head.append(&t);
            head.append(&meta);
            self.try_content.append(&head);
            let text = gtk::Label::new(Some(&p.text));
            text.set_wrap(true);
            text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            text.set_xalign(0.0);
            text.set_selectable(true);
            text.set_margin_start(12);
            text.set_margin_end(12);
            text.set_margin_top(12);
            text.set_margin_bottom(12);
            let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
            frame.add_css_class("preview-box");
            frame.append(&text);
            self.try_content.append(&frame);
        }
    }
}
