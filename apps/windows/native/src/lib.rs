//! Qantrip 核心的 C ABI，供 Windows 外壳通过 P/Invoke 调用（docs/windows-spike.md「绑定」）。
//!
//! uniffi-bindgen-cs 目前只支持到 uniffi 0.31，核心使用 0.32，因此不走 UniFFI，
//! 而是参照 `core/wasm` 的做法：包装 `Engine`，结构化数据一律以 JSON 字符串传递
//! （核心的语法类型已实现 serde 序列化）。
//!
//! 约定：
//! - 所有字符串参数为以 NUL 结尾的 UTF-8，可为空指针（表示 None）。
//! - 返回的字符串由本库分配，调用方必须用 `qt_string_free` 释放；返回空指针表示 None。
//! - 文本位置（token、高亮、编辑）均为 UTF-16 偏移，与 C# `string` 的索引一致。
//! - 任何 panic 都在边界处捕获，返回空指针 / 不做任何事，不会跨越 FFI 展开。

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use qantrip_core::{Choice, Engine, Locale, ParamDoc, ParamEdit, ParamKind, TemplateDoc};
use serde::{Deserialize, Serialize};

mod secrets;
mod sync;

pub(crate) fn opt_str(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: 调用方保证 p 指向以 NUL 结尾的字符串
    Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

pub(crate) fn str_or_empty(p: *const c_char) -> String {
    opt_str(p).unwrap_or_default()
}

pub(crate) fn out(s: Option<String>) -> *mut c_char {
    match s {
        // 字符串中不应出现 NUL；万一出现则去掉
        Some(s) => CString::new(s.replace('\0', "")).map(CString::into_raw).unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }
}

fn engine<'a>(e: *const Engine) -> Option<&'a Engine> {
    // SAFETY: e 来自 qt_engine_new，且尚未被 qt_engine_free 释放
    unsafe { e.as_ref() }
}

pub(crate) fn guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(default)
}

fn json<T: serde::Serialize>(v: &T) -> Option<String> {
    serde_json::to_string(v).ok()
}

/// 释放本库返回的字符串。
#[unsafe(no_mangle)]
pub extern "C" fn qt_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: s 由 CString::into_raw 产生
        drop(unsafe { CString::from_raw(s) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn qt_core_version() -> *mut c_char {
    out(Some(qantrip_core::core_version()))
}

/// 创建引擎。`user_dir`：用户模板目录；`usage_path`：使用记录文件。两者都可为空指针。
#[unsafe(no_mangle)]
pub extern "C" fn qt_engine_new(user_dir: *const c_char, usage_path: *const c_char) -> *const Engine {
    guard(std::ptr::null(), || Arc::into_raw(Engine::new(opt_str(user_dir), opt_str(usage_path))))
}

#[unsafe(no_mangle)]
pub extern "C" fn qt_engine_free(e: *const Engine) {
    if !e.is_null() {
        // SAFETY: e 来自 Arc::into_raw
        drop(unsafe { Arc::from_raw(e) });
    }
}

/// 设置界面语言与内置模板语言（`zh` / `en`，按前缀判断）。
#[unsafe(no_mangle)]
pub extern "C" fn qt_set_locale(e: *const Engine, ui: *const c_char, templates: *const c_char) {
    if let Some(e) = engine(e) {
        guard((), || e.set_locale(Locale::from_code(&str_or_empty(ui)), Locale::from_code(&str_or_empty(templates))));
    }
}

/// 面板打开时调用。`app`：前台应用标识（Windows 上为进程的 exe 名）。
#[unsafe(no_mangle)]
pub extern "C" fn qt_begin_session(e: *const Engine, app: *const c_char) {
    if let Some(e) = engine(e) {
        guard((), || e.begin_session(opt_str(app)));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn qt_reload(e: *const Engine) {
    if let Some(e) = engine(e) {
        guard((), || e.reload());
    }
}

/// 解析面板输入，返回 `Analysis` 的 JSON。
#[unsafe(no_mangle)]
pub extern "C" fn qt_analyze(e: *const Engine, input: *const c_char, selected: *const c_char) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || out(json(&e.analyze(str_or_empty(input), opt_str(selected)))))
}

/// 可插入的最终文本；输入有错误时返回空指针。
#[unsafe(no_mangle)]
pub extern "C" fn qt_render(e: *const Engine, input: *const c_char, selected: *const c_char) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || out(e.render(str_or_empty(input), opt_str(selected))))
}

#[unsafe(no_mangle)]
pub extern "C" fn qt_record_use(e: *const Engine, input: *const c_char, selected: *const c_char, app: *const c_char) {
    if let Some(e) = engine(e) {
        guard((), || e.record_use(str_or_empty(input), opt_str(selected), opt_str(app)));
    }
}

/// 鼠标操作参数。`edit`：`ParamEdit` 的 JSON（如 `{"kind":"toggle"}`）。返回 `EditResult` 的 JSON。
#[unsafe(no_mangle)]
pub extern "C" fn qt_edit_param(
    e: *const Engine,
    input: *const c_char,
    selected: *const c_char,
    name: *const c_char,
    edit: *const c_char,
) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(None, || {
        let edit: ParamEdit = serde_json::from_str(&str_or_empty(edit)).ok()?;
        e.edit_param(str_or_empty(input), opt_str(selected), str_or_empty(name), edit).and_then(|r| json(&r))
    })
    .map_or(std::ptr::null_mut(), |s| out(Some(s)))
}

/// 可见模板（`TemplateInfo` 数组）的 JSON。
#[unsafe(no_mangle)]
pub extern "C" fn qt_templates(e: *const Engine) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || out(json(&e.templates())))
}

/// 加载模板时发现的问题（字符串数组）的 JSON。
#[unsafe(no_mangle)]
pub extern "C" fn qt_problems(e: *const Engine) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || out(json(&e.problems())))
}

/// 清除使用记录。
#[unsafe(no_mangle)]
pub extern "C" fn qt_clear_usage(e: *const Engine) {
    if let Some(e) = engine(e) {
        guard((), || e.clear_usage());
    }
}

/// 指令模式 `/new 名称` 不写正文时使用的剪贴板文字；空指针表示没有文字。
#[unsafe(no_mangle)]
pub extern "C" fn qt_set_clipboard(e: *const Engine, text: *const c_char) {
    if let Some(e) = engine(e) {
        guard((), || e.set_clipboard(opt_str(text)));
    }
}

/// 执行指令（输入以 `/` 开头，docs/decisions.md D31）。
/// 返回 `{"ok":true,"outcome":CommandOutcome}` 或 `{"ok":false,"error":"…"}` 的 JSON。
#[unsafe(no_mangle)]
pub extern "C" fn qt_run_command(e: *const Engine, input: *const c_char) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || outcome_json(e.run_command(str_or_empty(input))))
}

/// 删除模板（模板库的"删除"，与 `/rm` 相同，docs/decisions.md D34）。返回值同 `qt_run_command`；
/// `trash_path` 由外壳移到回收站。
#[unsafe(no_mangle)]
pub extern "C" fn qt_delete_template(e: *const Engine, name: *const c_char) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || outcome_json(e.delete_template(str_or_empty(name))))
}

/// 恢复内置模板（撤销删除或修改，与 `/restore` 相同）。返回值同 `qt_run_command`。
#[unsafe(no_mangle)]
pub extern "C" fn qt_restore_template(e: *const Engine, name: *const c_char) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || outcome_json(e.restore_template(str_or_empty(name))))
}

fn outcome_json(r: Result<qantrip_core::command::CommandOutcome, qantrip_core::DocError>) -> *mut c_char {
    let v = match r {
        Ok(o) => serde_json::json!({ "ok": true, "outcome": o }),
        Err(err) => serde_json::json!({ "ok": false, "error": err.to_string() }),
    };
    out(Some(v.to_string()))
}

// MARK: - 模板库与编辑器（docs/decisions.md D19）

/// `ParamDoc` 的 JSON 形式（核心类型只有 UniFFI 派生，这里做一层可序列化的镜像）。
#[derive(Serialize, Deserialize)]
struct ParamJson {
    name: String,
    kind: ParamKind,
    #[serde(default)]
    description: String,
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    default_value: Option<String>,
    #[serde(default)]
    positional: bool,
    #[serde(default)]
    short: Option<String>,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    conflicts: Vec<String>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
}

/// `TemplateDoc` 的 JSON 形式。
#[derive(Serialize, Deserialize)]
struct DocJson {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    params: Vec<ParamJson>,
    #[serde(default)]
    body: String,
}

impl From<TemplateDoc> for DocJson {
    fn from(d: TemplateDoc) -> Self {
        let params = d
            .params
            .into_iter()
            .map(|p| ParamJson {
                name: p.name,
                kind: p.kind,
                description: p.description,
                choices: p.choices,
                default_value: p.default_value,
                positional: p.positional,
                short: p.short,
                required: p.required,
                requires: p.requires,
                conflicts: p.conflicts,
                min: p.min,
                max: p.max,
            })
            .collect();
        DocJson { name: d.name, aliases: d.aliases, description: d.description, category: d.category, params, body: d.body }
    }
}

impl From<DocJson> for TemplateDoc {
    fn from(d: DocJson) -> Self {
        let params = d
            .params
            .into_iter()
            .map(|p| ParamDoc {
                name: p.name,
                kind: p.kind,
                description: p.description,
                choices: p.choices,
                default_value: p.default_value,
                positional: p.positional,
                short: p.short,
                required: p.required,
                requires: p.requires,
                conflicts: p.conflicts,
                min: p.min,
                max: p.max,
            })
            .collect();
        TemplateDoc { name: d.name, aliases: d.aliases, description: d.description, category: d.category, params, body: d.body }
    }
}

/// 模板库全部条目（`TemplateEntry` 数组：内置、用户模板与公共片段，按模板库顺序）的 JSON。
#[unsafe(no_mangle)]
pub extern "C" fn qt_entries(e: *const Engine) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || {
        let v: Vec<serde_json::Value> = e
            .entries()
            .into_iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name, "aliases": t.aliases, "description": t.description, "category": t.category,
                    "builtin": t.builtin, "overrides_builtin": t.overrides_builtin, "partial": t.partial,
                    "path": t.path, "source": t.source, "deleted": t.deleted,
                })
            })
            .collect();
        out(json(&v))
    })
}

/// 内置模板的原始源码（"恢复默认"）；不是内置模板时返回空指针。
#[unsafe(no_mangle)]
pub extern "C" fn qt_builtin_source(e: *const Engine, name: *const c_char) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || out(e.builtin_source(str_or_empty(name))))
}

/// 校验编辑中的源码并以 `try_input` 试用。返回 `{"name","problems","doc","analysis"}` 的 JSON。
#[unsafe(no_mangle)]
pub extern "C" fn qt_check_source(e: *const Engine, source: *const c_char, try_input: *const c_char) -> *mut c_char {
    let Some(e) = engine(e) else { return std::ptr::null_mut() };
    guard(std::ptr::null_mut(), || {
        let c = e.check_source(str_or_empty(source), str_or_empty(try_input));
        let v = serde_json::json!({
            "name": c.name,
            "problems": c.problems,
            "doc": c.doc.map(DocJson::from),
            "analysis": c.analysis,
        });
        out(Some(v.to_string()))
    })
}

/// 源码 → 结构化文档。返回 `{"ok":true,"doc":…}` 或 `{"ok":false,"error":"…"}`。
#[unsafe(no_mangle)]
pub extern "C" fn qt_parse_doc(source: *const c_char) -> *mut c_char {
    guard(std::ptr::null_mut(), || {
        let v = match qantrip_core::parse_template_doc(str_or_empty(source)) {
            Ok(d) => serde_json::json!({ "ok": true, "doc": DocJson::from(d) }),
            Err(err) => serde_json::json!({ "ok": false, "error": err.to_string() }),
        };
        out(Some(v.to_string()))
    })
}

/// 结构化文档（JSON）→ 源码；JSON 无效时返回空指针。
#[unsafe(no_mangle)]
pub extern "C" fn qt_serialize_doc(doc: *const c_char) -> *mut c_char {
    guard(None, || {
        let d: DocJson = serde_json::from_str(&str_or_empty(doc)).ok()?;
        Some(qantrip_core::serialize_template_doc(d.into()))
    })
    .map_or(std::ptr::null_mut(), |s| out(Some(s)))
}

/// 新模板的初始文档（JSON）。`locale`：`zh` / `en`。
#[unsafe(no_mangle)]
pub extern "C" fn qt_new_doc(name: *const c_char, locale: *const c_char) -> *mut c_char {
    guard(std::ptr::null_mut(), || {
        let d = qantrip_core::new_template_doc(str_or_empty(name), Locale::from_code(&str_or_empty(locale)));
        out(json(&DocJson::from(d)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> CString {
        CString::new(v).unwrap()
    }

    fn take(p: *mut c_char) -> Option<String> {
        let r = opt_str(p);
        qt_string_free(p);
        r
    }

    #[test]
    fn roundtrip() {
        let e = qt_engine_new(std::ptr::null(), std::ptr::null());
        assert!(!e.is_null());
        qt_set_locale(e, s("en").as_ptr(), s("en").as_ptr());
        qt_begin_session(e, std::ptr::null());
        let a = take(qt_analyze(e, s("r --strict").as_ptr(), std::ptr::null())).unwrap();
        let v: serde_json::Value = serde_json::from_str(&a).unwrap();
        assert_eq!(v["template"]["name"], "review");
        assert_eq!(v["enter"]["kind"], "insert");
        assert!(take(qt_render(e, s("r --strict").as_ptr(), std::ptr::null())).is_some());
        assert!(take(qt_render(e, s("r --nope").as_ptr(), std::ptr::null())).is_none());
        let r = take(qt_edit_param(e, s("r ").as_ptr(), std::ptr::null(), s("strict").as_ptr(), s(r#"{"kind":"toggle"}"#).as_ptr()));
        assert!(r.unwrap().contains("--strict"));
        assert!(take(qt_templates(e)).unwrap().starts_with('['));
        qt_engine_free(e);
    }

    #[test]
    fn commands() {
        let dir = std::env::temp_dir().join(format!("qantrip-ffi-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = s(dir.to_str().unwrap());
        let e = qt_engine_new(d.as_ptr(), std::ptr::null());
        qt_set_locale(e, s("en").as_ptr(), s("en").as_ptr());
        // 还没选指令：帮助文字作为预览，没有模板
        let v: serde_json::Value = serde_json::from_str(&take(qt_analyze(e, s("/").as_ptr(), std::ptr::null())).unwrap()).unwrap();
        assert!(v["template"].is_null());
        assert!(v["preview"]["text"].as_str().unwrap().contains("/new"));
        // /new 不写正文时使用剪贴板
        qt_set_clipboard(e, s("Explain {topic} simply").as_ptr());
        let v: serde_json::Value = serde_json::from_str(&take(qt_analyze(e, s("/new explainer").as_ptr(), std::ptr::null())).unwrap()).unwrap();
        assert_eq!(v["enter"]["kind"], "run");
        assert_eq!(v["enter"]["confirm"], false);
        let r: serde_json::Value = serde_json::from_str(&take(qt_run_command(e, s("/new explainer").as_ptr())).unwrap()).unwrap();
        assert_eq!(r["ok"], true, "{r}");
        assert!(dir.join("explainer.md").exists());
        // /rm 需要确认，返回待移到回收站的路径
        let v: serde_json::Value = serde_json::from_str(&take(qt_analyze(e, s("/rm explainer").as_ptr(), std::ptr::null())).unwrap()).unwrap();
        assert_eq!(v["enter"]["confirm"], true);
        let r: serde_json::Value = serde_json::from_str(&take(qt_run_command(e, s("/rm explainer").as_ptr())).unwrap()).unwrap();
        assert!(r["outcome"]["trash_path"].as_str().unwrap().ends_with("explainer.md"));
        let r: serde_json::Value = serde_json::from_str(&take(qt_run_command(e, s("/rm nope").as_ptr())).unwrap()).unwrap();
        assert_eq!(r["ok"], false);
        assert!(r["error"].as_str().is_some());
        qt_clear_usage(e);
        qt_engine_free(e);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn parse(p: *mut c_char) -> serde_json::Value {
        serde_json::from_str(&take(p).unwrap()).unwrap()
    }

    #[test]
    fn delete_restore_builtin() {
        let dir = std::env::temp_dir().join(format!("qantrip-ffi-del-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = s(dir.to_str().unwrap());
        let e = qt_engine_new(d.as_ptr(), std::ptr::null());
        qt_set_locale(e, s("en").as_ptr(), s("en").as_ptr());
        // 删除内置模板：写入标记，不需要移到回收站；条目以 deleted 列出
        let r = parse(qt_delete_template(e, s("review").as_ptr()));
        assert_eq!(r["ok"], true, "{r}");
        assert!(r["outcome"]["trash_path"].is_null());
        let entries = parse(qt_entries(e));
        let review = entries.as_array().unwrap().iter().find(|t| t["name"] == "review").unwrap().clone();
        assert_eq!(review["deleted"], true);
        assert_eq!(review["builtin"], true);
        assert!(review["path"].as_str().unwrap().ends_with(".deleted.md"));
        // 恢复
        let r = parse(qt_restore_template(e, s("review").as_ptr()));
        assert_eq!(r["ok"], true, "{r}");
        let entries = parse(qt_entries(e));
        let review = entries.as_array().unwrap().iter().find(|t| t["name"] == "review").unwrap().clone();
        assert_eq!(review["deleted"], false);
        assert!(parse(qt_restore_template(e, s("review").as_ptr()))["ok"] == false);
        qt_engine_free(e);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library() {
        let dir = std::env::temp_dir().join(format!("qantrip-ffi-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = s(dir.to_str().unwrap());
        let e = qt_engine_new(d.as_ptr(), std::ptr::null());
        qt_set_locale(e, s("en").as_ptr(), s("en").as_ptr());
        let entries = parse(qt_entries(e));
        let list = entries.as_array().unwrap();
        let review = list.iter().find(|t| t["name"] == "review").unwrap();
        assert_eq!(review["builtin"], true);
        assert!(list.iter().any(|t| t["partial"] == true));
        let src = take(qt_builtin_source(e, s("review").as_ptr())).unwrap();
        assert_eq!(review["source"].as_str().unwrap(), src);
        assert!(take(qt_builtin_source(e, s("nope").as_ptr())).is_none());
        // 源码 → 文档 → 源码
        let p = parse(qt_parse_doc(s(&src).as_ptr()));
        assert_eq!(p["ok"], true);
        let mut doc = p["doc"].clone();
        assert_eq!(doc["name"], "review");
        doc["name"] = "my-review".into();
        doc["aliases"] = serde_json::json!([]); // 否则与内置 review 的别名 r 冲突
        let src2 = take(qt_serialize_doc(s(&doc.to_string()).as_ptr())).unwrap();
        assert!(src2.contains("name: my-review"));
        let back = parse(qt_parse_doc(s(&src2).as_ptr()));
        assert_eq!(back["doc"], doc);
        assert!(take(qt_serialize_doc(s("{").as_ptr())).is_none());
        assert_eq!(parse(qt_parse_doc(s("no frontmatter").as_ptr()))["ok"], false);
        // 校验与试用
        let c = parse(qt_check_source(e, s(&src2).as_ptr(), s("my-review --strict").as_ptr()));
        assert_eq!(c["name"], "my-review");
        assert_eq!(c["problems"].as_array().unwrap().len(), 0, "{c}");
        assert!(c["analysis"]["preview"]["text"].as_str().unwrap().len() > 10);
        let c = parse(qt_check_source(e, s("---\nname: bad\n---\n{% if %}").as_ptr(), s("").as_ptr()));
        assert!(!c["problems"].as_array().unwrap().is_empty());
        assert!(c["analysis"].is_null());
        // 新模板
        let n = parse(qt_new_doc(s("my-template").as_ptr(), s("zh").as_ptr()));
        assert_eq!(n["name"], "my-template");
        assert!(n["body"].as_str().unwrap().contains("在这里编写"));
        qt_engine_free(e);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_client_signed_out() {
        let dir = std::env::temp_dir().join(format!("qantrip-ffi-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("templates")).unwrap();
        let data = s(dir.join("sync").to_str().unwrap());
        let tpl = s(dir.join("templates").to_str().unwrap());
        let secrets = s(dir.join("sync").join("secrets.bin").to_str().unwrap());
        // 不可达的地址：构造与读取状态都不发网络请求
        let c = sync::qt_sync_new(
            s("http://127.0.0.1:9").as_ptr(),
            data.as_ptr(),
            tpl.as_ptr(),
            secrets.as_ptr(),
            s("TEST-PC").as_ptr(),
            s("Windows 11 26200").as_ptr(),
            s("0.0.0").as_ptr(),
        );
        assert!(!c.is_null());
        let call = |op: &str, args: &str| parse(sync::qt_sync_call(c, s(op).as_ptr(), s(args).as_ptr()));
        let st = call("status", "{}");
        assert_eq!(st["ok"], true, "{st}");
        assert_eq!(st["value"]["signed_in"], false);
        assert_eq!(st["value"]["has_vault"], false);
        assert_eq!(st["value"]["enrolled"], false);
        assert!(st["value"]["alerts"].as_array().unwrap().is_empty());
        // v0.4 新字段：未收到服务端策略前为默认值
        assert!(st["value"]["usage"].is_null());
        assert_eq!(st["value"]["pending_joins"], 0);
        assert_eq!(st["value"]["policy"]["check_interval_secs"], 3600);
        assert_eq!(st["value"]["policy"]["panel_check_secs"], 900);
        assert_eq!(st["value"]["policy"]["min_push_interval_secs"], 900);
        assert!(st["value"]["next_push_at"].is_null());
        assert!(st["value"]["device_limit"].is_null());
        // 未登录、没有保险库：自动触发不发请求，返回 null
        for t in ["timer", "panel_opened", "local_change", "launch", "wake"] {
            let r = call("maybe_sync", &format!(r#"{{"trigger":"{t}"}}"#));
            assert_eq!(r["ok"], true, "{t}: {r}");
            assert!(r["value"].is_null(), "{t}: {r}");
        }
        assert_eq!(call("maybe_sync", r#"{"trigger":"bogus"}"#)["error"]["kind"], "invalid_input");
        assert_eq!(call("maybe_sync", "{}")["error"]["kind"], "invalid_input");
        // 账号设备与用量需要登录；解除设备先校验参数
        assert_eq!(call("account_devices", "{}")["ok"], false);
        assert_eq!(call("account_usage", "{}")["ok"], false);
        assert_eq!(call("release_device", "{}")["error"]["kind"], "invalid_input");
        assert_eq!(call("release_device", r#"{"grant_id":"../x"}"#)["error"]["kind"], "invalid_input");
        // 需要登录的操作返回结构化错误
        let r = call("sync_now", "{}");
        assert_eq!(r["ok"], false);
        assert!(r["error"]["kind"].as_str().is_some(), "{r}");
        assert_eq!(call("bogus", "{}")["error"]["kind"], "invalid_input");
        assert_eq!(call("approve_join", "{}")["error"]["kind"], "invalid_input");
        // 登录第一步只生成授权地址（PKCE），不联网
        let r = call("begin_sign_in", r#"{"redirect_uri":"http://127.0.0.1:5555/callback"}"#);
        assert_eq!(r["ok"], true, "{r}");
        let url = r["value"].as_str().unwrap();
        assert!(url.starts_with("http://127.0.0.1:9/"), "{url}");
        assert!(url.contains("code_challenge"), "{url}");
        sync::qt_sync_free(c);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
