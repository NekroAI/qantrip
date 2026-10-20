//! Qantrip 核心：模板库、输入语法解析与渲染、端到端加密同步。通过 UniFFI 暴露给各平台外壳。

uniffi::setup_scaffolding!();

pub mod command;
pub mod doc;
pub mod grammar;
pub mod i18n;
pub mod library;
pub mod placeholder;
/// 端到端加密同步（wasm 构建不包含）
#[cfg(not(target_arch = "wasm32"))]
pub mod sync;
pub mod template;
pub mod usage;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub use doc::{DocError, ParamDoc, TemplateDoc};
pub use i18n::Locale;
pub use grammar::*;
pub use template::{Choice, ParamKind};

#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 模板源码 → 结构化文档（图形化编辑器）。
#[uniffi::export]
pub fn parse_template_doc(source: String) -> Result<TemplateDoc, DocError> {
    doc::parse_doc(&source)
}

/// 结构化文档 → 模板源码。
#[uniffi::export]
pub fn serialize_template_doc(doc: TemplateDoc) -> String {
    doc::serialize_doc(&doc)
}

/// 模板编辑器的引导：按当前参数生成可插入的写法与试用示例
#[uniffi::export]
pub fn template_guide(doc: TemplateDoc, locale: Locale) -> doc::TemplateGuide {
    doc::guide(&doc, locale)
}

#[uniffi::export]
pub fn new_template_doc(name: String, locale: Locale) -> TemplateDoc {
    doc::new_doc(&name, locale)
}

/// 模板库中的一项（编辑器列表使用）。
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TemplateEntry {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub category: String,
    pub builtin: bool,
    /// 用户模板覆盖了同名内置模板
    pub overrides_builtin: bool,
    pub partial: bool,
    pub path: Option<String>,
    pub source: String,
    /// 已删除的内置模板（模板库中显示为可恢复）
    pub deleted: bool,
}

/// 编辑器的实时校验与试用结果。
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SourceCheck {
    pub name: Option<String>,
    pub problems: Vec<String>,
    pub doc: Option<TemplateDoc>,
    /// 以试用输入解析的结果（预览、参数状态等）
    pub analysis: Option<Analysis>,
}

#[derive(uniffi::Object)]
pub struct Engine {
    user_dir: Option<PathBuf>,
    /// (界面语言, 内置模板语言)
    langs: RwLock<(Locale, Locale)>,
    /// 指令模式 `/new 名称` 不写正文时使用的剪贴板文字
    clipboard: RwLock<Option<String>>,
    lib: RwLock<library::Library>,
    usage: RwLock<usage::Usage>,
}

#[uniffi::export]
impl Engine {
    /// `user_dir`：用户模板目录（`*.md`），不存在时只加载内置模板。
    /// `usage_path`：使用记录文件；为空时不持久化。
    #[uniffi::constructor]
    pub fn new(user_dir: Option<String>, usage_path: Option<String>) -> Arc<Self> {
        let user_dir = user_dir.map(PathBuf::from);
        let lib = library::Library::load(user_dir.as_deref());
        let usage = usage::Usage::load(usage_path.map(PathBuf::from));
        Arc::new(Engine { user_dir, langs: RwLock::new((Locale::Zh, Locale::Zh)), clipboard: RwLock::new(None), lib: RwLock::new(lib), usage: RwLock::new(usage) })
    }

    /// 设置界面语言与内置模板语言，并重新加载模板库。
    pub fn set_locale(&self, ui: Locale, templates: Locale) {
        *self.langs.write().unwrap() = (ui, templates);
        self.reload();
    }

    /// 面板打开时调用：按当前前台应用（bundle id）计算排序快照。
    pub fn begin_session(&self, app: Option<String>) {
        self.usage.write().unwrap().begin_session(app, usage::now());
    }

    /// 插入成功后记录使用（不含附加文本）。
    pub fn record_use(&self, input: String, selected: Option<String>, app: Option<String>) {
        let lib = self.lib.read().unwrap();
        grammar::record_use(&lib, &mut self.usage.write().unwrap(), &input, selected.as_deref(), app.as_deref());
    }

    pub fn clear_usage(&self) {
        self.usage.write().unwrap().clear();
    }

    /// 鼠标操作参数：返回改写后的输入和光标位置。
    pub fn edit_param(&self, input: String, selected: Option<String>, name: String, edit: ParamEdit) -> Option<EditResult> {
        grammar::edit_param(&self.lib.read().unwrap(), &self.usage.read().unwrap(), &input, selected.as_deref(), &name, edit)
    }

    /// 重新加载模板目录。
    pub fn reload(&self) {
        let (ui, tpl) = *self.langs.read().unwrap();
        let lib = library::Library::load_with(self.user_dir.as_deref(), ui, tpl);
        *self.lib.write().unwrap() = lib;
    }

    /// 加载过程中发现的问题（格式错误、名称或别名冲突等）。
    pub fn problems(&self) -> Vec<String> {
        self.lib.read().unwrap().problems.clone()
    }

    pub fn templates(&self) -> Vec<TemplateInfo> {
        let lib = self.lib.read().unwrap();
        lib.visible()
            .map(|t| TemplateInfo {
                name: t.name.clone(),
                aliases: t.aliases.clone(),
                description: t.description.clone(),
                category: t.category.clone(),
                builtin: t.builtin,
            })
            .collect()
    }

    pub fn entries(&self) -> Vec<TemplateEntry> {
        let tpl = self.langs.read().unwrap().1;
        let lib = self.lib.read().unwrap();
        let out: Vec<TemplateEntry> = lib
            .templates
            .iter()
            .map(|t| TemplateEntry {
                name: t.name.clone(),
                aliases: t.aliases.clone(),
                description: t.description.clone(),
                category: t.category.clone(),
                builtin: t.builtin,
                overrides_builtin: !t.builtin && library::builtin_source_in(&t.name, tpl).is_some(),
                partial: t.is_partial(),
                path: t.path.as_ref().map(|p| p.to_string_lossy().to_string()),
                source: t.source.clone(),
                deleted: false,
            })
            .chain(lib.deleted.iter().filter_map(|d| {
                let src = library::builtin_source_in(&d.name, tpl)?;
                let t = template::parse_in(src, true, lib.locale).ok()?;
                Some(TemplateEntry {
                    name: t.name,
                    aliases: t.aliases,
                    description: t.description,
                    category: t.category,
                    builtin: true,
                    overrides_builtin: false,
                    partial: false,
                    path: Some(d.marker.to_string_lossy().to_string()),
                    source: src.to_string(),
                    deleted: true,
                })
            }))
            .collect();
        // 保持模板库顺序（内置模板按工作流分组），同分类相邻
        let rank = |c: &str| out.iter().position(|e| e.category == c).unwrap_or(usize::MAX);
        let ranks: Vec<usize> = out.iter().map(|e| rank(&e.category)).collect();
        let mut idx: Vec<usize> = (0..out.len()).collect();
        idx.sort_by_key(|&i| (out[i].partial, ranks[i], i));
        idx.into_iter().map(|i| out[i].clone()).collect()
    }

    /// 内置模板的原始源码（用于"自定义内置模板"与"恢复默认"）。
    pub fn builtin_source(&self, name: String) -> Option<String> {
        library::builtin_source_in(&name, self.langs.read().unwrap().1).map(str::to_string)
    }

    /// 校验编辑中的源码，并用 `try_input`（为空时用模板名）试用。
    pub fn check_source(&self, source: String, try_input: String) -> SourceCheck {
        let (ui, tpl) = *self.langs.read().unwrap();
        let (lib, name, problems) = library::Library::with_override_in(self.user_dir.as_deref(), &source, ui, tpl);
        let doc = doc::parse_doc(&source).ok();
        let analysis = name.as_ref().filter(|_| problems.is_empty()).map(|n| {
            let input = if try_input.trim().is_empty() { format!("{n} ") } else { try_input.clone() };
            grammar::analyze(&lib, &self.usage.read().unwrap(), &input, Some(n))
        });
        SourceCheck { name, problems, doc, analysis }
    }

    /// 指令模式使用的剪贴板文字（外壳在输入 `/new` 时读取并传入）。
    pub fn set_clipboard(&self, text: Option<String>) {
        *self.clipboard.write().unwrap() = text;
    }

    /// 删除模板（模板库的"删除"，与 `/rm` 相同）：用户模板移到废纸篓，内置模板写入可恢复的删除标记。
    /// 返回值中的 `trash_path` 由外壳移到废纸篓。
    pub fn delete_template(&self, name: String) -> Result<command::CommandOutcome, DocError> {
        self.run_command(format!("/rm {name}"))
    }

    /// 恢复内置模板（撤销删除或修改，与 `/restore` 相同）。
    pub fn restore_template(&self, name: String) -> Result<command::CommandOutcome, DocError> {
        self.run_command(format!("/restore {name}"))
    }

    /// 执行指令（输入以 `/` 开头）。成功后重新加载模板库。
    pub fn run_command(&self, input: String) -> Result<command::CommandOutcome, DocError> {
        let out = {
            let lib = self.lib.read().unwrap();
            let clip = self.clipboard.read().unwrap();
            command::run(&lib, self.user_dir.as_deref(), &input, clip.as_deref())
        };
        let out = out.map_err(|message| DocError::Invalid { message })?;
        self.reload();
        Ok(out)
    }

    /// 解析面板输入。`selected`：界面当前高亮的模板名。
    pub fn analyze(&self, input: String, selected: Option<String>) -> Analysis {
        if command::is_command(&input) {
            let lib = self.lib.read().unwrap();
            return command::analyze(&lib, self.user_dir.as_deref(), &input, self.clipboard.read().unwrap().as_deref());
        }
        grammar::analyze(&self.lib.read().unwrap(), &self.usage.read().unwrap(), &input, selected.as_deref())
    }

    /// 可插入的最终文本；输入有错误时返回 None。
    pub fn render(&self, input: String, selected: Option<String>) -> Option<String> {
        grammar::render_final(&self.lib.read().unwrap(), &self.usage.read().unwrap(), &input, selected.as_deref())
    }
}
