//! 模板库与设置窗口（PRD 4.5 / 11.2，docs/decisions.md D19）：侧栏为模板列表（我的模板 / 内置 / 公共片段 /
//! 已删除）与底部固定的"设置"；右侧为模板编辑器或设置页。与 macOS 的 LibraryWindow.swift 对应。

mod editor;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use qantrip_core::TemplateEntry;
use qantrip_core::command::CommandOutcome;

use crate::app::App;
use crate::i18n::l;
use crate::paths;
use crate::settings_ui::SettingsView;
use crate::t;

pub use editor::Editor;

/// 调试钩子：驱动当前编辑的模板
#[cfg(all(feature = "debug-hooks", debug_assertions))]
pub fn debug_editor(app: &Rc<App>, op: &str) {
    let Some(lib) = app.library_handle() else { return };
    if op == "new" {
        lib.new_template();
        return;
    }
    let Some(e) = lib.editor.borrow().clone() else { return };
    e.debug(op);
}

/// 移到回收站（freedesktop Trash），失败时删除
pub fn trash(path: &Path) {
    let f = gio::File::for_path(path);
    if f.trash(gio::Cancellable::NONE).is_err() {
        let _ = std::fs::remove_file(path);
    }
}

/// 列表中的标识：已删除的内置模板可能与同名的用户模板并存
pub fn key(e: &TemplateEntry) -> String {
    if e.deleted { format!("deleted:{}", e.name) } else { e.name.clone() }
}

pub struct LibraryWindow {
    app: Rc<App>,
    window: adw::ApplicationWindow,
    split: adw::NavigationSplitView,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    settings_button: gtk::ToggleButton,
    content_title: adw::WindowTitle,
    content_header: adw::HeaderBar,
    content_stack: gtk::Stack,
    header_end: gtk::Box,
    lib_buttons: gtk::Box,
    pub entries: RefCell<Vec<TemplateEntry>>,
    selection: RefCell<Option<String>>,
    pub editor: RefCell<Option<Rc<Editor>>>,
    settings: RefCell<Option<Rc<SettingsView>>>,
    /// 程序修改列表选择时不触发选择回调
    syncing: Cell<bool>,
    row_keys: RefCell<Vec<Option<String>>>,
}

impl LibraryWindow {
    pub fn new(app: Rc<App>) -> Rc<LibraryWindow> {
        let window = adw::ApplicationWindow::builder().application(&app.gapp).title("Qantrip").default_width(1180).default_height(760).build();
        window.set_hide_on_close(true);

        // 侧栏
        let search = gtk::SearchEntry::builder()
            .placeholder_text(l("搜索模板", "Search templates"))
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(6)
            .build();
        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.set_selection_mode(gtk::SelectionMode::Single);
        let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        let settings_button = gtk::ToggleButton::new();
        let sb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        sb.append(&gtk::Image::from_icon_name("qantrip-settings-symbolic"));
        sb.append(&gtk::Label::new(Some(&l("设置", "Settings"))));
        settings_button.set_child(Some(&sb));
        settings_button.add_css_class("flat");
        settings_button.set_margin_start(8);
        settings_button.set_margin_end(8);
        settings_button.set_margin_top(6);
        settings_button.set_margin_bottom(8);
        let side_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        side_box.append(&search);
        side_box.append(&scroll);
        side_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        side_box.append(&settings_button);

        let new_btn = gtk::Button::builder().icon_name("list-add-symbolic").tooltip_text(l("新建模板", "New template")).build();
        let import_btn = gtk::MenuButton::builder().icon_name("document-open-symbolic").tooltip_text(l("导入模板", "Import templates")).build();
        let export_btn =
            gtk::Button::builder().icon_name("document-save-symbolic").tooltip_text(l("把我的模板导出为 zip", "Export my templates as a zip")).build();
        let lib_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        lib_buttons.append(&new_btn);
        lib_buttons.append(&import_btn);
        lib_buttons.append(&export_btn);
        let side_header = adw::HeaderBar::new();
        side_header.set_show_end_title_buttons(false);
        side_header.set_title_widget(Some(&adw::WindowTitle::new(&l("模板库", "Templates"), "")));
        side_header.pack_start(&lib_buttons);
        let side_view = adw::ToolbarView::new();
        side_view.add_top_bar(&side_header);
        side_view.set_content(Some(&side_box));
        let sidebar = adw::NavigationPage::new(&side_view, "Qantrip");

        // 内容
        let content_title = adw::WindowTitle::new("Qantrip", "");
        let header_end = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let content_header = adw::HeaderBar::new();
        content_header.set_title_widget(Some(&content_title));
        content_header.pack_end(&header_end);
        let content_stack = gtk::Stack::new();
        content_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        let empty = adw::StatusPage::builder()
            .icon_name("qantrip-wand-symbolic")
            .title(l("选择或新建一个模板", "Select or create a template"))
            .description(l(
                "模板保存为 Markdown 文件，也可以用任意编辑器直接修改。",
                "Templates are saved as Markdown files. You can also edit them in any editor.",
            ))
            .build();
        content_stack.add_named(&empty, Some("empty"));
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&content_header);
        content_view.set_content(Some(&content_stack));
        let content = adw::NavigationPage::new(&content_view, "Qantrip");

        let split = adw::NavigationSplitView::new();
        split.set_sidebar(Some(&sidebar));
        split.set_content(Some(&content));
        split.set_min_sidebar_width(240.0);
        split.set_max_sidebar_width(320.0);
        window.set_content(Some(&split));

        let w = Rc::new(LibraryWindow {
            app,
            window,
            split,
            search,
            list,
            settings_button,
            content_title,
            content_header,
            content_stack,
            header_end,
            lib_buttons,
            entries: RefCell::new(Vec::new()),
            selection: RefCell::new(None),
            editor: RefCell::new(None),
            settings: RefCell::new(None),
            syncing: Cell::new(false),
            row_keys: RefCell::new(Vec::new()),
        });

        // 导入菜单：文件 / 文件夹
        let pop = gtk::Popover::new();
        let pbox = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let imp_files = gtk::Button::with_label(&l("导入文件（.md / .zip）…", "Import Files (.md / .zip)…"));
        let imp_dir = gtk::Button::with_label(&l("导入文件夹…", "Import Folder…"));
        for b in [&imp_files, &imp_dir] {
            b.add_css_class("flat");
            pbox.append(b);
        }
        pop.set_child(Some(&pbox));
        import_btn.set_popover(Some(&pop));
        let weak = Rc::downgrade(&w);
        let p2 = pop.clone();
        imp_files.connect_clicked(move |_| {
            p2.popdown();
            if let Some(w) = weak.upgrade() {
                w.import_files();
            }
        });
        let weak = Rc::downgrade(&w);
        let p2 = pop.clone();
        imp_dir.connect_clicked(move |_| {
            p2.popdown();
            if let Some(w) = weak.upgrade() {
                w.import_folder();
            }
        });
        let weak = Rc::downgrade(&w);
        new_btn.connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                w.new_template();
            }
        });
        let weak = Rc::downgrade(&w);
        export_btn.connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                w.export_all();
            }
        });
        let weak = Rc::downgrade(&w);
        w.search.connect_search_changed(move |_| {
            if let Some(w) = weak.upgrade() {
                w.rebuild_list();
            }
        });
        let weak = Rc::downgrade(&w);
        w.list.connect_row_selected(move |_, row| {
            let Some(w) = weak.upgrade() else { return };
            if w.syncing.get() {
                return;
            }
            let Some(row) = row else { return };
            let idx = row.index() as usize;
            let key = w.row_keys.borrow().get(idx).cloned().flatten();
            if let Some(k) = key {
                w.select(Some(k));
            }
        });
        let weak = Rc::downgrade(&w);
        w.settings_button.connect_clicked(move |b| {
            let Some(w) = weak.upgrade() else { return };
            if w.syncing.get() {
                return;
            }
            if b.is_active() {
                w.select(Some("settings:general".into()));
            } else {
                // 已在设置页时再点一次保持选中
                w.syncing.set(true);
                b.set_active(w.is_settings());
                w.syncing.set(false);
            }
        });
        // 拖入文件导入
        let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let weak = Rc::downgrade(&w);
        drop.connect_drop(move |_, value, _, _| {
            let Some(w) = weak.upgrade() else { return false };
            let Ok(files) = value.get::<gdk::FileList>() else { return false };
            let paths: Vec<PathBuf> = files.files().iter().filter_map(|f| f.path()).collect();
            w.import_paths(paths);
            true
        });
        w.window.add_controller(drop);
        // Ctrl+S 保存，Ctrl+F 搜索，Ctrl+W 关闭
        let sc = gtk::ShortcutController::new();
        sc.set_scope(gtk::ShortcutScope::Global);
        let weak = Rc::downgrade(&w);
        sc.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>s"),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(w) = weak.upgrade()
                    && let Some(e) = w.editor.borrow().clone()
                {
                    e.save();
                }
                glib::Propagation::Stop
            })),
        ));
        let weak = Rc::downgrade(&w);
        sc.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>f"),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(w) = weak.upgrade() {
                    w.search.grab_focus();
                }
                glib::Propagation::Stop
            })),
        ));
        let weak = Rc::downgrade(&w);
        sc.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>w"),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(w) = weak.upgrade() {
                    w.window.close();
                }
                glib::Propagation::Stop
            })),
        ));
        w.window.add_controller(sc);
        let weak = Rc::downgrade(&w);
        w.window.connect_close_request(move |_| {
            let Some(w) = weak.upgrade() else { return glib::Propagation::Proceed };
            if let Some(s) = w.settings.borrow().as_ref() {
                s.set_visible(false);
            }
            glib::Propagation::Proceed
        });
        w.reload();
        w
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn window(&self) -> &adw::ApplicationWindow {
        &self.window
    }

    pub fn is_visible(&self) -> bool {
        self.window.is_visible()
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn show(self: &Rc<Self>, select: Option<String>) {
        self.reload();
        if let Some(name) = select {
            self.select(Some(name));
        } else if self.is_settings() {
            self.select(None);
        }
        self.window.present();
    }

    pub fn show_settings(self: &Rc<Self>, page: &str) {
        self.reload();
        self.select(Some(format!("settings:{page}")));
        self.window.present();
    }

    fn is_settings(&self) -> bool {
        self.selection.borrow().as_deref().is_some_and(|s| s.starts_with("settings:"))
    }

    pub fn selection(&self) -> Option<String> {
        self.selection.borrow().clone()
    }

    pub fn has_unsaved(&self) -> bool {
        self.editor.borrow().as_ref().is_some_and(|e| e.dirty())
    }

    /// 关闭并销毁窗口（界面语言变化后重建）
    pub fn destroy(&self) {
        if let Some(s) = self.settings.borrow().as_ref() {
            s.set_visible(false);
        }
        self.window.destroy();
    }

    /// 重新读取模板列表（编辑中且未修改的模板跟随磁盘内容刷新）
    pub fn reload(self: &Rc<Self>) {
        self.app.engine.reload();
        *self.entries.borrow_mut() = self.app.engine.entries();
        self.rebuild_list();
        let editor = self.editor.borrow().clone();
        if let Some(e) = editor
            && !e.dirty()
        {
            let k = e.key();
            let fresh = self.entries.borrow().iter().find(|x| key(x) == k).cloned();
            if let Some(fresh) = fresh
                && (fresh.source != e.entry().source || fresh.path != e.entry().path)
            {
                self.open_editor(fresh);
            }
        }
    }

    fn filtered(&self) -> Vec<TemplateEntry> {
        let q = self.search.text().to_lowercase();
        self.entries
            .borrow()
            .iter()
            .filter(|e| {
                q.is_empty()
                    || e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
                    || e.aliases.iter().any(|a| a.to_lowercase().contains(&q))
            })
            .cloned()
            .collect()
    }

    fn rebuild_list(self: &Rc<Self>) {
        self.syncing.set(true);
        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let items = self.filtered();
        let groups: [(String, Vec<&TemplateEntry>); 4] = [
            (l("我的模板", "My templates"), items.iter().filter(|e| !e.builtin && !e.partial).collect()),
            (l("内置", "Built-in"), items.iter().filter(|e| e.builtin && !e.partial && !e.deleted).collect()),
            (l("公共片段", "Shared snippets"), items.iter().filter(|e| e.partial).collect()),
            (l("已删除", "Deleted"), items.iter().filter(|e| e.deleted).collect()),
        ];
        let mut keys = Vec::new();
        let selected = self.selection.borrow().clone();
        let mut select_row = None;
        for (title, list) in groups {
            if list.is_empty() {
                continue;
            }
            let h = gtk::Label::new(Some(&title));
            h.add_css_class("heading");
            h.add_css_class("dim-label");
            h.set_xalign(0.0);
            h.set_margin_top(10);
            h.set_margin_start(6);
            let hrow = gtk::ListBoxRow::new();
            hrow.set_child(Some(&h));
            hrow.set_selectable(false);
            hrow.set_activatable(false);
            self.list.append(&hrow);
            keys.push(None);
            for e in list {
                let row = self.sidebar_row(e);
                self.list.append(&row);
                let k = key(e);
                if selected.as_deref() == Some(k.as_str()) {
                    select_row = Some(row.clone());
                }
                keys.push(Some(k));
            }
        }
        *self.row_keys.borrow_mut() = keys;
        if let Some(r) = select_row {
            self.list.select_row(Some(&r));
        }
        self.syncing.set(false);
    }

    fn sidebar_row(self: &Rc<Self>, e: &TemplateEntry) -> gtk::ListBoxRow {
        let v = gtk::Box::new(gtk::Orientation::Vertical, 3);
        v.set_margin_top(3);
        v.set_margin_bottom(3);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        let name = gtk::Label::new(Some(&e.name));
        name.add_css_class("lib-row-name");
        top.append(&name);
        for a in e.aliases.iter().take(2) {
            let al = gtk::Label::new(Some(a));
            al.add_css_class("lib-alias");
            top.append(&al);
        }
        if e.overrides_builtin {
            let t = gtk::Label::new(Some(&l("已自定义", "Customized")));
            t.add_css_class("tag");
            top.append(&t);
        }
        v.append(&top);
        if !e.description.is_empty() {
            let d = gtk::Label::new(Some(&e.description));
            d.add_css_class("caption");
            d.add_css_class("dim-label");
            d.set_xalign(0.0);
            d.set_ellipsize(gtk::pango::EllipsizeMode::End);
            v.append(&d);
        }
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&v));
        // 右键菜单
        let click = gtk::GestureClick::new();
        click.set_button(3);
        let weak = Rc::downgrade(self);
        let entry = e.clone();
        let row2 = row.clone();
        click.connect_pressed(move |_, _, x, y| {
            if let Some(w) = weak.upgrade() {
                w.context_menu(&row2, &entry, x, y);
            }
        });
        row.add_controller(click);
        row
    }

    fn context_menu(self: &Rc<Self>, row: &gtk::ListBoxRow, e: &TemplateEntry, x: f64, y: f64) {
        let pop = gtk::Popover::new();
        pop.set_parent(row);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.set_has_arrow(false);
        let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let add = |label: String, f: Box<dyn Fn(&Rc<LibraryWindow>)>, destructive: bool| {
            let btn = gtk::Button::with_label(&label);
            btn.add_css_class("flat");
            if destructive {
                btn.add_css_class("error");
            }
            if let Some(l) = btn.child().and_downcast::<gtk::Label>() {
                l.set_xalign(0.0);
            }
            let weak = Rc::downgrade(self);
            let p = pop.clone();
            btn.connect_clicked(move |_| {
                p.popdown();
                if let Some(w) = weak.upgrade() {
                    f(&w);
                }
            });
            b.append(&btn);
        };
        let e1 = e.clone();
        add(l("复制为新模板", "Duplicate as New Template"), Box::new(move |w| w.duplicate(&e1)), false);
        let e2 = e.clone();
        add(l("导出…", "Export…"), Box::new(move |w| w.export_one(&e2)), false);
        if let (Some(path), false) = (e.path.clone(), e.deleted) {
            add(
                l("在文件管理器中显示", "Show in Files"),
                Box::new(move |w| {
                    gtk::FileLauncher::new(Some(&gio::File::for_path(&path))).open_containing_folder(Some(&w.window), gio::Cancellable::NONE, |_| {});
                }),
                false,
            );
        }
        if !e.partial {
            if e.deleted || e.overrides_builtin {
                let e3 = e.clone();
                add(l("恢复内置版本", "Restore Built-in Version"), Box::new(move |w| w.restore(&e3)), false);
            }
            if !e.deleted {
                let e4 = e.clone();
                add(l("删除", "Delete"), Box::new(move |w| w.delete(&e4)), true);
            }
        }
        pop.set_child(Some(&b));
        pop.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        pop.popup();
    }

    // MARK: 选择

    pub fn select(self: &Rc<Self>, sel: Option<String>) {
        if *self.selection.borrow() == sel && sel.is_some() {
            if sel.as_deref().is_some_and(|k| k.starts_with("settings:")) {
                self.show_settings_page(sel.as_deref().unwrap());
            }
            return;
        }
        let dirty = self.editor.borrow().as_ref().filter(|e| e.dirty()).map(|e| e.display_name());
        if let Some(name) = dirty {
            let this = self.clone();
            let key2 = sel.clone();
            glib::spawn_future_local(async move {
                let d = adw::AlertDialog::new(Some(&t!("放弃对「{name}」的修改？", "Discard changes to “{name}”?")), None);
                d.add_response("cancel", &l("取消", "Cancel"));
                d.add_response("discard", &l("放弃修改", "Discard Changes"));
                d.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                d.set_default_response(Some("cancel"));
                if d.choose_future(Some(&this.window)).await == "discard" {
                    if let Some(e) = this.editor.borrow().as_ref() {
                        e.set_dirty(false);
                    }
                    this.select(key2);
                } else {
                    // 恢复列表中的选择
                    this.rebuild_list();
                    this.sync_settings_button();
                }
            });
            return;
        }
        *self.selection.borrow_mut() = sel.clone();
        self.sync_settings_button();
        match sel {
            Some(k) if k.starts_with("settings:") => {
                *self.editor.borrow_mut() = None;
                self.syncing.set(true);
                self.list.unselect_all();
                self.syncing.set(false);
                self.show_settings_page(&k);
            }
            Some(k) => {
                if let Some(s) = self.settings.borrow().as_ref() {
                    s.set_visible(false);
                }
                let entry = self.entries.borrow().iter().find(|e| key(e) == k).cloned();
                match entry {
                    Some(e) => {
                        self.open_editor(e);
                        self.rebuild_list();
                    }
                    None => self.show_empty(),
                }
            }
            None => {
                if let Some(s) = self.settings.borrow().as_ref() {
                    s.set_visible(false);
                }
                *self.editor.borrow_mut() = None;
                self.show_empty();
                self.rebuild_list();
            }
        }
        self.split.set_show_content(true);
    }

    fn sync_settings_button(&self) {
        self.syncing.set(true);
        self.settings_button.set_active(self.is_settings());
        self.syncing.set(false);
    }

    fn clear_header(&self) {
        while let Some(c) = self.header_end.first_child() {
            self.header_end.remove(&c);
        }
        self.content_header.set_title_widget(Some(&self.content_title));
    }

    fn show_empty(&self) {
        self.clear_header();
        self.content_title.set_title("Qantrip");
        self.content_title.set_subtitle("");
        self.content_stack.set_visible_child_name("empty");
        self.lib_buttons.set_sensitive(true);
    }

    fn show_settings_page(self: &Rc<Self>, key: &str) {
        let page = key.trim_start_matches("settings:");
        self.clear_header();
        let existing = self.settings.borrow().clone();
        let view = match existing {
            Some(v) => v,
            None => {
                let v = SettingsView::new(self.app.clone(), self.window.clone().upcast());
                if let Some(old) = self.content_stack.child_by_name("settings") {
                    self.content_stack.remove(&old);
                }
                self.content_stack.add_named(v.widget(), Some("settings"));
                *self.settings.borrow_mut() = Some(v.clone());
                v
            }
        };
        view.select(page);
        view.set_visible(true);
        // 设置页：标题栏中间放分页切换
        self.content_header.set_title_widget(Some(view.switcher()));
        self.content_stack.set_visible_child_name("settings");
        self.lib_buttons.set_sensitive(false);
        self.window.set_title(Some(&l("Qantrip 设置", "Qantrip Settings")));
    }

    fn open_editor(self: &Rc<Self>, entry: TemplateEntry) {
        let ed = Editor::new(self.app.clone(), Rc::downgrade(self), entry);
        if let Some(old) = self.content_stack.child_by_name("editor") {
            self.content_stack.remove(&old);
        }
        self.content_stack.add_named(ed.widget(), Some("editor"));
        self.content_stack.set_visible_child_name("editor");
        self.clear_header();
        for w in ed.header_widgets() {
            self.header_end.append(&w);
        }
        *self.editor.borrow_mut() = Some(ed.clone());
        self.update_title();
        self.lib_buttons.set_sensitive(true);
        self.window.set_title(Some(&l("Qantrip 模板库", "Qantrip Template Library")));
    }

    /// 编辑器状态变化时更新标题与副标题
    pub fn update_title(&self) {
        let Some(e) = self.editor.borrow().clone() else { return };
        self.content_title.set_title(&e.display_name());
        self.content_title.set_subtitle(&e.subtitle());
    }

    // MARK: 文件操作

    fn dir(&self) -> PathBuf {
        paths::templates_dir()
    }

    pub fn file_for(&self, name: &str) -> PathBuf {
        self.dir().join(format!("{name}.md"))
    }

    pub fn user_entry(&self, name: &str) -> Option<TemplateEntry> {
        self.entries.borrow().iter().find(|e| e.name == name && !e.builtin).cloned()
    }

    pub fn unique_name(&self, base: &str) -> String {
        let mut n = base.to_string();
        let mut i = 2;
        while self.entries.borrow().iter().any(|e| e.name == n) || self.file_for(&n).exists() {
            n = format!("{base}-{i}");
            i += 1;
        }
        n
    }

    /// 写入模板文件；改名时旧文件移到回收站
    pub fn write(&self, source: &str, name: &str, replacing: Option<&str>) -> std::io::Result<PathBuf> {
        let path = self.file_for(name);
        paths::write_atomic(&path, source.as_bytes())?;
        if let Some(old) = replacing
            && Path::new(old) != path
            && Path::new(old).exists()
        {
            trash(Path::new(old));
        }
        Ok(path)
    }

    fn new_template(self: &Rc<Self>) {
        let name = self.unique_name("my-template");
        let source = qantrip_core::serialize_template_doc(qantrip_core::new_template_doc(name.clone(), self.app.settings.ui_locale()));
        let _ = self.write(&source, &name, None);
        self.reload();
        self.select(Some(name));
    }

    pub fn duplicate(self: &Rc<Self>, e: &TemplateEntry) {
        let Ok(mut doc) = qantrip_core::parse_template_doc(e.source.clone()) else { return };
        doc.name = self.unique_name(&if e.partial { e.name.clone() } else { format!("{}-copy", e.name) });
        doc.aliases.clear();
        let name = doc.name.clone();
        let _ = self.write(&qantrip_core::serialize_template_doc(doc), &name, None);
        self.reload();
        self.select(Some(name));
    }

    /// 自定义内置模板：原样复制源码到用户目录，覆盖内置版本
    pub fn customize(self: &Rc<Self>, e: &TemplateEntry) {
        let _ = self.write(&e.source, &e.name, None);
        self.reload();
        *self.selection.borrow_mut() = None;
        self.select(Some(e.name.clone()));
    }

    /// 删除模板（docs/decisions.md D34）：用户模板移到回收站；内置模板（含修改过的）删除后可以恢复
    pub fn delete(self: &Rc<Self>, e: &TemplateEntry) {
        let name = e.name.clone();
        let (title, body) = if e.builtin || e.overrides_builtin {
            (
                t!("删除内置模板「{name}」？", "Delete the built-in template “{name}”?"),
                if e.overrides_builtin {
                    l(
                        "你的修改会移到回收站。之后可以在“已删除”中恢复内置版本，它的名称和别名在此期间可以给你自己的模板使用。",
                        "Your changes go to the Trash. You can restore the built-in version from “Deleted” later; meanwhile its name and aliases are free for your own templates.",
                    )
                } else {
                    l(
                        "之后可以在“已删除”中恢复。它的名称和别名在此期间可以给你自己的模板使用。",
                        "You can restore it from “Deleted” later. Meanwhile its name and aliases are free for your own templates.",
                    )
                },
            )
        } else {
            (t!("删除模板「{name}」？", "Delete template “{name}”?"), l("文件会被移到回收站。", "The file will be moved to the Trash."))
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let d = adw::AlertDialog::new(Some(&title), Some(&body));
            d.add_response("cancel", &l("取消", "Cancel"));
            d.add_response("delete", &l("删除", "Delete"));
            d.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            if d.choose_future(Some(&this.window)).await != "delete" {
                return;
            }
            let outcome = this.app.engine.delete_template(name).ok();
            if let Some(e) = this.editor.borrow().as_ref() {
                e.set_dirty(false);
            }
            this.apply_outcome(outcome);
            *this.selection.borrow_mut() = None;
            *this.editor.borrow_mut() = None;
            this.show_empty();
            this.rebuild_list();
        });
    }

    /// 恢复内置模板：撤销删除，或丢弃修改（修改版本移到回收站）
    pub fn restore(self: &Rc<Self>, e: &TemplateEntry) {
        let name = e.name.clone();
        let ask = e.overrides_builtin;
        let this = self.clone();
        glib::spawn_future_local(async move {
            if ask {
                let d = adw::AlertDialog::new(
                    Some(&t!("恢复「{name}」为内置版本？", "Restore “{name}” to the built-in version?")),
                    Some(&l("你的修改会移到回收站。", "Your changes will be moved to the Trash.")),
                );
                d.add_response("cancel", &l("取消", "Cancel"));
                d.add_response("restore", &l("恢复内置", "Restore"));
                if d.choose_future(Some(&this.window)).await != "restore" {
                    return;
                }
            }
            let outcome = this.app.engine.restore_template(name.clone()).ok();
            if let Some(e) = this.editor.borrow().as_ref() {
                e.set_dirty(false);
            }
            this.apply_outcome(outcome);
            *this.selection.borrow_mut() = None;
            this.select(Some(name));
        });
    }

    fn apply_outcome(self: &Rc<Self>, outcome: Option<CommandOutcome>) {
        if let Some(path) = outcome.and_then(|o| o.trash_path) {
            trash(Path::new(&path));
        }
        self.reload();
    }

    // MARK: 导入导出

    fn export_one(self: &Rc<Self>, e: &TemplateEntry) {
        let dialog = gtk::FileDialog::builder().initial_name(format!("{}.md", e.name)).build();
        let source = e.source.clone();
        dialog.save(Some(&self.window), gio::Cancellable::NONE, move |r| {
            if let Ok(f) = r
                && let Some(p) = f.path()
            {
                let _ = std::fs::write(p, source.as_bytes());
            }
        });
    }

    fn export_all(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder().initial_name("qantrip-templates.zip").build();
        let dir = self.dir();
        let weak = Rc::downgrade(self);
        dialog.save(Some(&self.window), gio::Cancellable::NONE, move |r| {
            let Ok(f) = r else { return };
            let Some(p) = f.path() else { return };
            if let Err(e) = zip_dir(&dir, &p)
                && let Some(w) = weak.upgrade()
            {
                let d = adw::AlertDialog::new(Some(&l("导出失败", "Export failed")), Some(&e.to_string()));
                d.add_response("ok", "OK");
                d.present(Some(&w.window));
            }
        });
    }

    fn import_files(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&l("模板文件（.md）或 zip", "Templates (.md) or zip")));
        filter.add_suffix("md");
        filter.add_suffix("zip");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder().title(l("选择模板文件（.md）或 zip", "Choose template files (.md) or zip archives")).filters(&filters).build();
        let weak = Rc::downgrade(self);
        dialog.open_multiple(Some(&self.window), gio::Cancellable::NONE, move |r| {
            let Ok(list) = r else { return };
            let paths: Vec<PathBuf> = (0..list.n_items()).filter_map(|i| list.item(i).and_downcast::<gio::File>().and_then(|f| f.path())).collect();
            if let Some(w) = weak.upgrade() {
                w.import_paths(paths);
            }
        });
    }

    fn import_folder(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder().title(l("选择包含模板的文件夹", "Choose a folder with templates")).build();
        let weak = Rc::downgrade(self);
        dialog.select_folder(Some(&self.window), gio::Cancellable::NONE, move |r| {
            if let (Ok(f), Some(w)) = (r, weak.upgrade())
                && let Some(p) = f.path()
            {
                w.import_paths(vec![p]);
            }
        });
    }

    pub fn import_paths(self: &Rc<Self>, paths: Vec<PathBuf>) {
        let this = self.clone();
        glib::spawn_future_local(async move { this.import_async(paths).await });
    }

    async fn import_async(self: Rc<Self>, paths: Vec<PathBuf>) {
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut failures: Vec<String> = Vec::new();
        for p in paths {
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
                match unzip_md(&p) {
                    Ok(list) => files.extend(list),
                    Err(_) => failures.push(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                }
            } else if p.is_dir() {
                collect_md(&p, &mut files);
            } else if let Ok(b) = std::fs::read(&p) {
                files.push((p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), b));
            }
        }
        let (mut imported, mut skipped) = (0, 0);
        let mut sticky: Option<String> = None;
        for (fname, bytes) in files {
            let Ok(source) = String::from_utf8(bytes) else {
                failures.push(fname);
                continue;
            };
            let Ok(mut doc) = qantrip_core::parse_template_doc(source.clone()) else {
                failures.push(fname);
                continue;
            };
            let mut text = source;
            if self.entries.borrow().iter().any(|e| e.name == doc.name && !e.builtin) {
                let choice = match &sticky {
                    Some(c) => c.clone(),
                    None => {
                        let (c, apply_all) = self.ask_conflict(&doc.name).await;
                        if apply_all {
                            sticky = Some(c.clone());
                        }
                        c
                    }
                };
                match choice.as_str() {
                    "replace" => {}
                    "copy" => {
                        doc.name = self.unique_name(&doc.name);
                        text = qantrip_core::serialize_template_doc(doc.clone());
                    }
                    _ => {
                        skipped += 1;
                        continue;
                    }
                }
            }
            if self.write(&text, &doc.name, None).is_ok() {
                imported += 1;
                self.reload();
            } else {
                failures.push(fname);
            }
        }
        let mut info = Vec::new();
        if skipped > 0 {
            info.push(t!("跳过 {skipped} 个", "{skipped} skipped"));
        }
        if !failures.is_empty() {
            let list = failures.join(if crate::i18n::is_zh() { "、" } else { ", " });
            info.push(t!("无法导入：{list}", "Could not import: {list}"));
        }
        let d = adw::AlertDialog::new(Some(&t!("导入完成：{imported} 个", "Import complete: {imported}")), Some(&info.join("\n")));
        d.add_response("ok", "OK");
        d.present(Some(&self.window));
    }

    /// 重名：覆盖 / 重命名导入 / 跳过，可"对其余冲突执行相同操作"
    async fn ask_conflict(&self, name: &str) -> (String, bool) {
        let d = adw::AlertDialog::new(Some(&t!("已存在名为「{name}」的模板", "A template named “{name}” already exists")), None);
        d.add_response("skip", &l("跳过", "Skip"));
        d.add_response("copy", &l("重命名导入", "Import as Copy"));
        d.add_response("replace", &l("覆盖", "Replace"));
        d.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        let check = gtk::CheckButton::with_label(&l("对其余冲突执行相同操作", "Apply to remaining conflicts"));
        d.set_extra_child(Some(&check));
        let r = d.choose_future(Some(&self.window)).await;
        (r.to_string(), check.is_active())
    }
}

fn collect_md(dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "__MACOSX") {
                continue;
            }
            collect_md(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md"))
            && let Ok(b) = std::fs::read(&p)
        {
            out.push((p.file_name().unwrap().to_string_lossy().into_owned(), b));
        }
    }
}

fn unzip_md(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for i in 0..z.len() {
        let mut file = z.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().map(|c| c.to_string()).unwrap_or_default();
        if !name.to_lowercase().ends_with(".md") || name.contains("__MACOSX") || file.is_dir() {
            continue;
        }
        // 限制单个文件大小，避免异常的压缩包
        if file.size() > 4 * 1024 * 1024 {
            continue;
        }
        let mut buf = Vec::new();
        use std::io::Read;
        file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        out.push((base, buf));
    }
    Ok(out)
}

/// 导出为 zip，内容放在 `templates/` 下（与 macOS 的导出格式一致）
fn zip_dir(dir: &Path, out: &Path) -> Result<(), String> {
    use std::io::Write;
    let f = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut names: Vec<_> = std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    names.sort();
    for p in names {
        let name = format!("templates/{}", p.file_name().unwrap().to_string_lossy());
        z.start_file(name, opts).map_err(|e| e.to_string())?;
        z.write_all(&std::fs::read(&p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_roundtrip_keeps_templates_folder() {
        let base = std::env::temp_dir().join(format!("qantrip-zip-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("templates");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "---\nname: a\n---\nA").unwrap();
        std::fs::write(dir.join("b.md"), "---\nname: b\n---\n中文").unwrap();
        let zip = base.join("out.zip");
        zip_dir(&dir, &zip).unwrap();
        let mut files = unzip_md(&zip).unwrap();
        files.sort();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "a.md");
        assert_eq!(String::from_utf8(files[1].1.clone()).unwrap(), "---\nname: b\n---\n中文");
        // 文件夹导入：递归收集 .md，忽略 __MACOSX
        std::fs::create_dir_all(base.join("templates/__MACOSX")).unwrap();
        std::fs::write(base.join("templates/__MACOSX/c.md"), "x").unwrap();
        let mut out = Vec::new();
        collect_md(&dir, &mut out);
        assert_eq!(out.len(), 2);
        let _ = std::fs::remove_dir_all(&base);
    }
}
