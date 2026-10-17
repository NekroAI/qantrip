//! Qantrip 核心：模板库、输入语法解析与渲染。通过 UniFFI 暴露给各平台外壳。

uniffi::setup_scaffolding!();

pub mod grammar;
pub mod library;
pub mod template;
pub mod usage;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub use grammar::*;
pub use template::{Choice, ParamKind};

#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[derive(uniffi::Object)]
pub struct Engine {
    user_dir: Option<PathBuf>,
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
        Arc::new(Engine { user_dir, lib: RwLock::new(lib), usage: RwLock::new(usage) })
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
        let lib = library::Library::load(self.user_dir.as_deref());
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

    /// 解析面板输入。`selected`：界面当前高亮的模板名。
    pub fn analyze(&self, input: String, selected: Option<String>) -> Analysis {
        grammar::analyze(&self.lib.read().unwrap(), &self.usage.read().unwrap(), &input, selected.as_deref())
    }

    /// 可插入的最终文本；输入有错误时返回 None。
    pub fn render(&self, input: String, selected: Option<String>) -> Option<String> {
        grammar::render_final(&self.lib.read().unwrap(), &self.usage.read().unwrap(), &input, selected.as_deref())
    }
}
