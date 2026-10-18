//! 埋点日志：每个事件一行 JSON，写到 `~/.local/state/qantrip/logs/events.jsonl`（与 macOS 的 spike.jsonl 对应）。
//! 只记录时间、长度、模式等指标，不记录 Prompt 正文与附加要求。

use std::io::Write;

use serde_json::{Map, Value};

use crate::paths;
use crate::util;

const MAX_BYTES: u64 = 2 * 1024 * 1024;

pub fn event(name: &str, fields: Value) {
    let mut obj = match fields {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    obj.insert("event".into(), Value::String(name.to_string()));
    obj.insert("uptime".into(), serde_json::json!((util::uptime() * 1000.0).round() / 1000.0));
    obj.insert("time".into(), serde_json::json!(util::now_secs()));
    let dir = paths::state_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("events.jsonl");
    // 超过上限时轮换一次，保留上一份
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("events.1.jsonl"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", Value::Object(obj));
    }
}
