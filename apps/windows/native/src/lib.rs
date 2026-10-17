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

use qantrip_core::{Engine, Locale, ParamEdit};

fn opt_str(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: 调用方保证 p 指向以 NUL 结尾的字符串
    Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

fn str_or_empty(p: *const c_char) -> String {
    opt_str(p).unwrap_or_default()
}

fn out(s: Option<String>) -> *mut c_char {
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

fn guard<T>(default: T, f: impl FnOnce() -> T) -> T {
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
}
