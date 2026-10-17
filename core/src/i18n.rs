//! 界面语言（docs/decisions.md D29）。核心中所有面向用户的文字都按 Locale 给出中文或英文。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, uniffi::Enum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    #[default]
    Zh,
    En,
}

impl Locale {
    pub fn from_code(code: &str) -> Locale {
        if code.to_lowercase().starts_with("zh") { Locale::Zh } else { Locale::En }
    }
}

/// 按语言选择格式字符串：`tr!(loc, "中文 {x}", "English {x}")`。两个字面量都可以使用内联参数。
#[macro_export]
macro_rules! tr {
    ($loc:expr, $zh:literal, $en:literal) => {
        match $loc {
            $crate::i18n::Locale::Zh => format!($zh),
            $crate::i18n::Locale::En => format!($en),
        }
    };
}
