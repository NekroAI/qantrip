//! 记录明文（docs/sync-design.md 4.3–4.5）：确定性 CBOR，加密前按 §4.2 填充。
//!
//! ```text
//! { type, counter, deleted, writer, roster: { index, head }, time, body }
//! ```
//!
//! - 墓碑（`deleted = true`）的 body 为空映射，不保留任何内容。
//! - 内容哈希 = SHA-256(未填充的明文规范编码)，用于防回滚与分叉判定（§4.3 第 3 步、§4.4）。
//! - 设备状态记录的条目为 `record_id → [max_counter, content_hash, deleted]`。设计中只有前两项；
//!   增加 `deleted` 是为了区分"墓碑被服务端按 180 天规则清理"与"服务端隐藏记录"，否则墓碑清理后会持续误报。

use std::collections::BTreeMap;

use ciborium::Value;

use super::codec::{self, Fields};
use super::crypto;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Template,
    Setting,
    AiProvider,
    DeviceState,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Template => "template",
            Kind::Setting => "setting",
            Kind::AiProvider => "ai_provider",
            Kind::DeviceState => "device_state",
        }
    }

    fn parse(s: &str) -> Result<Kind, String> {
        Ok(match s {
            "template" => Kind::Template,
            "setting" => Kind::Setting,
            "ai_provider" => Kind::AiProvider,
            "device_state" => Kind::DeviceState,
            other => return Err(format!("未知的记录类型 `{other}`")),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateEntry {
    pub counter: u64,
    pub hash: [u8; 32],
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// 墓碑
    Empty,
    Template { file_name: String, source: String },
    /// 预留：可同步的偏好设置（尚未接入）
    Setting { key: String, value: Value },
    /// 预留：AI 供应商配置（尚未接入；接入时 API Key 只能写入系统安全存储）
    AiProvider { id: String, protocol: String, base_url: String, api_key: String, model: String, params: Value },
    DeviceState { entries: BTreeMap<String, StateEntry> },
}

impl Body {
    fn encode(&self) -> Value {
        use codec::{bytes, map, text, uint};
        match self {
            Body::Empty => Value::Map(vec![]),
            Body::Template { file_name, source } => map(vec![("file_name", text(file_name)), ("source", text(source))]),
            Body::Setting { key, value } => map(vec![("key", text(key)), ("value", value.clone())]),
            Body::AiProvider { id, protocol, base_url, api_key, model, params } => map(vec![
                ("id", text(id)),
                ("protocol", text(protocol)),
                ("base_url", text(base_url)),
                ("api_key", text(api_key)),
                ("model", text(model)),
                ("params", params.clone()),
            ]),
            Body::DeviceState { entries } => {
                let items = entries
                    .iter()
                    .map(|(id, e)| (text(id), Value::Array(vec![uint(e.counter), bytes(&e.hash), Value::Bool(e.deleted)])))
                    .collect();
                map(vec![("entries", Value::Map(items))])
            }
        }
    }

    fn parse(kind: Kind, deleted: bool, v: Value) -> Result<Body, String> {
        if deleted {
            return match v {
                Value::Map(m) if m.is_empty() && kind != Kind::DeviceState => Ok(Body::Empty),
                _ => Err("墓碑的内容必须为空".into()),
            };
        }
        Ok(match kind {
            Kind::Template => {
                let mut f = Fields::exact(v, &["file_name", "source"])?;
                Body::Template { file_name: f.text("file_name")?, source: f.text("source")? }
            }
            Kind::Setting => {
                let mut f = Fields::exact(v, &["key", "value"])?;
                Body::Setting { key: f.text("key")?, value: f.value("value") }
            }
            Kind::AiProvider => {
                let mut f = Fields::exact(v, &["id", "protocol", "base_url", "api_key", "model", "params"])?;
                Body::AiProvider {
                    id: f.text("id")?,
                    protocol: f.text("protocol")?,
                    base_url: f.text("base_url")?,
                    api_key: f.text("api_key")?,
                    model: f.text("model")?,
                    params: f.value("params"),
                }
            }
            Kind::DeviceState => {
                let Value::Map(items) = Fields::exact(v, &["entries"])?.value("entries") else {
                    return Err("状态记录的 entries 应为映射".into());
                };
                let mut entries = BTreeMap::new();
                for (k, v) in items {
                    let Value::Text(id) = k else { return Err("状态记录的键应为文本".into()) };
                    let Value::Array(t) = v else { return Err("状态记录条目应为数组".into()) };
                    let [c, h, d]: [Value; 3] = t.try_into().map_err(|_| "状态记录条目应有 3 项".to_string())?;
                    let counter = codec::as_u64(&c).ok_or("状态记录的 counter 不正确")?;
                    let hash: [u8; 32] = codec::as_bytes(h)?.try_into().map_err(|_| "状态记录的哈希长度不正确".to_string())?;
                    let Value::Bool(deleted) = d else { return Err("状态记录的删除标记不正确".into()) };
                    entries.insert(id, StateEntry { counter, hash, deleted });
                }
                Body::DeviceState { entries }
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plaintext {
    pub kind: Kind,
    pub counter: u64,
    pub deleted: bool,
    pub writer: String,
    pub roster_index: u32,
    pub roster_head: [u8; 32],
    pub time: u64,
    pub body: Body,
}

impl Plaintext {
    pub fn encode(&self) -> Vec<u8> {
        use codec::{bytes, map, text, uint};
        codec::encode(&map(vec![
            ("type", text(self.kind.as_str())),
            ("counter", uint(self.counter)),
            ("deleted", Value::Bool(self.deleted)),
            ("writer", text(&self.writer)),
            ("roster", map(vec![("index", uint(self.roster_index as u64)), ("head", bytes(&self.roster_head))])),
            ("time", uint(self.time)),
            ("body", self.body.encode()),
        ]))
    }

    /// 解码带填充的明文：规范 CBOR 之后只能是零字节。返回明文与内容哈希。
    pub fn decode_padded(padded: &[u8]) -> Result<(Plaintext, [u8; 32]), String> {
        let (v, used) = codec::decode_prefix(padded)?;
        if padded[used..].iter().any(|b| *b != 0) {
            return Err("明文填充不正确".into());
        }
        if padded.len() != crypto::padded_len(used) {
            return Err("明文填充长度不正确".into());
        }
        let hash = crypto::sha256(&[&padded[..used]]);
        let mut f = Fields::exact(v, &["type", "counter", "deleted", "writer", "roster", "time", "body"])?;
        let kind = Kind::parse(&f.text("type")?)?;
        let deleted = f.bool("deleted")?;
        let mut roster = Fields::exact(f.value("roster"), &["index", "head"])?;
        let pt = Plaintext {
            kind,
            counter: f.u64("counter")?,
            deleted,
            writer: f.text("writer")?,
            roster_index: roster.u32("index")?,
            roster_head: roster.array("head")?,
            time: f.u64("time")?,
            body: Body::parse(kind, deleted, f.value("body"))?,
        };
        Ok((pt, hash))
    }

    pub fn content_hash(&self) -> [u8; 32] {
        crypto::sha256(&[&self.encode()])
    }
}
