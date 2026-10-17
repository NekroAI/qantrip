//! 官网在线演示：把 Rust 核心的解析、补全与渲染以 JSON 形式暴露给浏览器（docs/decisions.md D27）。
//! 与桌面应用使用同一份语法实现与内置模板，演示结果与应用完全一致。

use std::cell::RefCell;

use qantrip_core::grammar::{self, ParamEdit};
use qantrip_core::i18n::Locale;
use qantrip_core::library::Library;
use qantrip_core::usage::Usage;
use serde::Serialize;
use wasm_bindgen::prelude::*;

thread_local! {
    static LOCALE: RefCell<Locale> = const { RefCell::new(Locale::Zh) };
    static LIBRARY: RefCell<Library> = RefCell::new(Library::load(None));
    static USAGE: RefCell<Usage> = RefCell::new(Usage::load(None));
}

fn with<T>(f: impl FnOnce(&Library, &Usage) -> T) -> T {
    LIBRARY.with(|lib| USAGE.with(|u| f(&lib.borrow(), &u.borrow())))
}

/// 切换界面与内置模板语言（`zh` / `en`）。
#[wasm_bindgen]
pub fn set_locale(code: &str) {
    let loc = Locale::from_code(code);
    LOCALE.with(|l| *l.borrow_mut() = loc);
    LIBRARY.with(|lib| *lib.borrow_mut() = Library::load_with(None, loc, loc));
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

#[derive(Serialize)]
struct Check {
    name: Option<String>,
    problems: Vec<String>,
    analysis: Option<grammar::Analysis>,
}

/// 在线模板编辑器：用 `source` 覆盖或新增一个模板，校验并以 `try_input` 试用。返回 JSON。
#[wasm_bindgen]
pub fn check_source(source: &str, try_input: &str) -> String {
    let loc = LOCALE.with(|l| *l.borrow());
    let (lib, name, problems) = Library::with_override_in(None, source, loc, loc);
    let analysis = name.as_ref().filter(|_| problems.is_empty()).map(|n| {
        let input = if try_input.trim().is_empty() { format!("{n} ") } else { try_input.to_string() };
        USAGE.with(|u| grammar::analyze(&lib, &u.borrow(), &input, Some(n)))
    });
    serde_json::to_string(&Check { name, problems, analysis }).unwrap_or_default()
}

#[derive(Serialize)]
struct Param {
    name: String,
    kind: String,
    short: Option<char>,
    positional: bool,
    description: String,
    choices: Vec<(String, String)>,
}

#[derive(Serialize)]
struct Info {
    name: String,
    aliases: Vec<String>,
    description: String,
    category: String,
    params: Vec<Param>,
    source: String,
}

/// 当前语言下的内置模板（含源码），按模板库顺序。用于生成官网的模板一览。
#[wasm_bindgen]
pub fn templates() -> String {
    with(|lib, _| {
        let list: Vec<Info> = lib
            .visible()
            .map(|t| Info {
                name: t.name.clone(),
                aliases: t.aliases.clone(),
                description: t.description.clone(),
                category: t.category.clone(),
                params: t
                    .params
                    .iter()
                    .map(|p| Param {
                        name: p.name.clone(),
                        kind: format!("{:?}", p.kind).to_lowercase(),
                        short: p.short,
                        positional: p.positional,
                        description: p.description.clone(),
                        choices: p.choices.iter().map(|c| (c.value.clone(), c.label.clone())).collect(),
                    })
                    .collect(),
                source: t.source.clone(),
            })
            .collect();
        serde_json::to_string(&list).unwrap_or_default()
    })
}
