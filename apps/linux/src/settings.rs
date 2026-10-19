//! 应用设置：`~/.config/qantrip/settings.json`，键名与 Windows 版一致（camelCase），保留未知键。
//! 变化通过回调通知（如快捷键、语言、排除的应用需要即时生效）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use qantrip_core::Locale;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::i18n;
use crate::paths;

/// Linux 默认唤起快捷键 Ctrl+Shift+Q（docs/decisions.md D42）；保留已保存的 hotKey。
pub const DEFAULT_HOTKEY: &str = "<Control><Shift>q";
pub const DEFAULT_SYNC_SERVER: &str = "https://qantrip.nekro.ai";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LanguageChoice {
    Auto,
    Zh,
    En,
}

impl LanguageChoice {
    pub fn from_code(s: &str) -> LanguageChoice {
        match s {
            "zh" => LanguageChoice::Zh,
            "en" => LanguageChoice::En,
            _ => LanguageChoice::Auto,
        }
    }
}

/// 粘贴按键（docs/decisions.md D37）：自动 = 终端用 Ctrl+Shift+V、其他应用用 Ctrl+V，
/// 识别不出前台应用时（GNOME Wayland）用 Shift+Insert。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PasteKey {
    Auto,
    CtrlV,
    CtrlShiftV,
    ShiftInsert,
}

impl PasteKey {
    pub fn code(self) -> &'static str {
        match self {
            PasteKey::Auto => "auto",
            PasteKey::CtrlV => "ctrl-v",
            PasteKey::CtrlShiftV => "ctrl-shift-v",
            PasteKey::ShiftInsert => "shift-insert",
        }
    }
    pub fn from_code(s: &str) -> PasteKey {
        match s {
            "ctrl-v" => PasteKey::CtrlV,
            "ctrl-shift-v" => PasteKey::CtrlShiftV,
            "shift-insert" => PasteKey::ShiftInsert,
            _ => PasteKey::Auto,
        }
    }
    pub fn all() -> [PasteKey; 4] {
        [PasteKey::Auto, PasteKey::CtrlV, PasteKey::CtrlShiftV, PasteKey::ShiftInsert]
    }
}

/// 排除的应用（D35）：X11 为 WM_CLASS，Wayland 为 app_id（KDE），统一按小写比较。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExcludedApp {
    pub id: String,
    pub name: String,
}

type Listener = Box<dyn Fn(&str)>;

pub struct Settings {
    path: PathBuf,
    map: RefCell<Map<String, Value>>,
    listeners: RefCell<Vec<Rc<Listener>>>,
}

impl Settings {
    pub fn load() -> Rc<Settings> {
        let path = paths::settings_file();
        let map = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default();
        Rc::new(Settings { path, map: RefCell::new(map), listeners: RefCell::new(Vec::new()) })
    }

    pub fn connect_changed(&self, f: impl Fn(&str) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(Box::new(f)));
    }

    fn save(&self) {
        let data = serde_json::to_vec_pretty(&Value::Object(self.map.borrow().clone())).unwrap_or_default();
        if let Err(e) = paths::write_atomic(&self.path, &data) {
            eprintln!("qantrip: 保存设置失败：{e}");
        }
    }

    pub fn set_value(&self, key: &str, value: Value) {
        {
            let mut m = self.map.borrow_mut();
            if m.get(key) == Some(&value) {
                return;
            }
            if value.is_null() {
                m.remove(key);
            } else {
                m.insert(key.to_string(), value);
            }
        }
        self.save();
        let listeners: Vec<_> = self.listeners.borrow().clone();
        for l in listeners {
            l(key);
        }
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.map.borrow().get(key).cloned()
    }

    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }

    pub fn set_bool(&self, key: &str, v: bool) {
        self.set_value(key, Value::Bool(v));
    }

    pub fn string(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| v.as_str().map(str::to_string))
    }

    pub fn set_string(&self, key: &str, v: Option<&str>) {
        self.set_value(key, v.map(|s| Value::String(s.to_string())).unwrap_or(Value::Null));
    }

    pub fn f64(&self, key: &str, default: f64) -> f64 {
        self.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
    }

    pub fn set_f64(&self, key: &str, v: f64) {
        self.set_value(key, serde_json::json!(v));
    }

    // MARK: 具体设置

    pub fn hotkey(&self) -> String {
        self.string("hotKey").filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_HOTKEY.to_string())
    }
    pub fn force_ascii_input(&self) -> bool {
        self.bool("forceASCIIInput", true)
    }
    pub fn expand_paste_enabled(&self) -> bool {
        self.bool("expandPasteEnabled", true)
    }
    pub fn expand_paste_interval_ms(&self) -> f64 {
        self.f64("expandPasteIntervalMs", 250.0).clamp(100.0, 800.0)
    }
    pub fn copy_only(&self) -> bool {
        self.bool("copyOnly", false)
    }
    pub fn record_usage(&self) -> bool {
        self.bool("recordUsage", true)
    }
    pub fn onboarding_done(&self) -> bool {
        self.bool("onboardingDone", false)
    }
    pub fn paste_key(&self) -> PasteKey {
        PasteKey::from_code(&self.string("pasteKey").unwrap_or_default())
    }
    pub fn sync_server(&self) -> String {
        self.string("syncServer").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_SYNC_SERVER.to_string())
    }
    pub fn interface_language(&self) -> LanguageChoice {
        LanguageChoice::from_code(&self.string("interfaceLanguage").unwrap_or_default())
    }
    pub fn template_language(&self) -> LanguageChoice {
        LanguageChoice::from_code(&self.string("templateLanguage").unwrap_or_default())
    }
    pub fn auto_check_updates(&self) -> bool {
        self.bool("autoCheckUpdates", true)
    }

    pub fn excluded_apps(&self) -> Vec<ExcludedApp> {
        self.get("excludedApps").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }
    pub fn set_excluded_apps(&self, apps: &[ExcludedApp]) {
        self.set_value("excludedApps", serde_json::to_value(apps).unwrap_or(Value::Array(vec![])));
    }
    pub fn is_excluded(&self, id: Option<&str>) -> bool {
        let Some(id) = id else { return false };
        let id = id.to_lowercase();
        self.excluded_apps().iter().any(|a| a.id.to_lowercase() == id)
    }

    /// 实际使用的界面语言
    pub fn ui_locale(&self) -> Locale {
        match self.interface_language() {
            LanguageChoice::Zh => Locale::Zh,
            LanguageChoice::En => Locale::En,
            LanguageChoice::Auto => i18n::system_locale(),
        }
    }

    /// 实际使用的内置模板语言
    pub fn template_locale(&self) -> Locale {
        match self.template_language() {
            LanguageChoice::Zh => Locale::Zh,
            LanguageChoice::En => Locale::En,
            LanguageChoice::Auto => self.ui_locale(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_default_preserves_saved_combinations() {
        for (json, expected) in [
            (r#"{}"#, DEFAULT_HOTKEY),
            (r#"{"hotKey":""}"#, DEFAULT_HOTKEY),
            (r#"{"hotKey":"<Control>q"}"#, "<Control>q"),
            (r#"{"hotKey":"<Alt>space"}"#, "<Alt>space"),
        ] {
            let settings = Settings { path: PathBuf::new(), map: RefCell::new(serde_json::from_str(json).unwrap()), listeners: RefCell::new(Vec::new()) };
            assert_eq!(settings.hotkey(), expected);
        }
    }
}
