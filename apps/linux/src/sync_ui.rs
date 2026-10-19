//! 设置 › 同步（docs/sync-design.md）。按状态分为：未登录 → 超出设备上限 / 创建 / 加入 / 恢复 → 已加入
//! （状态、用量、加入请求、设备、轮换与重置）。与 macOS 的 SyncSettingsView.swift 对应。
//! 页面只在显示内容真正变化时重建，避免轮询时打断正在输入的校验码。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;
use qantrip_core::sync::{AccountDevice, DeviceInfo, JoinRequestStatus, JoinStage, SyncUsage};

use crate::app::App;
use crate::i18n::{is_zh, l};
use crate::secrets::Backend;
use crate::t;
use crate::util;

pub struct SyncPage {
    app: Rc<App>,
    parent: gtk::Window,
    page: adw::PreferencesPage,
    groups: RefCell<Vec<adw::PreferencesGroup>>,
    fingerprint: RefCell<String>,
    visible: Cell<bool>,
    poll: RefCell<Option<glib::SourceId>>,
    /// 恢复短语模式（未加入时）
    recover_mode: Cell<bool>,
    /// 输入中的恢复短语
    phrase_input: RefCell<String>,
    phrase_dialog_open: Cell<bool>,
}

/// 大号等宽显示的校验码
fn big_code(text: &str) -> gtk::Label {
    let code = gtk::Label::new(Some(text));
    code.add_css_class("big-code");
    code.set_selectable(true);
    code.set_margin_top(8);
    code.set_margin_bottom(4);
    code.set_xalign(0.0);
    code
}

fn relative(ts: i64) -> String {
    let d = (util::now_secs() - ts).max(0);
    let (n, zh, en) = if d < 60 {
        return l("刚刚", "just now");
    } else if d < 3600 {
        (d / 60, "分钟前", "min ago")
    } else if d < 86400 {
        (d / 3600, "小时前", "h ago")
    } else {
        (d / 86400, "天前", "d ago")
    };
    if is_zh() { format!("{n} {zh}") } else { format!("{n} {en}") }
}

fn clock(ts: i64) -> String {
    glib::DateTime::from_unix_local(ts).ok().and_then(|d| d.format("%H:%M").ok()).map(|s| s.to_string()).unwrap_or_default()
}

fn tag(text: &str, class: &str) -> gtk::Label {
    let t = gtk::Label::new(Some(text));
    t.add_css_class("tag");
    if !class.is_empty() {
        t.add_css_class(class);
    }
    t.set_valign(gtk::Align::Center);
    t
}

impl SyncPage {
    pub fn new(app: Rc<App>, parent: gtk::Window) -> Rc<SyncPage> {
        let p = Rc::new(SyncPage {
            app: app.clone(),
            parent,
            page: adw::PreferencesPage::new(),
            groups: RefCell::new(Vec::new()),
            fingerprint: RefCell::new(String::new()),
            visible: Cell::new(false),
            poll: RefCell::new(None),
            recover_mode: Cell::new(false),
            phrase_input: RefCell::new(String::new()),
            phrase_dialog_open: Cell::new(false),
        });
        let weak = Rc::downgrade(&p);
        app.sync.connect_changed(move || {
            if let Some(p) = weak.upgrade() {
                p.rebuild(false);
            }
        });
        p.rebuild(true);
        p
    }

    pub fn widget(&self) -> &adw::PreferencesPage {
        &self.page
    }

    /// 页面可见时：启动同步服务、刷新，并每 6 秒查询一次加入请求
    pub fn set_visible(self: &Rc<Self>, v: bool) {
        if self.visible.get() == v {
            return;
        }
        self.visible.set(v);
        self.app.sync.settings_visible.set(v);
        if v {
            self.app.sync.start();
            self.app.sync.refresh();
            let weak = Rc::downgrade(self);
            let id = glib::timeout_add_seconds_local(6, move || match weak.upgrade() {
                Some(p) => {
                    p.app.sync.poll_join_requests();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
            *self.poll.borrow_mut() = Some(id);
        } else if let Some(id) = self.poll.borrow_mut().take() {
            id.remove();
        }
    }

    fn fingerprint(&self) -> String {
        let s = &self.app.sync;
        let st = s.status.borrow().clone();
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}",
            st.as_ref().map(|x| (
                x.signed_in,
                &x.account,
                x.has_vault,
                x.enrolled,
                x.last_sync.map(|t| t / 60),
                x.writes_blocked,
                x.alerts.len(),
                x.usage,
                x.next_push_at,
                x.device_limit,
                &x.device_id
            )),
            s.remote_vault.borrow(),
            s.join.borrow().as_ref().map(|j| (j.stage, j.sas.clone())),
            s.join_requests.borrow().iter().map(|r| (r.join_id.clone(), r.status, r.choices.clone())).collect::<Vec<_>>(),
            s.approved_joins.borrow(),
            s.working.borrow(),
            s.error.borrow(),
            s.devices.borrow().iter().map(|d| d.device_id.clone()).collect::<Vec<_>>(),
            s.account_devices
                .borrow()
                .as_ref()
                .map(|a| (a.max_devices, a.devices.iter().map(|d| (d.grant_id.clone(), d.last_seen_at / 60, d.limited, d.in_vault)).collect::<Vec<_>>())),
            s.last_report.borrow().as_ref().map(|r| r.replaced),
            self.recover_mode.get(),
            self.app.settings.sync_server(),
            crate::i18n::is_zh(),
        )
    }

    fn rebuild(self: &Rc<Self>, force: bool) {
        // 恢复短语只显示一次
        if self.app.sync.phrase.borrow().is_some() && !self.phrase_dialog_open.get() {
            self.show_phrase_dialog();
        }
        let fp = self.fingerprint();
        if !force && *self.fingerprint.borrow() == fp {
            return;
        }
        *self.fingerprint.borrow_mut() = fp;
        for g in self.groups.borrow_mut().drain(..) {
            self.page.remove(&g);
        }
        let s = self.app.sync.clone();
        if let Some(w) = s.working.borrow().clone() {
            let g = self.group(None, None);
            let row = adw::ActionRow::builder().title(&w).build();
            row.add_prefix(&gtk::Spinner::builder().spinning(true).build());
            if !s.signed_in() {
                let c = gtk::Button::with_label(&l("取消", "Cancel"));
                c.set_valign(gtk::Align::Center);
                let ss = s.clone();
                c.connect_clicked(move |_| ss.cancel_sign_in());
                row.add_suffix(&c);
            }
            g.add(&row);
        }
        if let Some(e) = s.error.borrow().clone() {
            let g = self.group(None, None);
            let row = adw::ActionRow::builder().title(glib::markup_escape_text(&e).as_str()).build();
            row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
            row.add_css_class("error");
            g.add(&row);
        }
        let st = s.status.borrow().clone();
        match &st {
            None => self.signed_out(),
            Some(x) if !x.signed_in => self.signed_out(),
            Some(x) if x.device_limit.is_some() => self.device_limit(x.device_limit.unwrap_or(0)),
            Some(x) if !x.has_vault => self.no_vault(),
            _ => self.enrolled(),
        }
        self.advanced();
    }

    fn group(&self, title: Option<String>, desc: Option<String>) -> adw::PreferencesGroup {
        let g = adw::PreferencesGroup::new();
        if let Some(t) = title {
            g.set_title(&t);
        }
        if let Some(d) = desc {
            g.set_description(Some(&d));
        }
        self.page.add(&g);
        self.groups.borrow_mut().push(g.clone());
        g
    }

    fn busy(&self) -> bool {
        self.app.sync.working.borrow().is_some()
    }

    fn signed_out(self: &Rc<Self>) {
        let g = self.group(
            Some(l("在多台设备之间同步你的模板", "Sync your templates across your devices")),
            Some(l(
                "端到端加密：密钥只在你的设备上，服务器只保存无法解密的密文。",
                "End-to-end encrypted: keys stay on your devices, and the server only stores ciphertext it can't decrypt.",
            )),
        );
        let row = adw::ActionRow::new();
        let b = gtk::Button::with_label(&l("使用 GitHub 登录", "Sign in with GitHub"));
        b.add_css_class("suggested-action");
        b.set_valign(gtk::Align::Center);
        b.set_sensitive(!self.busy());
        let s = self.app.sync.clone();
        b.connect_clicked(move |_| s.sign_in());
        row.add_suffix(&b);
        g.add(&row);
        self.storage_note(&g);
    }

    /// 密钥保存位置说明（Secret Service 不可用时回退到文件）
    fn storage_note(&self, g: &adw::PreferencesGroup) {
        let text = match self.app.sync.backend.get() {
            Some(Backend::File) => l(
                "系统没有可用的密钥环（Secret Service），同步密钥保存在仅本用户可读的文件中（权限 0600）。以同一用户身份运行的其他程序可以读取它。",
                "No system keyring (Secret Service) is available, so sync keys are stored in a file only your user can read (mode 0600). Other programs running as your user can read it.",
            ),
            _ => l(
                "同步密钥保存在系统密钥环中（Secret Service，如 GNOME 密钥环、KWallet）。",
                "Sync keys are stored in the system keyring (Secret Service, e.g. GNOME Keyring or KWallet).",
            ),
        };
        let lbl = gtk::Label::new(Some(&text));
        lbl.set_wrap(true);
        lbl.set_xalign(0.0);
        lbl.add_css_class("caption");
        lbl.add_css_class("dim-label");
        lbl.set_margin_top(8);
        g.add(&lbl);
    }

    fn account_group(self: &Rc<Self>) -> adw::PreferencesGroup {
        let s = self.app.sync.clone();
        let g = self.group(None, None);
        let account = s.status.borrow().as_ref().and_then(|x| x.account.clone()).unwrap_or_default();
        let row = adw::ActionRow::builder().title(l("账号", "Account")).subtitle(&account).build();
        let out = gtk::Button::with_label(&l("退出登录", "Sign out"));
        out.set_valign(gtk::Align::Center);
        let ss = s.clone();
        out.connect_clicked(move |_| ss.sign_out());
        row.add_suffix(&out);
        g.add(&row);
        g
    }

    fn no_vault(self: &Rc<Self>) {
        let s = self.app.sync.clone();
        self.account_group();
        if let Some(join) = s.join.borrow().clone() {
            let g = self.group(None, None);
            match join.stage {
                JoinStage::WaitingForApprover => {
                    let r = adw::ActionRow::builder()
                        .title(l("等待已同步的设备响应…", "Waiting for a device that already syncs…"))
                        .subtitle(l("在那台设备上打开 Qantrip 设置 › 同步。", "Open Qantrip Settings › Sync on that device."))
                        .build();
                    r.add_prefix(&gtk::Spinner::builder().spinning(true).build());
                    g.add(&r);
                }
                JoinStage::CompareCode => {
                    g.set_title(&l("核对校验码", "Check the code"));
                    g.set_description(Some(&l(
                        "在另一台设备上，从三个码中点选这个码。两台设备上的码一致时，点「一致」。",
                        "On your other device, pick this code from the three shown. If both devices show the same code, click Match.",
                    )));
                    g.add(&big_code(join.sas.as_deref().unwrap_or_default()));
                    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                    let ok = gtk::Button::with_label(&l("一致", "Match"));
                    ok.add_css_class("suggested-action");
                    ok.set_sensitive(!self.busy());
                    let ss = s.clone();
                    ok.connect_clicked(move |_| ss.confirm_join());
                    let no = gtk::Button::with_label(&l("不一致", "Doesn't match"));
                    let ss = s.clone();
                    no.connect_clicked(move |_| ss.cancel_join());
                    bx.append(&ok);
                    bx.append(&no);
                    g.add(&bx);
                    return;
                }
                JoinStage::WaitingForOtherDevice => {
                    g.add(&big_code(join.sas.as_deref().unwrap_or_default()));
                    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                    bx.append(&gtk::Spinner::builder().spinning(true).build());
                    bx.append(&gtk::Label::new(Some(&l("等待另一台设备上点选这个码…", "Waiting for you to pick this code on the other device…"))));
                    g.add(&bx);
                }
                JoinStage::Completed => {
                    g.add(&adw::ActionRow::builder().title(l("已加入", "Joined")).build());
                }
                JoinStage::Expired => {
                    g.add(&adw::ActionRow::builder().title(l("请求已过期或已取消", "The request expired or was cancelled")).build());
                }
            }
            let c = gtk::Button::with_label(&if join.stage == JoinStage::Expired { l("关闭", "Close") } else { l("取消", "Cancel") });
            c.set_halign(gtk::Align::Start);
            c.set_margin_top(8);
            let ss = s.clone();
            c.connect_clicked(move |_| ss.cancel_join());
            g.add(&c);
            return;
        }
        if self.recover_mode.get() {
            let g = self.group(Some(l("输入创建保险库时保存的 24 个词", "Enter the 24 words you saved when creating the vault")), None);
            let tv =
                gtk::TextView::builder().monospace(true).wrap_mode(gtk::WrapMode::Word).top_margin(8).bottom_margin(8).left_margin(8).right_margin(8).build();
            tv.buffer().set_text(&self.phrase_input.borrow());
            tv.set_size_request(-1, 90);
            let frame = gtk::Frame::new(None);
            frame.set_child(Some(&tv));
            g.add(&frame);
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_margin_top(8);
            let back = gtk::Button::with_label(&l("返回", "Back"));
            let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            spacer.set_hexpand(true);
            let rec = gtk::Button::with_label(&l("恢复", "Recover"));
            rec.add_css_class("suggested-action");
            let words = self.phrase_input.borrow().split_whitespace().count();
            rec.set_sensitive(words >= 12 && !self.busy());
            row.append(&back);
            row.append(&spacer);
            row.append(&rec);
            g.add(&row);
            let weak = Rc::downgrade(self);
            let rec2 = rec.clone();
            tv.buffer().connect_changed(move |b| {
                if let Some(p) = weak.upgrade() {
                    let t = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
                    rec2.set_sensitive(t.split_whitespace().count() >= 12 && !p.busy());
                    *p.phrase_input.borrow_mut() = t;
                }
            });
            let weak = Rc::downgrade(self);
            back.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.recover_mode.set(false);
                    p.rebuild(true);
                }
            });
            let weak = Rc::downgrade(self);
            rec.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    let phrase = p.phrase_input.borrow().clone();
                    p.app.sync.recover(phrase);
                }
            });
            return;
        }
        match *s.remote_vault.borrow() {
            Some(false) => {
                let g = self.group(
                    Some(l("这是你的第一台设备", "This is your first device")),
                    Some(l(
                        "创建保险库后会显示一份恢复短语，请抄写保存。它是所有设备都丢失时找回数据的唯一方式。",
                        "Creating a vault shows a recovery phrase. Write it down: it's the only way back in if you lose every device.",
                    )),
                );
                let r = adw::ActionRow::new();
                let b = gtk::Button::with_label(&l("创建保险库", "Create vault"));
                b.add_css_class("suggested-action");
                b.set_valign(gtk::Align::Center);
                b.set_sensitive(!self.busy());
                let ss = s.clone();
                b.connect_clicked(move |_| ss.create_vault());
                r.add_suffix(&b);
                g.add(&r);
            }
            Some(true) => {
                let g = self.group(
                    Some(l("把这台设备加入保险库", "Add this device to your vault")),
                    Some(l(
                        "开始后，在一台已经同步的设备上打开 Qantrip 设置 › 同步，从三个码中点选这里显示的那个。",
                        "Then, on a device that already syncs, open Qantrip Settings › Sync and pick the code shown here.",
                    )),
                );
                let r = adw::ActionRow::new();
                let start = gtk::Button::with_label(&l("开始加入", "Start"));
                start.add_css_class("suggested-action");
                start.set_valign(gtk::Align::Center);
                start.set_sensitive(!self.busy());
                let ss = s.clone();
                start.connect_clicked(move |_| ss.start_join());
                let rec = gtk::Button::with_label(&l("改用恢复短语", "Use recovery phrase"));
                rec.set_valign(gtk::Align::Center);
                let weak = Rc::downgrade(self);
                rec.connect_clicked(move |_| {
                    if let Some(p) = weak.upgrade() {
                        p.recover_mode.set(true);
                        p.rebuild(true);
                    }
                });
                r.add_suffix(&rec);
                r.add_suffix(&start);
                g.add(&r);
            }
            None => {
                let g = self.group(None, None);
                g.add(&gtk::Spinner::builder().spinning(true).halign(gtk::Align::Start).build());
            }
        }
    }

    fn enrolled(self: &Rc<Self>) {
        let s = self.app.sync.clone();
        let st = s.status.borrow().clone().unwrap();
        if st.writes_blocked {
            let g = self.group(None, None);
            let r = adw::ActionRow::builder()
                .title(l("检测到异常，已暂停上传。本地模板不受影响。", "Something looked wrong, so uploads are paused. Your local templates are fine."))
                .build();
            r.add_prefix(&gtk::Image::from_icon_name("security-low-symbolic"));
            r.add_css_class("error");
            let b = gtk::Button::with_label(&l("重新验证", "Check again"));
            b.set_valign(gtk::Align::Center);
            b.set_sensitive(!self.busy());
            let ss = s.clone();
            b.connect_clicked(move |_| ss.reverify());
            r.add_suffix(&b);
            g.add(&r);
        }

        let g = self.group(None, None);
        let account = adw::ActionRow::builder().title(l("账号", "Account")).subtitle(st.account.clone().unwrap_or_default()).build();
        g.add(&account);
        let last = adw::ActionRow::builder()
            .title(l("上次同步", "Last synced"))
            .subtitle(st.last_sync.map(relative).unwrap_or_else(|| l("尚未同步", "Not yet")))
            .build();
        g.add(&last);
        if let Some(u) = st.usage {
            g.add(&usage_row(u));
        }
        if let Some(at) = st.next_push_at {
            let tm = clock(at);
            let r = adw::ActionRow::builder()
                .subtitle(t!(
                    "本地修改将在 {tm} 自动上传；点「立即同步」可马上上传。",
                    "Local changes upload automatically at {tm}. Click Sync Now to upload right away."
                ))
                .build();
            g.add(&r);
        }
        if let Some(n) = s.last_report.borrow().as_ref().map(|r| r.replaced).filter(|n| *n > 0) {
            let r = adw::ActionRow::builder()
                .subtitle(t!(
                    "有 {n} 个模板在两台设备上都改过，已保留最后修改的版本，另一版存入历史版本。",
                    "{n} template(s) were changed on two devices. The latest edit was kept; the other version is in History."
                ))
                .build();
            g.add(&r);
        }
        let actions = adw::ActionRow::new();
        let sync = gtk::Button::with_label(&l("立即同步", "Sync now"));
        sync.add_css_class("suggested-action");
        sync.set_sensitive(!self.busy());
        let ss = s.clone();
        sync.connect_clicked(move |_| ss.sync_now());
        let out = gtk::Button::with_label(&l("退出登录", "Sign out"));
        let ss = s.clone();
        out.connect_clicked(move |_| ss.sign_out());
        let history = gtk::Button::with_label(&l("历史版本", "History"));
        history.set_tooltip_text(Some(&l(
            "同一模板在两台设备上都改过时，保留最后修改的版本；被覆盖的版本保存在这里 30 天。",
            "When a template is changed on two devices, the latest edit wins. The overwritten version is kept here for 30 days.",
        )));
        let ss = s.clone();
        let parent = self.parent.clone();
        history.connect_clicked(move |_| {
            if let Some(dir) = ss.history_dir() {
                gtk::FileLauncher::new(Some(&gio::File::for_path(&dir))).launch(Some(&parent), gio::Cancellable::NONE, |_| {});
            }
        });
        for b in [&out, &history, &sync] {
            b.set_valign(gtk::Align::Center);
            actions.add_suffix(b);
        }
        g.add(&actions);

        if !st.alerts.is_empty() {
            let g = self.group(Some(l("安全提醒", "Security alerts")), None);
            for a in &st.alerts {
                let text = if is_zh() { a.message.clone() } else { a.code.replace('_', " ") };
                g.add(&adw::ActionRow::builder().title(glib::markup_escape_text(&text).as_str()).build());
            }
            let r = adw::ActionRow::new();
            let b = gtk::Button::with_label(&l("知道了", "Dismiss"));
            b.set_valign(gtk::Align::Center);
            let ss = s.clone();
            b.connect_clicked(move |_| ss.dismiss_alerts());
            r.add_suffix(&b);
            g.add(&r);
        }

        // 刚批准的请求：提示到新设备上确认
        for id in s.approved_joins.borrow().clone() {
            let g = self.group(Some(l("已批准", "Approved")), None);
            let bx = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            let note = gtk::Label::new(Some(&l("在新设备上点「一致」即可完成。", "Click Match on the new device to finish.")));
            note.set_hexpand(true);
            note.set_xalign(0.0);
            let b = gtk::Button::with_label(&l("完成", "Done"));
            b.set_valign(gtk::Align::Center);
            let ss = s.clone();
            b.connect_clicked(move |_| ss.dismiss_approved(&id));
            bx.append(&note);
            bx.append(&b);
            g.add(&bx);
        }

        let requests: Vec<_> = s
            .join_requests
            .borrow()
            .iter()
            .filter(|r| r.status != JoinRequestStatus::HandledElsewhere && !s.approved_joins.borrow().contains(&r.join_id))
            .cloned()
            .collect();
        for r in requests {
            let reject = gtk::Button::with_label(&l("拒绝", "Reject"));
            reject.add_css_class("destructive-action");
            reject.set_valign(gtk::Align::Center);
            let ss = s.clone();
            let id = r.join_id.clone();
            reject.connect_clicked(move |_| ss.reject(id.clone()));
            match r.status {
                JoinRequestStatus::WaitingForNewDevice => {
                    let g = self.group(Some(l("新设备请求加入", "A new device wants to join")), None);
                    let row = adw::ActionRow::builder().title(l("等待新设备显示校验码…", "Waiting for the new device to show its code…")).build();
                    row.add_prefix(&gtk::Spinner::builder().spinning(true).build());
                    g.add(&row);
                }
                JoinRequestStatus::Incompatible => {
                    let g = self.group(
                        Some(l("新设备请求加入", "A new device wants to join")),
                        Some(l(
                            "新设备上的 Qantrip 版本与这台不同，无法加入。请把两台设备都更新到最新版本。",
                            "The new device runs a different version of Qantrip and can't join. Update both devices to the latest version.",
                        )),
                    );
                    reject.set_halign(gtk::Align::Start);
                    g.add(&reject);
                }
                _ => {
                    // 只放普通控件：AdwPreferencesGroup 总把列表行排在普通控件前面
                    let g = self.group(
                        Some(l("新设备请求加入", "A new device wants to join")),
                        Some(l("新设备上显示的是哪一个码？如果不是你本人在操作，请拒绝。", "Which code does the new device show? If this isn't you, reject it.")),
                    );
                    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                    for c in &r.choices {
                        let b = gtk::Button::with_label(c);
                        b.add_css_class("mono");
                        b.add_css_class("code-choice");
                        b.set_sensitive(!self.busy());
                        let ss = s.clone();
                        let (id, c) = (r.join_id.clone(), c.clone());
                        b.connect_clicked(move |_| ss.approve(id.clone(), c.clone()));
                        bx.append(&b);
                    }
                    g.add(&bx);
                    reject.set_label(&l("都不是，拒绝", "None of these — reject"));
                    reject.set_halign(gtk::Align::Start);
                    g.add(&reject);
                }
            }
        }

        // 设备
        let account = s.account_devices.borrow().clone();
        let g = self.group(
            Some(l("设备", "Devices")),
            account.as_ref().map(|a| {
                let n = a.max_devices;
                t!(
                    "这个账号最多同时登录 {n} 台设备。要添加设备，在新设备上登录同一个账号并按提示操作。",
                    "This account can be signed in on up to {n} devices. To add one, sign in with the same account on it and follow the steps."
                )
            }),
        );
        match account {
            Some(a) => {
                for d in &a.devices {
                    g.add(&self.account_device_row(d, true));
                }
                // 已退出登录、但仍在保险库中的设备：建议从保险库移除并轮换密钥
                let roster: Vec<DeviceInfo> = s.devices.borrow().clone();
                for d in roster.into_iter().filter(|r| !r.is_self && !a.devices.iter().any(|x| x.device_id == r.device_id)) {
                    let row = adw::ActionRow::builder()
                        .title(l("已退出登录的设备", "Signed-out device"))
                        .subtitle(l("仍在保险库中。建议移除并轮换密钥。", "Still in the vault. Removing it rotates the keys."))
                        .build();
                    row.add_prefix(&gtk::Image::from_icon_name("computer-symbolic"));
                    let b = gtk::Button::with_label(&l("从保险库移除", "Remove from Vault"));
                    b.add_css_class("destructive-action");
                    b.set_valign(gtk::Align::Center);
                    b.set_sensitive(!self.busy());
                    let weak = Rc::downgrade(self);
                    b.connect_clicked(move |_| {
                        if let Some(p) = weak.upgrade() {
                            p.confirm_revoke(d.device_id.clone());
                        }
                    });
                    row.add_suffix(&b);
                    g.add(&row);
                }
            }
            None => g.add(&gtk::Spinner::builder().spinning(true).halign(gtk::Align::Start).build()),
        }

        let g = self.group(
            None,
            Some(l(
                "重置只删除这台设备的同步身份，不影响模板文件，也不影响其他设备。",
                "Resetting only removes this device's sync identity. Your template files and other devices are not affected.",
            )),
        );
        let r = adw::ActionRow::new();
        let rotate = gtk::Button::with_label(&l("轮换密钥", "Rotate keys"));
        rotate.set_sensitive(!self.busy());
        let ss = s.clone();
        rotate.connect_clicked(move |_| ss.rotate_keys());
        let reset = gtk::Button::with_label(&l("重置这台设备的同步", "Reset sync on this device"));
        reset.add_css_class("destructive-action");
        let weak = Rc::downgrade(self);
        reset.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.confirm(
                    l("重置这台设备的同步？", "Reset sync on this device?"),
                    l("之后需要重新加入保险库。", "You'll need to join the vault again afterwards."),
                    l("重置", "Reset"),
                    Box::new(|p| p.app.sync.reset_device()),
                );
            }
        });
        for b in [&rotate, &reset] {
            b.set_valign(gtk::Align::Center);
            r.add_suffix(b);
        }
        g.add(&r);
        self.storage_note(&g);
    }

    fn account_device_row(self: &Rc<Self>, d: &AccountDevice, show_vault_tag: bool) -> adw::ActionRow {
        let name = if d.name.is_empty() { l("未命名设备", "Unnamed device") } else { d.name.clone() };
        let mut parts = Vec::new();
        if !d.platform.is_empty() {
            parts.push(d.platform.clone());
        }
        if !d.app_version.is_empty() {
            parts.push(format!("Qantrip {}", d.app_version));
        }
        parts.push(format!("{}{}", l("上次在线 ", "Last seen "), relative(d.last_seen_at)));
        let row =
            adw::ActionRow::builder().title(glib::markup_escape_text(&name).as_str()).subtitle(glib::markup_escape_text(&parts.join(" · ")).as_str()).build();
        let p = d.platform.to_lowercase();
        row.add_prefix(&gtk::Image::from_icon_name(if p.starts_with("windows") || p.starts_with("linux") {
            "computer-symbolic"
        } else {
            "computer-apple-symbolic"
        }));
        if d.is_self {
            row.add_suffix(&tag(&l("本机", "This device"), ""));
        }
        if d.limited {
            row.add_suffix(&tag(&l("超出上限", "Over limit"), "orange"));
        } else if show_vault_tag && !d.in_vault && self.app.sync.enrolled() {
            // 本机没有保险库状态时无法判断其他设备是否在保险库中
            row.add_suffix(&tag(&l("未加入保险库", "Not in vault"), "dim"));
        }
        let b = gtk::Button::with_label(&if d.is_self { l("退出登录", "Sign Out") } else { l("解除", "Release") });
        b.add_css_class("flat");
        b.add_css_class("error");
        b.set_valign(gtk::Align::Center);
        let weak = Rc::downgrade(self);
        let dev = d.clone();
        b.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                let dev = dev.clone();
                p.confirm(
                    l("解除这台设备？", "Release this device?"),
                    l("它会退出登录，无法再上传或下载。如果它还在保险库中，之后可以在这里把它移除并轮换密钥。", "It will be signed out and can no longer upload or download. If it is still in the vault, you can then remove it here, which rotates the keys."),
                    l("解除", "Release"),
                    Box::new(move |p| p.app.sync.release(dev.clone())),
                );
            }
        });
        row.add_suffix(&b);
        row
    }

    /// 本机超出设备上限：列出账号下的设备，解除一台后本机即可同步
    fn device_limit(self: &Rc<Self>, max: u32) {
        let g = self.group(
            Some(t!("这个账号最多同时在 {max} 台设备上同步", "This account can sync on up to {max} devices at a time")),
            Some(l(
                "解除下面任意一台设备后，这台设备就可以同步。旧设备丢失或无法再使用时，也可以在这里直接解除它。",
                "Release any device below and this one can sync. If an old device is lost or unusable, you can release it right here.",
            )),
        );
        if self.app.sync.account_devices.borrow().is_none() {
            self.app.sync.load_account_devices();
        }
        match self.app.sync.account_devices.borrow().clone() {
            Some(a) => {
                for d in a.devices.iter().filter(|d| !d.is_self) {
                    g.add(&self.account_device_row(d, false));
                }
            }
            None => g.add(&gtk::Spinner::builder().spinning(true).halign(gtk::Align::Start).build()),
        }
        let r = adw::ActionRow::new();
        let b = gtk::Button::with_label(&l("退出登录", "Sign out"));
        b.set_valign(gtk::Align::Center);
        let s = self.app.sync.clone();
        b.connect_clicked(move |_| s.sign_out());
        r.add_suffix(&b);
        g.add(&r);
    }

    fn advanced(self: &Rc<Self>) {
        let g = self.group(
            Some(l("高级", "Advanced")),
            Some(l(
                "可以把服务端部署到自己的 Cloudflare 账号，在这里填写地址。登录后不能修改。",
                "You can deploy the server to your own Cloudflare account and enter its address here. Locked while signed in.",
            )),
        );
        let e = adw::EntryRow::builder().title(l("服务器", "Server")).text(self.app.settings.sync_server()).show_apply_button(true).build();
        e.set_sensitive(!self.app.sync.signed_in());
        let app = self.app.clone();
        e.connect_apply(move |e| {
            let v = e.text().trim().trim_end_matches('/').to_string();
            app.settings.set_string("syncServer", if v.is_empty() { None } else { Some(&v) });
            app.sync.start();
        });
        g.add(&e);
        if let Some(id) = self.app.sync.status.borrow().as_ref().and_then(|s| s.device_id.clone()) {
            let r = adw::ActionRow::builder().title(l("本机标识", "This device's ID")).subtitle(&id).subtitle_selectable(true).build();
            r.add_css_class("property");
            g.add(&r);
        }
    }

    fn confirm(self: &Rc<Self>, title: String, body: String, action: String, f: Box<dyn Fn(&Rc<SyncPage>)>) {
        let d = adw::AlertDialog::new(Some(&title), Some(&body));
        d.add_response("cancel", &l("取消", "Cancel"));
        d.add_response("ok", &action);
        d.set_response_appearance("ok", adw::ResponseAppearance::Destructive);
        let weak = Rc::downgrade(self);
        d.connect_response(None, move |_, r| {
            if r == "ok"
                && let Some(p) = weak.upgrade()
            {
                f(&p);
            }
        });
        d.present(Some(&self.parent));
    }

    fn confirm_revoke(self: &Rc<Self>, device_id: String) {
        let id = device_id.clone();
        self.confirm(
            l("移除这台设备？", "Remove this device?"),
            l("被移除的设备将无法读取之后的任何同步内容。", "The removed device won't be able to read anything synced afterwards."),
            l("移除并轮换密钥", "Remove and rotate keys"),
            Box::new(move |p| {
                let weak = Rc::downgrade(p);
                let id2 = id.clone();
                p.app.sync.revoke(id.clone(), None, move || {
                    if let Some(p) = weak.upgrade() {
                        p.ask_phrase_for_revoke(id2);
                    }
                });
            }),
        );
    }

    /// 移除除本机外的最后一台设备时需要恢复短语
    fn ask_phrase_for_revoke(self: &Rc<Self>, device_id: String) {
        let d = adw::AlertDialog::new(
            Some(&l("需要恢复短语", "Recovery phrase needed")),
            Some(&l("这是除本机外的最后一台设备。移除它需要输入恢复短语。", "This is the last other device. Removing it needs your recovery phrase.")),
        );
        let tv = gtk::TextView::builder().monospace(true).wrap_mode(gtk::WrapMode::Word).top_margin(6).bottom_margin(6).left_margin(6).right_margin(6).build();
        tv.set_size_request(380, 90);
        let frame = gtk::Frame::new(None);
        frame.set_child(Some(&tv));
        d.set_extra_child(Some(&frame));
        d.add_response("cancel", &l("取消", "Cancel"));
        d.add_response("remove", &l("移除", "Remove"));
        d.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        let s = self.app.sync.clone();
        d.connect_response(None, move |_, r| {
            if r == "remove" {
                let b = tv.buffer();
                let phrase = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
                s.revoke(device_id.clone(), Some(phrase), || {});
            }
        });
        d.present(Some(&self.parent));
    }

    /// 恢复短语：只显示一次，勾选"我已经保存好了"后才能关闭；可以复制、保存为文件
    fn show_phrase_dialog(self: &Rc<Self>) {
        let Some(phrase) = self.app.sync.take_phrase() else { return };
        self.phrase_dialog_open.set(true);
        let dialog = adw::Dialog::builder().title(l("你的恢复短语", "Your recovery phrase")).content_width(560).can_close(false).build();
        // 恢复短语的标签可以选择文字；焦点放在确认框上，避免第一个词默认被选中
        let b = gtk::Box::new(gtk::Orientation::Vertical, 14);
        b.set_margin_start(22);
        b.set_margin_end(22);
        b.set_margin_top(12);
        b.set_margin_bottom(22);
        let intro = gtk::Label::new(Some(&l(
            "把它保存在安全的地方。所有设备都丢失时，只有它能找回你的同步数据；任何人（包括我们）都无法替你恢复。",
            "Keep it somewhere safe. If you lose every device, it's the only way to get your synced data back. Nobody, including us, can recover it for you.",
        )));
        intro.set_wrap(true);
        intro.set_xalign(0.0);
        intro.add_css_class("dim-label");
        b.append(&intro);
        let grid = gtk::Grid::builder().column_spacing(18).row_spacing(8).column_homogeneous(true).build();
        grid.add_css_class("phrase-grid");
        for (i, w) in phrase.split_whitespace().enumerate() {
            let cell = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let n = gtk::Label::new(Some(&format!("{}", i + 1)));
            n.add_css_class("phrase-index");
            n.set_width_chars(2);
            n.set_xalign(1.0);
            let t = gtk::Label::new(Some(w));
            t.add_css_class("mono");
            t.set_selectable(true);
            cell.append(&n);
            cell.append(&t);
            grid.attach(&cell, (i % 4) as i32, (i / 4) as i32, 1, 1);
        }
        b.append(&grid);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let copy = gtk::Button::with_label(&l("复制", "Copy"));
        let save = gtk::Button::with_label(&l("保存为文件…", "Save to File…"));
        actions.append(&copy);
        actions.append(&save);
        b.append(&actions);
        let p2 = phrase.clone();
        copy.connect_clicked(move |btn| {
            crate::insert::copy_plain(&p2);
            btn.set_label(&l("已复制", "Copied"));
        });
        let parent = self.parent.clone();
        let p3 = phrase.clone();
        save.connect_clicked(move |_| save_phrase(&parent, &p3));
        let note = gtk::Label::new(Some(&l(
            "请把它保存在只有你能访问的地方，例如密码管理器；不要发给任何人。",
            "Keep it somewhere only you can access, such as a password manager. Never share it.",
        )));
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.add_css_class("caption");
        note.add_css_class("dim-label");
        b.append(&note);
        let saved = gtk::CheckButton::with_label(&l("我已经保存好了", "I've saved it"));
        b.append(&saved);
        dialog.set_focus(Some(&saved));
        let done = gtk::Button::with_label(&l("完成", "Done"));
        done.add_css_class("suggested-action");
        done.set_halign(gtk::Align::End);
        done.set_sensitive(false);
        b.append(&done);
        let d2 = dialog.clone();
        saved.connect_toggled(move |c| {
            done.set_sensitive(c.is_active());
            d2.set_can_close(c.is_active());
        });
        let toolbar = adw::ToolbarView::new();
        let hb = adw::HeaderBar::new();
        hb.set_show_end_title_buttons(false);
        toolbar.add_top_bar(&hb);
        toolbar.set_content(Some(&b));
        dialog.set_child(Some(&toolbar));
        let weak = Rc::downgrade(self);
        let d3 = dialog.clone();
        if let Some(done) = b.last_child().and_downcast::<gtk::Button>() {
            done.connect_clicked(move |_| {
                d3.force_close();
            });
        }
        dialog.connect_closed(move |_| {
            if let Some(p) = weak.upgrade() {
                p.phrase_dialog_open.set(false);
                p.app.sync.clear_phrase();
            }
        });
        dialog.present(Some(&self.parent));
    }
}

fn save_phrase(parent: &gtk::Window, phrase: &str) {
    let dialog = gtk::FileDialog::builder().initial_name(l("Qantrip 恢复短语.txt", "Qantrip recovery phrase.txt")).build();
    let words: Vec<String> = phrase.split_whitespace().enumerate().map(|(i, w)| format!("{}. {w}", i + 1)).collect();
    let text = l(
        "Nekro Qantrip 同步恢复短语\n所有设备都丢失时，用它找回同步数据。请妥善保管，不要发给任何人。\n\n",
        "Nekro Qantrip sync recovery phrase\nUse it to recover your synced data if you lose every device. Keep it safe and never share it.\n\n",
    ) + &words.join("\n")
        + "\n";
    dialog.save(Some(parent), gio::Cancellable::NONE, move |r| {
        if let Ok(f) = r
            && let Some(p) = f.path()
        {
            let _ = crate::paths::write_private(&p, text.as_bytes());
        }
    });
}

fn usage_row(u: SyncUsage) -> adw::ActionRow {
    let quota = (u.quota_bytes.max(1)) as f64;
    let ratio = (u.bytes as f64 / quota).min(1.0);
    let row = adw::ActionRow::builder()
        .title(l("云端用量", "Cloud storage"))
        .subtitle(format!("{} / {}", util::format_bytes(u.bytes), util::format_bytes(u.quota_bytes)))
        .build();
    let bar = gtk::LevelBar::builder().min_value(0.0).max_value(1.0).value(ratio).valign(gtk::Align::Center).width_request(160).build();
    row.add_suffix(&bar);
    if ratio >= 1.0 {
        row.set_subtitle(&format!(
            "{} / {} · {}",
            util::format_bytes(u.bytes),
            util::format_bytes(u.quota_bytes),
            l(
                "云端空间已满：新的修改不会上传，本地模板不受影响。可以删除不用的模板腾出空间。",
                "Cloud storage is full: new changes won't upload; local templates are unaffected. Delete unused templates to free space."
            )
        ));
        row.add_css_class("error");
    }
    row
}
