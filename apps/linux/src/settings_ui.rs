//! 设置页（与模板库合在同一个窗口）：通用 / 插入 / 模板文件 / 同步 / 隐私 / 关于。
//! 与 macOS 的 SettingsView.swift 对应；Linux 特有的部分：快捷键的注册方式（门户 / X11 / 系统自定义快捷键）、
//! Wayland 自动粘贴授权、粘贴按键、排除的应用在各桌面上的可用性说明。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::glib::translate::IntoGlib;

use crate::app::App;
use crate::autopaste::{Method, PortalState};
use crate::foreground::Source;
use crate::hotkey::{self, Mode, State};
use crate::i18n::l;
use crate::session::Desktop;
use crate::settings::{self, ExcludedApp, LanguageChoice, PasteKey};
use crate::sync_ui::SyncPage;
use crate::t;
use crate::update;

pub const PAGES: [&str; 6] = ["general", "insert", "templates", "sync", "privacy", "about"];

fn page_title(p: &str) -> String {
    match p {
        "general" => l("通用", "General"),
        "insert" => l("插入", "Insert"),
        "templates" => l("模板文件", "Template Files"),
        "sync" => l("同步", "Sync"),
        "privacy" => l("隐私", "Privacy"),
        _ => l("关于", "About"),
    }
}

fn page_icon(p: &str) -> &'static str {
    match p {
        "general" => "qantrip-settings-symbolic",
        "insert" => "edit-paste-symbolic",
        "templates" => "folder-symbolic",
        "sync" => "emblem-synchronizing-symbolic",
        "privacy" => "security-high-symbolic",
        _ => "help-about-symbolic",
    }
}

/// 可重建的区块：状态变化时只重建这一块，不影响页面其他控件的焦点
struct Slot {
    group: adw::PreferencesGroup,
    rows: RefCell<Vec<gtk::Widget>>,
}

impl Slot {
    fn new(group: adw::PreferencesGroup) -> Rc<Slot> {
        Rc::new(Slot { group, rows: RefCell::new(Vec::new()) })
    }
    fn clear(&self) {
        for r in self.rows.borrow_mut().drain(..) {
            self.group.remove(&r);
        }
    }
    fn add(&self, w: &impl IsA<gtk::Widget>) {
        self.group.add(w);
        self.rows.borrow_mut().push(w.clone().upcast());
    }
}

fn caption(text: &str) -> gtk::Label {
    let lbl = gtk::Label::new(Some(text));
    lbl.set_wrap(true);
    lbl.set_xalign(0.0);
    lbl.add_css_class("caption");
    lbl.add_css_class("dim-label");
    lbl
}

pub struct SettingsView {
    app: Rc<App>,
    parent: gtk::Window,
    stack: adw::ViewStack,
    switcher: adw::ViewSwitcher,
    hotkey_slot: RefCell<Option<Rc<Slot>>>,
    excluded_slot: RefCell<Option<Rc<Slot>>>,
    paste_slot: RefCell<Option<Rc<Slot>>>,
    templates_slot: RefCell<Option<Rc<Slot>>>,
    update_box: gtk::Box,
    sync_page: RefCell<Option<Rc<SyncPage>>>,
    recording: Cell<bool>,
    recording_controller: RefCell<Option<gtk::EventControllerKey>>,
    hotkey_message: RefCell<Option<(String, bool)>>,
    hotkey_change: Cell<u64>,
}

impl SettingsView {
    pub fn new(app: Rc<App>, parent: gtk::Window) -> Rc<SettingsView> {
        let stack = adw::ViewStack::new();
        let switcher = adw::ViewSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
        let v = Rc::new(SettingsView {
            app: app.clone(),
            parent,
            stack,
            switcher,
            hotkey_slot: RefCell::new(None),
            excluded_slot: RefCell::new(None),
            paste_slot: RefCell::new(None),
            templates_slot: RefCell::new(None),
            update_box: gtk::Box::new(gtk::Orientation::Vertical, 8),
            sync_page: RefCell::new(None),
            recording: Cell::new(false),
            recording_controller: RefCell::new(None),
            hotkey_message: RefCell::new(None),
            hotkey_change: Cell::new(0),
        });
        for p in PAGES {
            let w: gtk::Widget = match p {
                "general" => v.general_page().upcast(),
                "insert" => v.insert_page().upcast(),
                "templates" => v.templates_page().upcast(),
                "sync" => {
                    let s = SyncPage::new(app.clone(), v.parent.clone());
                    let w = s.widget().clone().upcast();
                    *v.sync_page.borrow_mut() = Some(s);
                    w
                }
                "privacy" => v.privacy_page().upcast(),
                _ => v.about_page().upcast(),
            };
            v.stack.add_titled_with_icon(&w, Some(p), &page_title(p), page_icon(p));
        }
        let weak = Rc::downgrade(&v);
        app.connect_changed(move || {
            if let Some(v) = weak.upgrade() {
                v.refresh();
            }
        });
        let weak = Rc::downgrade(&v);
        v.stack.connect_visible_child_name_notify(move |s| {
            if let Some(v) = weak.upgrade() {
                if s.visible_child_name().as_deref() != Some("general") && v.recording.get() {
                    v.finish_recording(None);
                }
                if let Some(sp) = v.sync_page.borrow().as_ref() {
                    sp.set_visible(s.visible_child_name().as_deref() == Some("sync") && v.parent.is_visible());
                }
            }
        });
        // Closing or hiding Settings while recording must restore the old grab.
        let weak = Rc::downgrade(&v);
        v.parent.connect_visible_notify(move |window| {
            if !window.is_visible()
                && let Some(view) = weak.upgrade()
                && view.recording.get()
            {
                view.finish_recording(None);
            }
        });
        v.refresh();
        v
    }

    pub fn widget(&self) -> &adw::ViewStack {
        &self.stack
    }

    pub fn switcher(&self) -> &adw::ViewSwitcher {
        &self.switcher
    }

    pub fn select(&self, page: &str) {
        let p = if PAGES.contains(&page) { page } else { "general" };
        self.stack.set_visible_child_name(p);
        if let Some(sp) = self.sync_page.borrow().as_ref() {
            sp.set_visible(p == "sync");
        }
    }

    /// 设置页可见（同步页据此轮询加入请求）
    pub fn set_visible(self: &Rc<Self>, visible: bool) {
        if !visible && self.recording.get() {
            self.finish_recording(None);
        }
        if let Some(sp) = self.sync_page.borrow().as_ref() {
            sp.set_visible(visible && self.stack.visible_child_name().as_deref() == Some("sync"));
        }
    }

    fn refresh(self: &Rc<Self>) {
        self.fill_hotkey();
        self.fill_excluded();
        self.fill_paste();
        self.fill_templates();
        self.fill_update();
    }

    fn page() -> adw::PreferencesPage {
        adw::PreferencesPage::new()
    }

    // MARK: 通用

    fn general_page(self: &Rc<Self>) -> adw::PreferencesPage {
        let app = &self.app;
        let page = Self::page();
        let hk = adw::PreferencesGroup::builder().title(l("唤起快捷键", "Panel shortcut")).build();
        *self.hotkey_slot.borrow_mut() = Some(Slot::new(hk.clone()));
        page.add(&hk);

        let ex = adw::PreferencesGroup::builder().title(l("排除的应用", "Excluded apps")).build();
        *self.excluded_slot.borrow_mut() = Some(Slot::new(ex.clone()));
        page.add(&ex);

        // 语言
        let lang = adw::PreferencesGroup::builder()
            .title(l("语言", "Language"))
            .description(l("你自己的模板不受影响。", "Your own templates are not affected."))
            .build();
        let choices = |auto: String| gtk::StringList::new(&[auto.as_str(), "中文", "English"]);
        let idx = |c: LanguageChoice| match c {
            LanguageChoice::Auto => 0,
            LanguageChoice::Zh => 1,
            LanguageChoice::En => 2,
        };
        let code = |i: u32| ["auto", "zh", "en"][i.min(2) as usize];
        let ui = adw::ComboRow::builder()
            .title(l("界面语言", "Language"))
            .model(&choices(l("跟随系统", "System default")))
            .selected(idx(app.settings.interface_language()))
            .build();
        let s = app.settings.clone();
        ui.connect_selected_notify(move |c| s.set_string("interfaceLanguage", Some(code(c.selected()))));
        lang.add(&ui);
        let tpl = adw::ComboRow::builder()
            .title(l("内置模板语言", "Built-in templates"))
            .model(&choices(l("跟随界面语言", "Same as language")))
            .selected(idx(app.settings.template_language()))
            .build();
        let s = app.settings.clone();
        tpl.connect_selected_notify(move |c| s.set_string("templateLanguage", Some(code(c.selected()))));
        lang.add(&tpl);
        page.add(&lang);

        // 启动与输入法
        let misc = adw::PreferencesGroup::builder()
            .description(l(
                "中文输入法下唤起时自动切到英文，关闭面板后恢复原输入法（支持 Fcitx5 与 IBus）。输入中文附加要求时手动切换即可。",
                "When opened with a Chinese input method, it switches to English and is restored when the panel closes (Fcitx5 and IBus). Switch manually to type extra instructions in Chinese.",
            ))
            .build();
        let login = adw::SwitchRow::builder().title(l("登录时启动", "Launch at login")).active(crate::autostart::is_enabled()).build();
        let parent = self.parent.clone();
        login.connect_active_notify(move |r| {
            if let Err(e) = crate::autostart::set_enabled(r.is_active()) {
                let d = adw::AlertDialog::new(Some(&l("无法设置登录时启动", "Couldn't change launch at login")), Some(&e.to_string()));
                d.add_response("ok", "OK");
                d.present(Some(&parent));
            }
        });
        misc.add(&login);
        let ascii = adw::SwitchRow::builder()
            .title(l("唤起时切换到英文输入法", "Switch to English input when opened"))
            .active(app.settings.force_ascii_input())
            .build();
        let s = app.settings.clone();
        ascii.connect_active_notify(move |r| s.set_bool("forceASCIIInput", r.is_active()));
        misc.add(&ascii);
        page.add(&misc);
        page
    }

    fn fill_hotkey(self: &Rc<Self>) {
        let Some(slot) = self.hotkey_slot.borrow().clone() else { return };
        if self.recording.get() {
            return;
        }
        slot.clear();
        let hk = self.app.hotkey.clone();
        let label = hk.display();
        match hk.mode() {
            Mode::X11 => {
                let row = adw::ActionRow::builder().title(l("唤起快捷键", "Panel shortcut")).build();
                let btn = gtk::Button::with_label(&label);
                btn.set_sensitive(!matches!(hk.state(), State::Pending));
                btn.add_css_class("mono");
                btn.set_valign(gtk::Align::Center);
                let weak = Rc::downgrade(self);
                btn.connect_clicked(move |b| {
                    if let Some(v) = weak.upgrade() {
                        v.record(b);
                    }
                });
                if hotkey::parse_accel(&hk.accel()) != hotkey::parse_accel(settings::DEFAULT_HOTKEY) {
                    let def = gtk::Button::with_label(&l("恢复默认", "Restore default"));
                    def.set_sensitive(!matches!(hk.state(), State::Pending));
                    def.add_css_class("flat");
                    def.set_valign(gtk::Align::Center);
                    let weak = Rc::downgrade(self);
                    def.connect_clicked(move |_| {
                        if let Some(v) = weak.upgrade() {
                            v.apply_hotkey(settings::DEFAULT_HOTKEY);
                        }
                    });
                    row.add_suffix(&def);
                }
                row.add_suffix(&btn);
                let sub = match (self.hotkey_message.borrow().clone(), hk.state()) {
                    (_, State::Pending) => l("正在注册快捷键…", "Registering the shortcut…"),
                    (Some((m, _)), State::Failed { reason }) => {
                        t!("{m}\n当前快捷键仍未注册：{reason}", "{m}\nThe current shortcut is still not registered: {reason}")
                    }
                    (Some((m, _)), _) => m,
                    (None, State::Failed { reason }) => t!("当前快捷键注册失败：{reason}", "Couldn't register the current shortcut: {reason}"),
                    (None, State::Paused { .. }) => l("已暂停，按键交给前台应用。", "Paused; keys are passed to the foreground app."),
                    _ => l("点击右侧按键可以修改，按 Esc 取消。", "Click the shortcut to change it; Esc cancels."),
                };
                row.set_subtitle(&sub);
                if matches!(hk.state(), State::Failed { .. }) {
                    let retry = gtk::Button::with_label(&l("重试", "Retry"));
                    retry.set_valign(gtk::Align::Center);
                    let h = hk.clone();
                    retry.connect_clicked(move |_| h.retry());
                    row.add_suffix(&retry);
                }
                slot.add(&row);
            }
            Mode::Portal => {
                let row = adw::ActionRow::builder().title(l("唤起快捷键", "Panel shortcut")).build();
                let (sub, retry) = match hk.state() {
                    State::Active { .. } => (
                        t!(
                            "已由系统注册为 {label}。修改快捷键请在系统设置中进行。",
                            "Registered with the system as {label}. Change it in your system settings."
                        ),
                        false,
                    ),
                    State::Pending => (l("正在检测快捷键服务…", "Checking the shortcut service…"), false),
                    State::AwaitingPermission => (l("等待系统授权（请留意系统对话框）…", "Waiting for authorization (watch for a system dialog)…"), false),
                    State::Paused { .. } => {
                        (l("已暂停唤起；系统仍可能拦截此组合键。", "Activation is paused; the system may still intercept this shortcut."), false)
                    }
                    State::Failed { reason } => (
                        t!(
                            "{reason}。可以重试，或在系统设置中添加自定义快捷键，命令为 qantrip toggle。",
                            "{reason}. Try again, or add a custom shortcut in system settings that runs qantrip toggle."
                        ),
                        true,
                    ),
                    State::Manual | State::Closed => (String::new(), false),
                };
                row.set_subtitle(&sub);
                let k = gtk::Label::new(Some(&label));
                k.add_css_class("mono");
                row.add_suffix(&k);
                if retry {
                    let b = gtk::Button::with_label(&l("重试", "Retry"));
                    b.set_valign(gtk::Align::Center);
                    let h = hk.clone();
                    b.connect_clicked(move |_| h.retry());
                    row.add_suffix(&b);
                } else if hk.state().registered() {
                    let b = gtk::Button::with_label(&l("在系统设置中修改…", "Change in System Settings…"));
                    b.set_valign(gtk::Align::Center);
                    let h = hk.clone();
                    let desktop = self.app.session.desktop;
                    b.connect_clicked(move |_| {
                        if !h.configure() {
                            open_system_shortcuts(desktop);
                        }
                    });
                    row.add_suffix(&b);
                }
                slot.add(&row);
                slot.add(&self.manual_row(false));
            }
            Mode::Manual => {
                slot.add(&self.manual_row(true));
            }
        }
        let note = caption(&l(
            "Linux 默认是 Ctrl+Shift+Q，可以修改为其他组合。请先确认注册成功；未注册时，按键仍由当前应用处理，可能触发退出等操作。",
            "The Linux default is Ctrl+Shift+Q. You can choose another combination. Confirm successful registration first; otherwise the foreground app handles the keys and may quit or perform another action.",
        ));
        note.set_margin_top(6);
        slot.add(&note);
    }

    /// 系统自定义快捷键说明：绑定 `qantrip toggle`
    fn manual_row(self: &Rc<Self>, primary: bool) -> adw::ActionRow {
        if let Some((config, hint)) = crate::hotkey::compositor_binding_hint(&self.app.session) {
            let row = adw::ActionRow::builder()
                .title(l("在合成器配置中绑定快捷键", "Bind a shortcut in compositor configuration"))
                .subtitle(hint)
                .subtitle_selectable(true)
                .build();
            let copy = gtk::Button::with_label(&l("复制配置", "Copy configuration"));
            copy.set_valign(gtk::Align::Center);
            copy.connect_clicked(move |_| crate::insert::copy_plain(&config));
            row.add_suffix(&copy);
            return row;
        }
        let cmd = hotkey::manual_command();
        let row = adw::ActionRow::builder()
            .title(if primary {
                l("在系统设置中绑定快捷键", "Bind a shortcut in system settings")
            } else {
                l("也可以使用系统自定义快捷键", "Or use a system custom shortcut")
            })
            .subtitle(t!(
                "这个桌面不支持应用直接注册全局快捷键时，在系统设置 › 键盘 › 自定义快捷键中添加一个快捷键（如 Ctrl+Shift+Q），命令填写：{cmd}",
                "If your desktop doesn't let apps register global shortcuts, add a custom shortcut (e.g. Ctrl+Shift+Q) in system settings › Keyboard that runs: {cmd}"
            ))
            .subtitle_selectable(true)
            .build();
        let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
        copy.set_tooltip_text(Some(&l("复制命令", "Copy command")));
        copy.set_valign(gtk::Align::Center);
        copy.add_css_class("flat");
        copy.connect_clicked(move |_| crate::insert::copy_plain(&cmd));
        row.add_suffix(&copy);
        if primary {
            let open = gtk::Button::with_label(&l("打开键盘设置…", "Open Keyboard Settings…"));
            open.set_valign(gtk::Align::Center);
            let desktop = self.app.session.desktop;
            open.connect_clicked(move |_| open_system_shortcuts(desktop));
            row.add_suffix(&open);
        }
        row
    }

    /// 录制新快捷键（X11）：按下新的组合键，Esc 取消
    fn record(self: &Rc<Self>, button: &gtk::Button) {
        if self.recording.replace(true) {
            return;
        }
        *self.hotkey_message.borrow_mut() = None;
        self.app.hotkey.suspend();
        button.set_label(&l("按下新的组合键…", "Press new shortcut…"));
        button.add_css_class("suggested-action");
        let ctl = gtk::EventControllerKey::new();
        ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        ctl.connect_key_pressed(move |_, key, _, mods| {
            let Some(v) = weak.upgrade() else { return glib::Propagation::Proceed };
            if key == gdk::Key::Escape {
                v.finish_recording(None);
                return glib::Propagation::Stop;
            }
            // 只按了修饰键：继续等待
            if matches!(
                key,
                gdk::Key::Control_L
                    | gdk::Key::Control_R
                    | gdk::Key::Shift_L
                    | gdk::Key::Shift_R
                    | gdk::Key::Alt_L
                    | gdk::Key::Alt_R
                    | gdk::Key::Super_L
                    | gdk::Key::Super_R
                    | gdk::Key::Meta_L
                    | gdk::Key::Meta_R
                    | gdk::Key::ISO_Level3_Shift
            ) {
                return glib::Propagation::Stop;
            }
            let mods = mods & (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK | gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK);
            let k = key.into_glib();
            let is_fn = (gdk::Key::F1.into_glib()..=gdk::Key::F35.into_glib()).contains(&k);
            if mods.is_empty() && !is_fn {
                *v.hotkey_message.borrow_mut() =
                    Some((l("需要包含至少一个修饰键（Ctrl、Alt、Shift、Super）", "Include at least one modifier key (Ctrl, Alt, Shift, Super)"), true));
                v.finish_recording(None);
                return glib::Propagation::Stop;
            }
            let accel = gtk::accelerator_name(key.to_lower(), mods).to_string();
            v.finish_recording(Some(accel));
            glib::Propagation::Stop
        });
        *self.recording_controller.borrow_mut() = Some(ctl.clone());
        self.parent.add_controller(ctl);
    }

    fn finish_recording(self: &Rc<Self>, accel: Option<String>) {
        self.recording.set(false);
        if let Some(controller) = self.recording_controller.borrow_mut().take() {
            self.parent.remove_controller(&controller);
        }
        match accel {
            Some(a) => self.apply_hotkey(&a),
            None => {
                self.app.hotkey.resume();
                self.fill_hotkey();
            }
        }
    }

    fn apply_hotkey(self: &Rc<Self>, accel: &str) {
        let change = self.hotkey_change.get() + 1;
        self.hotkey_change.set(change);
        let label = hotkey::accel_label(accel);
        let accel = accel.to_owned();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = this.app.hotkey.try_register(&accel).await;
            if this.hotkey_change.get() != change {
                return;
            }
            match result {
                Ok(()) => {
                    this.app.settings.set_string("hotKey", Some(&accel));
                    *this.hotkey_message.borrow_mut() = Some((t!("已设置为 {label}", "Set to {label}"), false));
                }
                Err(reason) => {
                    *this.hotkey_message.borrow_mut() = Some((
                        t!("无法设置 {label}：{reason}。保留原来的快捷键设置。", "Couldn't set {label}: {reason}. The previous shortcut setting is kept."),
                        true,
                    ));
                }
            }
            this.app.hotkey.resume();
            this.fill_hotkey();
            this.app.changed();
        });
    }

    fn fill_excluded(self: &Rc<Self>) {
        let Some(slot) = self.excluded_slot.borrow().clone() else { return };
        slot.clear();
        let app = self.app.clone();
        let list = app.settings.excluded_apps();
        for a in &list {
            let row = adw::ActionRow::builder().title(&a.name).subtitle(&a.id).build();
            let rm = gtk::Button::with_label(&l("移除", "Remove"));
            rm.add_css_class("flat");
            rm.set_valign(gtk::Align::Center);
            let s = app.settings.clone();
            let id = a.id.clone();
            rm.connect_clicked(move |_| {
                let mut l = s.excluded_apps();
                l.retain(|x| x.id != id);
                s.set_excluded_apps(&l);
            });
            row.add_suffix(&rm);
            slot.add(&row);
        }
        // 添加：最近用过的应用，或手动输入窗口类名 / app_id
        let add_row = adw::ActionRow::builder().title(l("添加应用", "Add an app")).build();
        let menu = gtk::MenuButton::builder().label(l("最近使用…", "Recent…")).valign(gtk::Align::Center).build();
        let pop = gtk::Popover::new();
        let bx = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let recent: Vec<_> = app.foreground.recent().into_iter().filter(|r| !app.settings.is_excluded(Some(&r.id))).collect();
        if recent.is_empty() {
            let lbl = gtk::Label::new(Some(&l("还没有记录到其他应用", "No apps seen yet")));
            lbl.add_css_class("dim-label");
            lbl.set_margin_start(8);
            lbl.set_margin_end(8);
            lbl.set_margin_top(6);
            lbl.set_margin_bottom(6);
            bx.append(&lbl);
        }
        for r in recent {
            let b = gtk::Button::with_label(&format!("{}  ({})", r.name, r.id));
            b.add_css_class("flat");
            let s = app.settings.clone();
            let p = pop.clone();
            b.connect_clicked(move |_| {
                p.popdown();
                let mut l = s.excluded_apps();
                l.push(ExcludedApp { id: r.id.clone(), name: r.name.clone() });
                s.set_excluded_apps(&l);
            });
            bx.append(&b);
        }
        pop.set_child(Some(&bx));
        menu.set_popover(Some(&pop));
        add_row.add_suffix(&menu);
        slot.add(&add_row);
        let entry = adw::EntryRow::builder()
            .title(l("或输入窗口类名 / app_id，回车添加", "Or type a window class / app_id and press Enter"))
            .show_apply_button(true)
            .build();
        let s = app.settings.clone();
        entry.connect_apply(move |e| {
            let id = e.text().trim().to_string();
            if id.is_empty() {
                return;
            }
            let mut l = s.excluded_apps();
            if !l.iter().any(|x| x.id.eq_ignore_ascii_case(&id)) {
                l.push(ExcludedApp { name: crate::foreground::pretty_name(&id, &id), id });
                s.set_excluded_apps(&l);
            }
        });
        slot.add(&entry);
        let how = match (app.session.is_x11(), app.foreground.source(), app.session.desktop) {
            (true, _, _) => l(
                "在这些应用里按快捷键不会唤起 Qantrip，按键照常交给应用本身（按窗口类名 WM_CLASS 识别）。也可以在托盘菜单中一键停用当前应用。",
                "In these apps the shortcut doesn't open Qantrip and reaches the app as usual (matched by WM_CLASS). You can also disable the current app from the tray menu.",
            ),
            (false, Source::KWin, _) => l(
                "在这些应用里按快捷键不会打开面板（按 app_id 识别，由 KWin 脚本提供）。Wayland 下快捷键由系统注册，按键不能原样交给应用。也可以在托盘菜单中一键停用当前应用。",
                "In these apps the shortcut doesn't open the panel (matched by app_id via a KWin script). On Wayland the system owns the shortcut, so the key isn't passed on to the app. You can also disable the current app from the tray menu.",
            ),
            (false, _, Desktop::Gnome) => l(
                "GNOME（Wayland）不向应用提供当前窗口的信息，排除列表在这里无法生效。需要时可以在 X11 会话中使用，或为该应用换一个唤起快捷键。",
                "GNOME on Wayland doesn't tell apps which window is active, so this list has no effect here. It works in X11 sessions; otherwise choose a different shortcut.",
            ),
            _ => l(
                "这个桌面不向应用提供当前窗口的信息，排除列表可能无法生效。",
                "This desktop doesn't tell apps which window is active, so this list may have no effect.",
            ),
        };
        let note = caption(&how);
        note.set_margin_top(6);
        slot.add(&note);
    }

    // MARK: 插入

    fn insert_page(self: &Rc<Self>) -> adw::PreferencesPage {
        let app = &self.app;
        let page = Self::page();
        let method = adw::PreferencesGroup::builder().title(l("插入方式", "Insert method")).build();
        let auto = gtk::CheckButton::new();
        let copy = gtk::CheckButton::new();
        copy.set_group(Some(&auto));
        if app.settings.copy_only() {
            copy.set_active(true)
        } else {
            auto.set_active(true)
        }
        let r1 = adw::ActionRow::builder().title(l("自动粘贴到当前应用", "Auto-paste into the current app")).activatable_widget(&auto).build();
        r1.add_prefix(&auto);
        let r2 = adw::ActionRow::builder()
            .title(l("只复制，手动粘贴", "Copy only, paste manually"))
            .subtitle(l("不需要任何系统权限", "Needs no system permission"))
            .activatable_widget(&copy)
            .build();
        r2.add_prefix(&copy);
        let s = app.settings.clone();
        copy.connect_toggled(move |c| s.set_bool("copyOnly", c.is_active()));
        method.add(&r1);
        method.add(&r2);
        page.add(&method);

        let paste = adw::PreferencesGroup::builder().title(l("自动粘贴", "Auto-paste")).build();
        *self.paste_slot.borrow_mut() = Some(Slot::new(paste.clone()));
        page.add(&paste);

        let keys = adw::PreferencesGroup::builder().build();
        let labels = [l("自动", "Automatic"), "Ctrl+V".into(), "Ctrl+Shift+V".into(), "Shift+Insert".into()];
        let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
        let cur = PasteKey::all().iter().position(|k| *k == app.settings.paste_key()).unwrap_or(0);
        let combo = adw::ComboRow::builder().title(l("粘贴按键", "Paste key")).model(&model).selected(cur as u32).build();
        let s = app.settings.clone();
        combo.connect_selected_notify(move |c| s.set_string("pasteKey", Some(PasteKey::all()[c.selected() as usize % 4].code())));
        keys.add(&combo);
        let note = caption(&l(
            "粘贴按键选“自动”时：终端（按窗口类名 / app_id 识别）用 Ctrl+Shift+V，其他应用用 Ctrl+V；识别不出当前应用时（如 GNOME Wayland）用 Shift+Insert，它在终端与普通应用中都能粘贴。\n自动粘贴把 Prompt 写入剪贴板后模拟粘贴按键。X11 下粘贴后剪贴板会恢复为原内容；Wayland 下应用失去焦点后不能再写剪贴板，Prompt 会留在剪贴板中。写入时附加 KDE 剪贴板历史的排除标记，Klipper 等不会记录。",
            "With Automatic, terminals (recognized by window class / app_id) get Ctrl+Shift+V and other apps Ctrl+V; when the current app can't be identified (e.g. GNOME on Wayland), Shift+Insert is used, which pastes in both terminals and regular apps.\nAuto-paste writes the prompt to the clipboard and simulates the paste key. On X11 the clipboard is restored afterwards; on Wayland apps can't write the clipboard after losing focus, so the prompt stays on the clipboard. The prompt is marked so that Klipper and similar clipboard managers don't keep it in their history.",
        ));
        note.set_margin_top(6);
        keys.add(&note);
        page.add(&keys);

        let expand = adw::PreferencesGroup::builder()
            .title(l("Claude Code 展开粘贴", "Claude Code expand paste"))
            .description(l(
                "Claude Code 会把超过 800 字符或多于 3 行的粘贴折叠为 [Pasted text]，再次粘贴相同内容即可展开。Ctrl+↩ 只在内容会被折叠时才粘贴两次。",
                "Claude Code collapses pastes over 800 characters or more than 3 lines into [Pasted text]; pasting the same content again expands it. Ctrl+↩ pastes twice only when the content would be collapsed.",
            ))
            .build();
        let on = adw::SwitchRow::builder().title(l("Ctrl+↩ 展开粘贴", "Ctrl+↩ expand paste")).active(app.settings.expand_paste_enabled()).build();
        let s = app.settings.clone();
        on.connect_active_notify(move |r| s.set_bool("expandPasteEnabled", r.is_active()));
        expand.add(&on);
        let spin = adw::SpinRow::with_range(100.0, 800.0, 50.0);
        spin.set_title(&l("两次粘贴间隔（毫秒）", "Interval between pastes (ms)"));
        spin.set_value(app.settings.expand_paste_interval_ms());
        let s = app.settings.clone();
        spin.connect_value_notify(move |r| s.set_f64("expandPasteIntervalMs", r.value()));
        on.bind_property("active", &spin, "sensitive").sync_create().build();
        expand.add(&spin);
        page.add(&expand);
        page
    }

    fn fill_paste(self: &Rc<Self>) {
        let Some(slot) = self.paste_slot.borrow().clone() else { return };
        slot.clear();
        let ap = self.app.autopaste.clone();
        let row = adw::ActionRow::builder().title(l("状态", "Status")).build();
        if self.app.session.is_x11() {
            row.set_subtitle(&l("X11：直接模拟粘贴按键（XTest），不需要授权。", "X11: simulates the paste key directly (XTest); no permission needed."));
            row.add_suffix(&status_icon(true));
        } else {
            match ap.portal_state() {
                PortalState::Authorized => {
                    row.set_subtitle(&l(
                        "已授权（系统已记住，之后不再询问）。每次粘贴时系统会短暂显示远程控制指示。",
                        "Allowed (remembered by the system). The system briefly shows a remote-control indicator while pasting.",
                    ));
                    row.add_suffix(&status_icon(true));
                    let b = gtk::Button::with_label(&l("撤销", "Revoke"));
                    b.add_css_class("flat");
                    b.set_valign(gtk::Align::Center);
                    let a = ap.clone();
                    b.connect_clicked(move |_| a.forget());
                    row.add_suffix(&b);
                }
                PortalState::Requesting => {
                    row.set_subtitle(&l(
                        "请在系统对话框中允许“远程交互”并勾选“记住”…",
                        "Allow remote interaction in the system dialog and choose to remember it…",
                    ));
                    row.add_suffix(&gtk::Spinner::builder().spinning(true).build());
                }
                PortalState::NotAuthorized | PortalState::Unknown | PortalState::Denied(_) => {
                    let denied = matches!(ap.portal_state(), PortalState::Denied(_));
                    row.set_subtitle(&if denied {
                        l("没有授权。授权后 Qantrip 才能替你按下粘贴键；不授权时 Prompt 会复制到剪贴板。", "Not allowed. Qantrip needs this to press the paste key for you; otherwise prompts are copied to the clipboard.")
                    } else {
                        l("Wayland 下需要授权一次：Qantrip 通过系统的远程桌面接口模拟粘贴按键（只使用键盘，不读取屏幕）。不授权时 Prompt 会复制到剪贴板。", "On Wayland, allow this once: Qantrip uses the system's remote desktop interface to press the paste key (keyboard only, no screen access). Otherwise prompts are copied to the clipboard.")
                    });
                    row.add_suffix(&status_icon(false));
                    let b = gtk::Button::with_label(&l("授权自动粘贴…", "Allow Auto-paste…"));
                    b.add_css_class("suggested-action");
                    b.set_valign(gtk::Align::Center);
                    let a = ap.clone();
                    b.connect_clicked(move |_| a.authorize());
                    row.add_suffix(&b);
                }
                PortalState::Unavailable => {
                    if ap.method() == Method::Wtype {
                        row.set_subtitle(&l(
                            "使用 wtype 模拟粘贴按键（wlroots 桌面，尽力支持）。",
                            "Uses wtype to press the paste key (wlroots desktops, best effort).",
                        ));
                        row.add_suffix(&status_icon(true));
                    } else {
                        row.set_subtitle(&l("这个桌面不支持应用模拟按键（没有远程桌面门户）。Prompt 会复制到剪贴板，按 Ctrl+V 粘贴；wlroots 桌面可以安装 wtype。", "This desktop doesn't let apps simulate keys (no remote desktop portal). Prompts are copied to the clipboard; press Ctrl+V to paste. On wlroots desktops, install wtype."));
                        row.add_suffix(&status_icon(false));
                    }
                }
            }
        }
        slot.add(&row);
    }

    // MARK: 模板文件

    fn templates_page(self: &Rc<Self>) -> adw::PreferencesPage {
        let page = Self::page();
        let g = adw::PreferencesGroup::new();
        *self.templates_slot.borrow_mut() = Some(Slot::new(g.clone()));
        page.add(&g);
        page
    }

    fn fill_templates(self: &Rc<Self>) {
        let Some(slot) = self.templates_slot.borrow().clone() else { return };
        slot.clear();
        let app = self.app.clone();
        let count = app.engine.templates().len();
        slot.add(&adw::ActionRow::builder().title(l("模板数量", "Template count")).subtitle(count.to_string()).build());
        let dir = crate::paths::templates_dir();
        let row = adw::ActionRow::builder().title(l("模板目录", "Template folder")).subtitle(crate::paths::display(&dir)).subtitle_selectable(true).build();
        let open = gtk::Button::with_label(&l("在文件管理器中打开", "Open in Files"));
        open.set_valign(gtk::Align::Center);
        let parent = self.parent.clone();
        open.connect_clicked(move |_| {
            gtk::FileLauncher::new(Some(&gio::File::for_path(&dir))).launch(Some(&parent), gio::Cancellable::NONE, |_| {});
        });
        row.add_suffix(&open);
        slot.add(&row);
        let actions = adw::ActionRow::new();
        let lib = gtk::Button::with_label(&l("打开模板库…", "Open Template Library…"));
        let reload = gtk::Button::with_label(&l("重新加载", "Reload"));
        for b in [&lib, &reload] {
            b.set_valign(gtk::Align::Center);
            actions.add_suffix(b);
        }
        let a = app.clone();
        lib.connect_clicked(move |_| a.show_library(None));
        let a = app.clone();
        reload.connect_clicked(move |_| {
            a.engine.reload();
            a.library_reload();
            a.changed();
        });
        slot.add(&actions);
        for p in app.engine.problems() {
            let r = adw::ActionRow::builder().title(glib::markup_escape_text(&p).as_str()).build();
            r.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
            r.add_css_class("warning");
            slot.add(&r);
        }
    }

    // MARK: 隐私

    fn privacy_page(self: &Rc<Self>) -> adw::PreferencesPage {
        let app = &self.app;
        let page = Self::page();
        let g = adw::PreferencesGroup::builder()
            .description(l(
                "这是纯本地功能：用于首屏的「最近施放」和「常用」排序，以及按应用区分常用模板。只记录模板名、参数和使用时所在的应用，不记录附加要求和生成的 Prompt。记录保存在本机的 usage.json 中，不会上传到任何服务器，也不会随云同步传到其他设备；关闭后不再记录，清除后立即删除。",
                "This is local only: it orders “Recent” and “Frequent” on the start screen and tells which templates you use in which app. Only template names, parameters and the app you used them in are recorded — never extra instructions or generated prompts. The record lives in usage.json on this computer, is never uploaded to any server and is not part of sync. Turn it off to stop recording; Clear deletes it immediately.",
            ))
            .build();
        let rec = adw::SwitchRow::builder()
            .title(l("记录使用习惯（仅本机）", "Remember what I use (this computer only)"))
            .subtitle(l("用于排序，不会上传，也不参与同步", "Used for ordering. Never uploaded or synced."))
            .active(app.settings.record_usage())
            .build();
        let s = app.settings.clone();
        rec.connect_active_notify(move |r| s.set_bool("recordUsage", r.is_active()));
        g.add(&rec);
        let clear_row = adw::ActionRow::new();
        let clear = gtk::Button::with_label(&l("清除使用记录", "Clear Usage History"));
        clear.set_valign(gtk::Align::Center);
        let e = app.engine.clone();
        clear.connect_clicked(move |b| {
            e.clear_usage();
            b.set_label(&l("已清除", "Cleared"));
            b.set_sensitive(false);
        });
        clear_row.add_suffix(&clear);
        g.add(&clear_row);
        page.add(&g);
        page
    }

    // MARK: 关于

    fn about_page(self: &Rc<Self>) -> adw::PreferencesPage {
        let page = Self::page();
        let g = adw::PreferencesGroup::new();
        let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
        b.set_halign(gtk::Align::Center);
        b.set_margin_top(18);
        let icon = gtk::Image::from_icon_name(crate::APP_ID);
        icon.set_pixel_size(72);
        b.append(&icon);
        let name = gtk::Label::new(Some("Nekro Qantrip"));
        name.add_css_class("display-title");
        name.add_css_class("title-1");
        b.append(&name);
        let slogan = gtk::Label::new(Some(&l("按 Q 施放一个戏法", "Press Q to cast a spell")));
        slogan.add_css_class("dim-label");
        b.append(&slogan);
        let (v, core) = (crate::VERSION, qantrip_core::core_version());
        let ver = gtk::Label::new(Some(&t!("版本 {v} · core {core} · Linux 预览版", "Version {v} · core {core} · Linux preview")));
        ver.add_css_class("mono");
        ver.add_css_class("caption");
        ver.add_css_class("dim-label");
        b.append(&ver);
        let made = gtk::Label::new(None);
        made.set_markup(&t!("由 <a href=\"https://nekro.ai\">NekroAI</a> 开发", "Made by <a href=\"https://nekro.ai\">NekroAI</a>"));
        b.append(&made);
        let links = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        links.set_halign(gtk::Align::Center);
        links.append(&gtk::LinkButton::with_label("https://qantrip.nekro.ai", &l("官网", "Website")));
        links.append(&gtk::LinkButton::with_label("https://github.com/NekroAI/qantrip", "GitHub"));
        b.append(&links);
        self.update_box.set_halign(gtk::Align::Center);
        self.update_box.set_margin_top(6);
        b.append(&self.update_box);
        let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
        sep.set_margin_top(8);
        b.append(&sep);
        let lic = gtk::Label::new(Some(&l(
            "以 GPL-3.0-or-later 许可开源\n内置字体 Monaspace Neon、Fraunces 以 SIL OFL 1.1 许可分发",
            "Open source under GPL-3.0-or-later\nBundled fonts Monaspace Neon and Fraunces are distributed under SIL OFL 1.1",
        )));
        lic.set_justify(gtk::Justification::Center);
        lic.add_css_class("caption");
        lic.add_css_class("dim-label");
        b.append(&lic);
        let sess = gtk::Label::new(Some(&self.app.session.label()));
        sess.add_css_class("caption");
        sess.add_css_class("dim-label");
        b.append(&sess);
        g.add(&b);
        page.add(&g);
        page
    }

    fn fill_update(self: &Rc<Self>) {
        let b = &self.update_box;
        while let Some(c) = b.first_child() {
            b.remove(&c);
        }
        let u = self.app.updates.clone();
        if let Some(r) = u.available.borrow().clone() {
            let v = r.version.clone();
            let t = gtk::Label::new(Some(&t!("有新版本 {v}", "Version {v} is available")));
            t.add_css_class("heading");
            b.append(&t);
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_halign(gtk::Align::Center);
            let dl = gtk::Button::with_label(&l("下载", "Download"));
            dl.add_css_class("suggested-action");
            dl.set_tooltip_text(Some(&update::asset_name(&r.version)));
            let uu = u.clone();
            dl.connect_clicked(move |_| uu.open_download());
            row.append(&dl);
            if u.skipped().as_deref() != Some(r.version.as_str()) {
                let skip = gtk::Button::with_label(&l("跳过此版本", "Skip This Version"));
                let uu = u.clone();
                let ver = r.version.clone();
                skip.connect_clicked(move |_| uu.skip(&ver));
                row.append(&skip);
            } else {
                let s = gtk::Label::new(Some(&l("已跳过", "Skipped")));
                s.add_css_class("dim-label");
                row.append(&s);
            }
            b.append(&row);
        } else {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_halign(gtk::Align::Center);
            let check = gtk::Button::with_label(&l("检查更新", "Check for Updates"));
            check.set_sensitive(u.state.get() != update::State::Checking);
            let uu = u.clone();
            check.connect_clicked(move |_| uu.check(true));
            row.append(&check);
            match u.state.get() {
                update::State::Checking => row.append(&gtk::Spinner::builder().spinning(true).build()),
                update::State::UpToDate => {
                    let s = gtk::Label::new(Some(&l("已是最新版本", "You're up to date")));
                    s.add_css_class("dim-label");
                    row.append(&s);
                }
                update::State::Failed => {
                    let s = gtk::Label::new(Some(&l("检查失败", "Couldn't check")));
                    s.add_css_class("error");
                    row.append(&s);
                    row.append(&gtk::LinkButton::with_label("https://qantrip.nekro.ai/download", &l("前往下载页", "Open download page")));
                }
                update::State::Idle => {}
            }
            b.append(&row);
        }
        let auto = gtk::CheckButton::with_label(&l("自动检查更新", "Check for updates automatically"));
        auto.set_active(u.auto_check());
        auto.set_halign(gtk::Align::Center);
        let uu = u.clone();
        auto.connect_toggled(move |c| uu.set_auto_check(c.is_active()));
        b.append(&auto);
    }
}

fn status_icon(ok: bool) -> gtk::Image {
    let i = gtk::Image::from_icon_name(if ok { "emblem-ok-symbolic" } else { "dialog-warning-symbolic" });
    i.add_css_class(if ok { "success" } else { "warning" });
    i
}

/// 打开系统的键盘快捷键设置
pub fn open_system_shortcuts(desktop: Desktop) {
    let cmd: &[&str] = match desktop {
        Desktop::Gnome => &["gnome-control-center", "keyboard"],
        Desktop::Kde => &["systemsettings", "kcm_keys"],
        _ => &["xdg-open", "settings://"],
    };
    let _ = crate::util::launcher().spawn(&cmd.iter().map(std::ffi::OsStr::new).collect::<Vec<_>>());
}
