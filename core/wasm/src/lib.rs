//! 官网在线演示：把 Rust 核心的解析、补全与渲染以 JSON 形式暴露给浏览器（docs/decisions.md D27）。
//! 与桌面应用使用同一份语法实现与内置模板，演示结果与应用完全一致。

use std::cell::RefCell;

use qantrip_core::grammar::{self, ParamEdit};
use qantrip_core::library::Library;
use qantrip_core::usage::Usage;
use wasm_bindgen::prelude::*;

thread_local! {
    static LIBRARY: Library = Library::load(None);
    static USAGE: RefCell<Usage> = RefCell::new(Usage::load(None));
}

fn with<T>(f: impl FnOnce(&Library, &Usage) -> T) -> T {
    LIBRARY.with(|lib| USAGE.with(|u| f(lib, &u.borrow())))
}

/// 解析面板输入，返回 `Analysis` 的 JSON。
#[wasm_bindgen]
pub fn analyze(input: &str, selected: Option<String>) -> String {
    with(|lib, usage| serde_json::to_string(&grammar::analyze(lib, usage, input, selected.as_deref())).unwrap_or_default())
}

/// 可插入的最终文本；输入有错误时返回 undefined。
#[wasm_bindgen]
pub fn render(input: &str, selected: Option<String>) -> Option<String> {
    with(|lib, usage| grammar::render_final(lib, usage, input, selected.as_deref()))
}

/// 鼠标操作参数：`edit` 为 ParamEdit 的 JSON（如 `{"kind":"toggle"}`），返回 `EditResult` 的 JSON。
#[wasm_bindgen]
pub fn edit_param(input: &str, selected: Option<String>, name: &str, edit: &str) -> Option<String> {
    let edit: ParamEdit = serde_json::from_str(edit).ok()?;
    with(|lib, usage| grammar::edit_param(lib, usage, input, selected.as_deref(), name, edit))
        .and_then(|r| serde_json::to_string(&r).ok())
}
