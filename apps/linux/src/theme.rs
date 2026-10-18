//! 视觉：品牌色、语法高亮配色、内置字体（docs/decisions.md D17）与面板动效（D15）。
//! 与 macOS 的 Theme.swift 使用同一套颜色；系统关闭动画（gtk-enable-animations）时 CSS 动画自动停用。

use gtk::gdk;
use gtk::prelude::*;

pub struct Palette {
    pub accent: &'static str,
    pub option: &'static str,
    pub value: &'static str,
    pub error: &'static str,
    pub warning: &'static str,
}

pub const LIGHT: Palette = Palette { accent: "#7C3AED", option: "#0F766E", value: "#B45309", error: "#DC2626", warning: "#D97706" };
pub const DARK: Palette = Palette { accent: "#A78BFA", option: "#5EEAD4", value: "#FBBF24", error: "#F87171", warning: "#FBBF24" };

pub fn palette() -> &'static Palette {
    if adw::StyleManager::default().is_dark() { &DARK } else { &LIGHT }
}

pub fn rgba(hex: &str) -> gdk::RGBA {
    gdk::RGBA::parse(hex).unwrap_or(gdk::RGBA::BLACK)
}

unsafe extern "C" {
    fn FcConfigAppFontAddDir(config: *mut std::ffi::c_void, dir: *const std::ffi::c_char) -> std::ffi::c_int;
}

#[link(name = "fontconfig")]
unsafe extern "C" {}

/// 注册应用内置字体（仅本进程可见）。需在 GTK 初始化前调用。
pub fn register_fonts() {
    let Some(dir) = crate::paths::resource("fonts") else { return };
    let Ok(c) = std::ffi::CString::new(dir.to_string_lossy().as_bytes()) else { return };
    // SAFETY：传入 NULL 表示当前配置；路径为以 NUL 结尾的字符串
    unsafe {
        FcConfigAppFontAddDir(std::ptr::null_mut(), c.as_ptr());
    }
}

fn colors(p: &Palette) -> String {
    // libadwaita 1.6+（GTK 4.16+）用 CSS 变量定义强调色；旧版本只认 @define-color
    let vars = if gtk::check_version(4, 16, 0).is_none() {
        format!(":root {{ --accent-bg-color: {a}; --accent-color: {a}; }}\n", a = p.accent)
    } else {
        String::new()
    };
    vars + &format!(
        "@define-color qt_accent {a};\n@define-color qt_option {o};\n@define-color qt_value {v};\n@define-color qt_error {e};\n@define-color qt_warning {w};\n\
         @define-color accent_bg_color {a};\n@define-color accent_color {a};\n",
        a = p.accent,
        o = p.option,
        v = p.value,
        e = p.error,
        w = p.warning
    )
}

const CSS: &str = r#"
window.qantrip-panel, window.qantrip-panel.background { background: transparent; box-shadow: none; }
.qantrip-card {
  background-color: alpha(@window_bg_color, 0.97);
  border-radius: 16px;
  border: 1px solid alpha(@borders, 0.8);
  box-shadow: 0 18px 48px alpha(black, 0.32), 0 2px 8px alpha(black, 0.18);
  margin: 24px;
}
.qantrip-card .glow { background-image: linear-gradient(to bottom, alpha(@qt_accent, 0.13), transparent); border-radius: 16px 16px 0 0; transition: background-image 300ms ease-out; }
.qantrip-card.locked .glow { background-image: linear-gradient(to bottom, alpha(@qt_accent, 0.24), transparent); }
.qantrip-card.entering { animation: qt-in 260ms cubic-bezier(0.2, 0.9, 0.3, 1.15); }
.qantrip-card.shake { animation: qt-shake 320ms linear; }
@keyframes qt-in { from { opacity: 0; transform: translateY(-8px) scale(0.965); } to { opacity: 1; transform: none; } }
@keyframes qt-shake { 0% { transform: translateX(0); } 15% { transform: translateX(-9px); } 30% { transform: translateX(8px); } 45% { transform: translateX(-6px); } 60% { transform: translateX(4px); } 75% { transform: translateX(-2px); } 100% { transform: translateX(0); } }

.wand { color: @qt_accent; }
.wand.bounce { animation: qt-bounce 320ms ease-out; }
@keyframes qt-bounce { 0% { transform: scale(1); } 35% { transform: scale(1.22); } 100% { transform: scale(1); } }

.command-field, .command-field text { background: transparent; font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-size: 19px; }
.command-ghost { font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-size: 19px; color: alpha(currentColor, 0.38); }
.target-chip { font-size: 12px; padding: 4px 10px; border-radius: 999px; background-color: alpha(currentColor, 0.07); color: alpha(currentColor, 0.7); }
.header-icon { min-width: 26px; min-height: 26px; padding: 0; border-radius: 7px; color: alpha(currentColor, 0.45); }
.header-icon:hover { color: currentColor; background-color: alpha(currentColor, 0.09); }

.cand-list { padding: 8px; }
.cand-row { border-radius: 8px; padding: 7px 10px; transition: background-color 120ms ease-out; }
.cand-row:hover { background-color: alpha(currentColor, 0.05); }
.cand-row.selected { background-color: alpha(@qt_accent, 0.20); }
.cand-row.enter { animation: qt-row 280ms ease-out backwards; }
.cand-row.d1 { animation-delay: 16ms; } .cand-row.d2 { animation-delay: 32ms; } .cand-row.d3 { animation-delay: 48ms; }
.cand-row.d4 { animation-delay: 64ms; } .cand-row.d5 { animation-delay: 80ms; } .cand-row.d6 { animation-delay: 96ms; }
.cand-row.d7 { animation-delay: 112ms; } .cand-row.d8 { animation-delay: 128ms; } .cand-row.d9 { animation-delay: 144ms; }
.cand-row.d10 { animation-delay: 160ms; } .cand-row.d11 { animation-delay: 176ms; } .cand-row.d12 { animation-delay: 192ms; }
@keyframes qt-row { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: none; } }
.cand-list.fade { animation: qt-fade 140ms ease-out; }
@keyframes qt-fade { from { opacity: 0; transform: translateX(-6px); } to { opacity: 1; transform: none; } }
.cand-title { font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-size: 14px; }
.cand-title.template { font-weight: 600; }
.cand-title.param { color: @qt_option; }
.cand-title.choice { color: @qt_value; }
.cand-sub { font-size: 12px; color: alpha(currentColor, 0.6); }
.cand-detail { font-size: 12px; color: alpha(currentColor, 0.55); }
.cand-check { color: @qt_accent; }
.section-header { font-size: 11px; font-weight: 600; color: alpha(currentColor, 0.45); padding: 10px 10px 2px 10px; }
.section-header.first { padding-top: 2px; }
.empty-hint { font-size: 12px; color: alpha(currentColor, 0.4); }

.spell-name { font-family: Fraunces, serif; font-size: 22px; font-weight: 600; color: @qt_accent; }
.spell-name.slide { animation: qt-slide 200ms ease-out; }
@keyframes qt-slide { from { opacity: 0; transform: translateX(-10px); } to { opacity: 1; transform: none; } }
.spell-desc { font-size: 13px; color: alpha(currentColor, 0.65); }
.spell-hint { font-size: 11px; color: alpha(currentColor, 0.6); }
.commands-help { font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-size: 12px; color: alpha(currentColor, 0.7); }

button.chip, menubutton.chip > button { border-radius: 999px; padding: 2px 9px; min-height: 0; min-width: 0; border: 1px solid alpha(currentColor, 0.22); background: none; box-shadow: none; }
menubutton.chip { padding: 0; border: none; background: none; box-shadow: none; min-height: 0; }
button.chip:hover, menubutton.chip > button:hover { background-color: alpha(currentColor, 0.06); }
button.chip.set, menubutton.chip.set > button { background-color: alpha(@qt_accent, 0.16); border-color: alpha(@qt_accent, 0.42); }
box.chip { border-radius: 999px; padding: 2px 9px; border: 1px solid alpha(currentColor, 0.22); }
box.chip.set { background-color: alpha(@qt_accent, 0.16); border-color: alpha(@qt_accent, 0.42); }
.chip.bounce { animation: qt-bounce 300ms ease-out; }
.chip .chip-name { font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-size: 11px; color: alpha(currentColor, 0.6); }
.chip.set .chip-name { color: @qt_option; }
.chip .chip-value { font-size: 11px; color: alpha(currentColor, 0.45); }
.chip.set .chip-value { font-weight: 600; color: currentColor; }

.preview-title { font-size: 11px; font-weight: 600; color: alpha(currentColor, 0.6); }
.preview-meta { font-size: 11px; color: alpha(currentColor, 0.45); }
.preview-box { background-color: alpha(@view_bg_color, 0.45); border-radius: 10px; }
.preview-box text, .preview-text, .preview-text text { background: transparent; font-size: 13px; }

.footer { padding: 8px 16px; }
.status { font-size: 12px; }
.status.error { color: @qt_error; }
.status.warning { color: @qt_warning; }
.status.dim { color: alpha(currentColor, 0.5); }
.status.canonical { font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-size: 11px; color: alpha(currentColor, 0.45); }
.keycap { font-size: 10px; font-weight: 500; padding: 1px 5px; border-radius: 4px; background-color: alpha(currentColor, 0.1); }
.key-label { font-size: 11px; }
.key-button { padding: 2px 4px; border-radius: 6px; color: alpha(currentColor, 0.65); background: none; box-shadow: none; min-height: 0; }
.key-button:hover { background-color: alpha(currentColor, 0.07); }

.mono { font-family: Monaspace Neon Var, Monaspace Neon, monospace; }
.display-title { font-family: Fraunces, serif; font-weight: 600; }
.code-view, .code-view text { font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-size: 13px; }
.lib-row-name { font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-weight: 600; font-size: 13px; }
.lib-alias { font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-size: 10px; padding: 0 4px; border-radius: 3px; background-color: alpha(currentColor, 0.1); color: alpha(currentColor, 0.65); }
.tag { font-size: 9px; font-weight: 600; padding: 1px 6px; border-radius: 999px; background-color: alpha(@qt_accent, 0.2); }
.tag.orange { background-color: alpha(@qt_warning, 0.22); }
.tag.dim { background-color: alpha(currentColor, 0.12); }
.param-name { font-family: Monaspace Neon Var, Monaspace Neon, monospace; color: @qt_option; }
.big-code { font-family: Monaspace Neon Var, Monaspace Neon, monospace; font-size: 32px; font-weight: 600; letter-spacing: 4px; }
.phrase-grid { padding: 12px; border-radius: 8px; background-color: alpha(currentColor, 0.06); }
.phrase-index { font-size: 11px; color: alpha(currentColor, 0.5); }
.onboarding-step { background-color: alpha(currentColor, 0.15); border-radius: 999px; min-height: 4px; }
.onboarding-step.done { background-color: @qt_accent; }
.success { color: @success_color; }
.try-input { font-family: Monaspace Neon Var, Monaspace Neon, monospace; }
.problem-box { background-color: alpha(@qt_error, 0.08); border-radius: 8px; padding: 10px; }
.banner-bar { padding: 8px 16px; background-color: alpha(@qt_accent, 0.10); }
.banner-bar.deleted { background-color: alpha(@qt_warning, 0.12); }
"#;

/// 加载样式；深浅色切换时更新品牌色
pub fn install() {
    let Some(display) = gdk::Display::default() else { return };
    let base = gtk::CssProvider::new();
    base.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(&display, &base, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    let colors_provider = gtk::CssProvider::new();
    let sm = adw::StyleManager::default();
    colors_provider.load_from_string(&colors(if sm.is_dark() { &DARK } else { &LIGHT }));
    gtk::style_context_add_provider_for_display(&display, &colors_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    sm.connect_dark_notify(move |sm| {
        colors_provider.load_from_string(&colors(if sm.is_dark() { &DARK } else { &LIGHT }));
    });
    // 自带图标（魔杖等）
    if let Some(dir) = crate::paths::resource("icons") {
        gtk::IconTheme::for_display(&display).add_search_path(dir);
    }
}

/// 重新触发一次 CSS 动画（先移除样式类，下一帧再加上）
pub fn retrigger(w: &impl IsA<gtk::Widget>, class: &str) {
    let w = w.as_ref().clone();
    w.remove_css_class(class);
    let class = class.to_string();
    gtk::glib::idle_add_local_once(move || w.add_css_class(&class));
}

/// 系统是否开启了动画
pub fn motion() -> bool {
    gtk::Settings::default().map(|s| s.is_gtk_enable_animations()).unwrap_or(true)
}
