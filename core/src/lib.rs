//! Qantrip 核心（Spike 版）。
//!
//! 阶段 1 只用于验证 Rust ↔ Swift（UniFFI）链路：内置几个硬编码模板、
//! 简单的分层匹配和"命令词 + 附加文本"拆分。正式的语法解析与 MiniJinja
//! 渲染在阶段 2 按 docs/input-grammar.md 实现。

uniffi::setup_scaffolding!();

mod spike;

pub use spike::{MatchTier, TemplateSummary};

#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 按输入的第一个词搜索模板，结果按匹配层级排序。
#[uniffi::export]
pub fn search(input: String) -> Vec<TemplateSummary> {
    spike::search(&input)
}

/// 用模板渲染最终 Prompt；`input` 中命令词之后的内容作为附加文本。
#[uniffi::export]
pub fn render(template_id: String, input: String) -> Option<String> {
    spike::render(&template_id, &input)
}
