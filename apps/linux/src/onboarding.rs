//! 首次启动引导（PRD 11.2）：快捷键确认 → 自动粘贴授权（可跳过）→ 施放一次。与 macOS 的 OnboardingView.swift 对应。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::app::App;
use crate::autopaste::{Method, PortalState};
use crate::hotkey::{Mode, State};
use crate::i18n::l;
use crate::t;

pub struct Onboarding {
    app: Rc<App>,
    window: adw::Window,
    stack: gtk::Stack,
    dots: Vec<gtk::Box>,
    step: Cell<usize>,
    verified: Cell<bool>,
    back: gtk::Button,
    next: gtk::Button,
    skip: gtk::Button,
    hotkey_box: gtk::Box,
    paste_box: gtk::Box,
    success: gtk::Label,
    verify_label: gtk::Label,
    listening: RefCell<bool>,
}

fn title(t: &str, sub: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let h = gtk::Label::new(Some(t));
    h.add_css_class("display-title");
    h.add_css_class("title-1");
    let s = gtk::Label::new(Some(sub));
    s.set_wrap(true);
    s.set_justify(gtk::Justification::Center);
    s.add_css_class("dim-label");
    s.set_max_width_chars(60);
    b.append(&h);
    b.append(&s);
    b
}

impl Onboarding {
    pub fn new(app: Rc<App>) -> Rc<Onboarding> {
        let window = adw::Window::builder().title(l("欢迎使用 Nekro Qantrip", "Welcome to Nekro Qantrip")).default_width(640).default_height(560).build();
        window.set_application(Some(&app.gapp));
        let stack = gtk::Stack::builder().transition_type(gtk::StackTransitionType::SlideLeft).vexpand(true).build();
        let dots_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        dots_box.set_homogeneous(true);
        dots_box.set_margin_start(32);
        dots_box.set_margin_end(32);
        dots_box.set_margin_top(20);
        let dots: Vec<gtk::Box> = (0..3)
            .map(|_| {
                let d = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                d.add_css_class("onboarding-step");
                d.set_size_request(-1, 4);
                dots_box.append(&d);
                d
            })
            .collect();
        let back = gtk::Button::with_label(&l("上一步", "Back"));
        let skip = gtk::Button::with_label(&l("跳过，先用只复制", "Skip and use copy only"));
        let next = gtk::Button::with_label(&l("继续", "Continue"));
        next.add_css_class("suggested-action");
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.set_margin_start(20);
        footer.set_margin_end(20);
        footer.set_margin_bottom(20);
        footer.set_margin_top(10);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        footer.append(&back);
        footer.append(&spacer);
        footer.append(&skip);
        footer.append(&next);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&dots_box);
        root.append(&stack);
        root.append(&footer);
        let tv = adw::ToolbarView::new();
        tv.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
        tv.set_content(Some(&root));
        window.set_content(Some(&tv));

        let o = Rc::new(Onboarding {
            app: app.clone(),
            window,
            stack,
            dots,
            step: Cell::new(0),
            verified: Cell::new(false),
            back,
            next,
            skip,
            hotkey_box: gtk::Box::new(gtk::Orientation::Vertical, 10),
            paste_box: gtk::Box::new(gtk::Orientation::Vertical, 10),
            success: gtk::Label::new(Some(&l("成功了！之后在任何应用里都可以这样使用。", "It worked! You can use it like this in any app."))),
            verify_label: gtk::Label::new(None),
            listening: RefCell::new(false),
        });
        o.build_steps();
        let weak = Rc::downgrade(&o);
        o.back.connect_clicked(move |_| {
            if let Some(o) = weak.upgrade() {
                o.go(o.step.get().saturating_sub(1));
            }
        });
        let weak = Rc::downgrade(&o);
        o.skip.connect_clicked(move |_| {
            if let Some(o) = weak.upgrade() {
                o.go(2);
            }
        });
        let weak = Rc::downgrade(&o);
        o.next.connect_clicked(move |_| {
            if let Some(o) = weak.upgrade() {
                if o.step.get() < 2 {
                    o.go(o.step.get() + 1);
                } else {
                    o.app.settings.set_bool("onboardingDone", true);
                    o.window.close();
                }
            }
        });
        let weak = Rc::downgrade(&o);
        o.window.connect_close_request(move |_| {
            if let Some(o) = weak.upgrade() {
                o.app.onboarding_closed();
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(&o);
        app.connect_changed(move || {
            if let Some(o) = weak.upgrade() {
                o.fill_hotkey();
                o.fill_paste();
            }
        });
        o.go(0);
        o
    }

    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    pub fn window(&self) -> &gtk::Window {
        self.window.upcast_ref()
    }

    pub fn show(&self) {
        self.window.present();
    }

    /// 引导期间按下快捷键：确认可用
    pub fn hotkey_pressed(&self) {
        if !self.verified.replace(true) {
            self.fill_verify();
        }
    }

    fn go(&self, step: usize) {
        self.step.set(step);
        self.stack.set_visible_child_name(&step.to_string());
        for (i, d) in self.dots.iter().enumerate() {
            if i <= step { d.add_css_class("done") } else { d.remove_css_class("done") }
        }
        self.back.set_visible(step > 0);
        let authorized = self.app.autopaste.method() != Method::None;
        self.skip.set_visible(step == 1 && !authorized);
        self.next.set_label(&if step < 2 { l("继续", "Continue") } else { l("开始使用", "Get started") });
    }

    fn page(&self) -> gtk::Box {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 20);
        b.set_valign(gtk::Align::Center);
        b.set_margin_start(40);
        b.set_margin_end(40);
        b
    }

    fn build_steps(self: &Rc<Self>) {
        let hk = self.app.hotkey.display();
        // 1. 快捷键
        let p1 = self.page();
        let wand = gtk::Image::from_icon_name("qantrip-wand-symbolic");
        wand.set_pixel_size(44);
        wand.add_css_class("wand");
        p1.append(&wand);
        p1.append(&title(
            &l("按 Q 施放一个戏法", "Press Q to cast a spell"),
            &l(
                "在任何应用里按下唤起快捷键，输入几个字母，就能把常用 Prompt 插入到当前输入框。",
                "In any app, press the shortcut, type a few letters, and insert a prompt into the current text field.",
            ),
        ));
        p1.append(&self.hotkey_box);
        self.verify_label.set_wrap(true);
        self.verify_label.set_justify(gtk::Justification::Center);
        p1.append(&self.verify_label);
        self.stack.add_named(&p1, Some("0"));

        // 2. 自动粘贴
        let p2 = self.page();
        let icon = gtk::Image::from_icon_name("edit-paste-symbolic");
        icon.set_pixel_size(44);
        icon.add_css_class("wand");
        p2.append(&icon);
        p2.append(&title(
            &l("自动粘贴（可选）", "Auto-paste (optional)"),
            &l("授权后，Qantrip 会替你按下粘贴键，把 Prompt 直接插入当前应用。\n不授权也能用：Prompt 会复制到剪贴板，由你手动粘贴。", "With permission, Qantrip presses the paste key for you and inserts the prompt into the current app.\nWithout it, the prompt is copied to the clipboard for you to paste."),
        ));
        p2.append(&self.paste_box);
        self.stack.add_named(&p2, Some("1"));

        // 3. 练习
        let p3 = self.page();
        p3.append(&title(
            &l("施放一次", "Try it once"),
            &t!("点一下下面的输入框，按 {hk}，输入 zh 后按回车。", "Click the field below, press {hk}, type zh, then press Enter."),
        ));
        let tv = gtk::TextView::builder().wrap_mode(gtk::WrapMode::WordChar).top_margin(8).bottom_margin(8).left_margin(8).right_margin(8).build();
        tv.set_size_request(-1, 120);
        let frame = gtk::Frame::new(None);
        frame.set_child(Some(&tv));
        p3.append(&frame);
        self.success.add_css_class("success");
        self.success.set_visible(false);
        p3.append(&self.success);
        let success = self.success.clone();
        tv.buffer().connect_changed(move |b| success.set_visible(b.char_count() > 0));
        let tip = gtk::Label::new(Some(&l(
            "小提示：输入 r 后按空格再输入 --，可以看到审查模板的全部参数；面板里也可以用鼠标点选。",
            "Tip: type r, a space, then -- to see all parameters of the review template. You can also click items in the panel.",
        )));
        tip.set_wrap(true);
        tip.set_justify(gtk::Justification::Center);
        tip.add_css_class("caption");
        tip.add_css_class("dim-label");
        p3.append(&tip);
        self.stack.add_named(&p3, Some("2"));
        self.fill_hotkey();
        self.fill_paste();
        self.fill_verify();
    }

    fn fill_verify(&self) {
        let hk = self.app.hotkey.display();
        if self.verified.get() {
            self.verify_label.set_label(&l("✓ 快捷键可以正常使用", "✓ The shortcut works"));
            self.verify_label.add_css_class("success");
        } else {
            self.verify_label.set_label(&t!("现在按一次 {hk} 试试", "Press {hk} now to try it"));
            self.verify_label.remove_css_class("success");
        }
    }

    fn fill_hotkey(self: &Rc<Self>) {
        let b = &self.hotkey_box;
        while let Some(c) = b.first_child() {
            b.remove(&c);
        }
        let hk = self.app.hotkey.clone();
        let label = hk.display();
        let line = |text: String, class: &str| {
            let lbl = gtk::Label::new(Some(&text));
            lbl.set_wrap(true);
            lbl.set_justify(gtk::Justification::Center);
            if !class.is_empty() {
                lbl.add_css_class(class);
            }
            b.append(&lbl);
        };
        match (hk.mode(), hk.state()) {
            (_, State::Active { .. }) => {
                let k = gtk::Label::new(Some(&label));
                k.add_css_class("mono");
                k.add_css_class("title-2");
                b.append(&k);
                if hk.mode() == Mode::Portal {
                    line(l("已由系统注册。修改快捷键请在系统设置中进行。", "Registered with the system. Change it in your system settings."), "dim-label");
                } else {
                    line(l("可以在设置 › 通用中更换。", "You can change it in Settings › General."), "dim-label");
                }
            }
            (_, State::Pending) => {
                line(l("正在向系统注册快捷键，请在系统弹出的对话框中确认…", "Registering the shortcut — confirm it in the system dialog…"), "dim-label")
            }
            (_, State::Failed { reason }) => {
                line(t!("快捷键注册失败：{reason}", "Couldn't register the shortcut: {reason}"), "warning");
                let r = gtk::Button::with_label(&l("重试", "Retry"));
                r.set_halign(gtk::Align::Center);
                let h = hk.clone();
                r.connect_clicked(move |_| h.retry());
                b.append(&r);
                self.manual_hint();
            }
            (_, State::Manual) => self.manual_hint(),
        }
    }

    fn manual_hint(self: &Rc<Self>) {
        let cmd = format!("{} toggle", if std::env::var_os("APPIMAGE").is_some() { crate::autostart::exec_path() } else { "qantrip".into() });
        let lbl = gtk::Label::new(Some(&t!(
            "在系统设置 › 键盘 › 自定义快捷键中添加一个快捷键（建议 Ctrl+Q），命令填写：\n{cmd}",
            "Add a custom shortcut (we suggest Ctrl+Q) in system settings › Keyboard that runs:\n{cmd}"
        )));
        lbl.set_wrap(true);
        lbl.set_justify(gtk::Justification::Center);
        lbl.set_selectable(true);
        self.hotkey_box.append(&lbl);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_halign(gtk::Align::Center);
        let copy = gtk::Button::with_label(&l("复制命令", "Copy Command"));
        let c2 = cmd.clone();
        copy.connect_clicked(move |b| {
            crate::insert::copy_plain(&c2);
            b.set_label(&l("已复制", "Copied"));
        });
        let open = gtk::Button::with_label(&l("打开键盘设置…", "Open Keyboard Settings…"));
        let desktop = self.app.session.desktop;
        open.connect_clicked(move |_| crate::settings_ui::open_system_shortcuts(desktop));
        row.append(&copy);
        row.append(&open);
        self.hotkey_box.append(&row);
    }

    fn fill_paste(self: &Rc<Self>) {
        let b = &self.paste_box;
        while let Some(c) = b.first_child() {
            b.remove(&c);
        }
        let ap = self.app.autopaste.clone();
        let status = |ok: bool, text: String| {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_halign(gtk::Align::Center);
            let i = gtk::Image::from_icon_name(if ok { "emblem-ok-symbolic" } else { "dialog-information-symbolic" });
            if ok {
                i.add_css_class("success");
            }
            let lbl = gtk::Label::new(Some(&text));
            lbl.set_wrap(true);
            if ok {
                lbl.add_css_class("success");
            }
            row.append(&i);
            row.append(&lbl);
            b.append(&row);
        };
        if self.app.session.is_x11() {
            status(true, l("X11 下自动粘贴不需要授权", "On X11, auto-paste needs no permission"));
        } else {
            match ap.portal_state() {
                PortalState::Authorized => status(true, l("已授权自动粘贴", "Auto-paste is allowed")),
                PortalState::Requesting => status(
                    false,
                    l(
                        "请在系统对话框中打开“允许远程交互”，勾选“记住”，然后点“共享”。",
                        "In the system dialog, turn on remote interaction, keep “Remember” checked, then click Share.",
                    ),
                ),
                PortalState::Unavailable if ap.method() == Method::Wtype => status(true, l("将使用 wtype 模拟粘贴按键", "wtype will press the paste key")),
                PortalState::Unavailable => status(
                    false,
                    l(
                        "这个桌面不支持应用模拟按键，Prompt 会复制到剪贴板。",
                        "This desktop doesn't let apps simulate keys; prompts are copied to the clipboard.",
                    ),
                ),
                _ => {
                    let btn = gtk::Button::with_label(&l("授权自动粘贴…", "Allow Auto-paste…"));
                    btn.add_css_class("suggested-action");
                    btn.add_css_class("pill");
                    btn.set_halign(gtk::Align::Center);
                    let a = ap.clone();
                    btn.connect_clicked(move |_| a.authorize());
                    b.append(&btn);
                    let note = gtk::Label::new(Some(&l(
                        "Qantrip 通过系统的远程桌面接口按下粘贴键（只使用键盘，不读取屏幕）。系统会询问一次，勾选“记住”后不再询问。",
                        "Qantrip uses the system's remote desktop interface to press the paste key (keyboard only, no screen access). The system asks once; keep “Remember” checked.",
                    )));
                    note.set_wrap(true);
                    note.set_justify(gtk::Justification::Center);
                    note.add_css_class("caption");
                    note.add_css_class("dim-label");
                    b.append(&note);
                }
            }
        }
        self.skip.set_visible(self.step.get() == 1 && ap.method() == Method::None);
        let _ = self.listening.borrow();
    }
}
