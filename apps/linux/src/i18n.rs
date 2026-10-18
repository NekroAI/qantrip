//! 界面语言（docs/decisions.md D29）。界面文字按当前语言选择中文或英文。

use std::sync::atomic::{AtomicU8, Ordering};

use qantrip_core::Locale;

static UI: AtomicU8 = AtomicU8::new(0);

pub fn set_ui_locale(l: Locale) {
    UI.store(if l == Locale::En { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn ui_locale() -> Locale {
    if UI.load(Ordering::Relaxed) == 1 { Locale::En } else { Locale::Zh }
}

pub fn is_zh() -> bool {
    ui_locale() == Locale::Zh
}

/// 按当前界面语言选择：`l("中文", "English")`
pub fn l(zh: &str, en: &str) -> String {
    if is_zh() { zh.to_string() } else { en.to_string() }
}

/// 带内联参数的版本：`t!("已设置为 {x}", "Set to {x}")`
#[macro_export]
macro_rules! t {
    ($zh:literal, $en:literal) => {
        if $crate::i18n::is_zh() { format!($zh) } else { format!($en) }
    };
}

/// 系统语言（LANGUAGE / LC_ALL / LC_MESSAGES / LANG 中第一个设置的值）。
pub fn system_locale() -> Locale {
    for var in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            let v = v.trim();
            if v.is_empty() || v == "C" || v == "POSIX" {
                continue;
            }
            return Locale::from_code(v.split(':').next().unwrap_or(v));
        }
    }
    Locale::En
}
