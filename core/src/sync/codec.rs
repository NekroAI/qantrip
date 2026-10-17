//! 编码工具：确定性 CBOR（RFC 8949 §4.2.1）、base64url、Base32。
//!
//! 签名、哈希与加密的输入都必须是规范编码：编码前递归排序映射的键，解码时要求输入与重新编码的结果逐字节相同，
//! 从而拒绝重复键、非最短整数、不定长等所有非规范形式。

use std::collections::BTreeMap;

use ciborium::Value;
use data_encoding::{BASE32_NOPAD, BASE64URL_NOPAD};

pub fn b64u(bytes: &[u8]) -> String {
    BASE64URL_NOPAD.encode(bytes)
}

pub fn from_b64u(text: &str) -> Result<Vec<u8>, String> {
    BASE64URL_NOPAD.decode(text.as_bytes()).map_err(|_| "数据不是合法的 base64url 编码".to_string())
}

pub fn base32(bytes: &[u8]) -> String {
    BASE32_NOPAD.encode(bytes)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// MARK: - 确定性 CBOR

fn raw(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(v, &mut out).expect("写入内存不会失败");
    out
}

/// 递归排序映射的键（按键的编码字节序）。
fn canonical(v: Value) -> Value {
    match v {
        Value::Map(entries) => {
            let mut keyed: Vec<(Vec<u8>, Value, Value)> = entries
                .into_iter()
                .map(|(k, v)| {
                    let k = canonical(k);
                    (raw(&k), k, canonical(v))
                })
                .collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Map(keyed.into_iter().map(|(_, k, v)| (k, v)).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
        other => other,
    }
}

pub fn encode(v: &Value) -> Vec<u8> {
    raw(&canonical(v.clone()))
}

/// 只允许本协议用到的类型；拒绝浮点、标签与重复键。
fn check(v: &Value) -> Result<(), String> {
    match v {
        Value::Integer(_) | Value::Bytes(_) | Value::Text(_) | Value::Bool(_) | Value::Null => Ok(()),
        Value::Array(items) => items.iter().try_for_each(check),
        Value::Map(entries) => {
            let mut keys: Vec<Vec<u8>> = entries.iter().map(|(k, _)| raw(k)).collect();
            keys.sort();
            if keys.windows(2).any(|w| w[0] == w[1]) {
                return Err("CBOR 映射包含重复的键".into());
            }
            entries.iter().try_for_each(|(k, v)| check(k).and_then(|_| check(v)))
        }
        _ => Err("CBOR 包含不支持的类型".into()),
    }
}

/// 严格解码：输入必须恰好是一个规范编码的数据项。
pub fn decode(bytes: &[u8]) -> Result<Value, String> {
    let (v, used) = decode_prefix(bytes)?;
    if used != bytes.len() {
        return Err("CBOR 数据之后有多余字节".into());
    }
    Ok(v)
}

/// 解码开头的一个规范数据项，返回它和占用的字节数（用于去掉记录明文的长度填充）。
pub fn decode_prefix(bytes: &[u8]) -> Result<(Value, usize), String> {
    let mut rest = bytes;
    let v: Value = ciborium::from_reader(&mut rest).map_err(|_| "CBOR 解码失败".to_string())?;
    let used = bytes.len() - rest.len();
    check(&v)?;
    if encode(&v) != bytes[..used] {
        return Err("CBOR 不是规范编码".into());
    }
    Ok((v, used))
}

// MARK: - 构造与读取

pub fn text(s: &str) -> Value {
    Value::Text(s.to_string())
}

pub fn bytes(b: &[u8]) -> Value {
    Value::Bytes(b.to_vec())
}

pub fn uint(n: u64) -> Value {
    Value::Integer(n.into())
}

pub fn map(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(entries.into_iter().map(|(k, v)| (text(k), v)).collect())
}

/// 以文本为键的映射，读取时要求键集合与预期完全一致（多一个、少一个都拒绝）。
pub struct Fields(BTreeMap<String, Value>);

impl Fields {
    pub fn exact(v: Value, keys: &[&str]) -> Result<Fields, String> {
        let Value::Map(entries) = v else { return Err("CBOR 应为映射".into()) };
        let mut out = BTreeMap::new();
        for (k, v) in entries {
            let Value::Text(k) = k else { return Err("CBOR 映射的键应为文本".into()) };
            // 不回显字段名：它可能来自服务端（错误信息不得包含服务端可控的文本）
            if !keys.contains(&k.as_str()) {
                return Err("包含未知字段".into());
            }
            out.insert(k, v);
        }
        if let Some(missing) = keys.iter().find(|k| !out.contains_key(**k)) {
            return Err(format!("缺少字段 `{missing}`"));
        }
        Ok(Fields(out))
    }

    pub fn value(&mut self, k: &str) -> Value {
        self.0.remove(k).expect("exact 已检查字段存在")
    }

    pub fn text(&mut self, k: &str) -> Result<String, String> {
        match self.value(k) {
            Value::Text(s) => Ok(s),
            _ => Err(format!("字段 `{k}` 应为文本")),
        }
    }

    pub fn bytes(&mut self, k: &str) -> Result<Vec<u8>, String> {
        as_bytes(self.value(k)).map_err(|_| format!("字段 `{k}` 应为字节串"))
    }

    pub fn array<const N: usize>(&mut self, k: &str) -> Result<[u8; N], String> {
        self.bytes(k)?.try_into().map_err(|_| format!("字段 `{k}` 长度应为 {N} 字节"))
    }

    pub fn u64(&mut self, k: &str) -> Result<u64, String> {
        as_u64(&self.value(k)).ok_or_else(|| format!("字段 `{k}` 应为非负整数"))
    }

    pub fn u32(&mut self, k: &str) -> Result<u32, String> {
        u32::try_from(self.u64(k)?).map_err(|_| format!("字段 `{k}` 超出范围"))
    }

    pub fn bool(&mut self, k: &str) -> Result<bool, String> {
        match self.value(k) {
            Value::Bool(b) => Ok(b),
            _ => Err(format!("字段 `{k}` 应为布尔值")),
        }
    }
}

pub fn as_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Integer(i) => u64::try_from(*i).ok(),
        _ => None,
    }
}

pub fn as_bytes(v: Value) -> Result<Vec<u8>, String> {
    match v {
        Value::Bytes(b) => Ok(b),
        _ => Err("应为字节串".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_order_and_strict_decode() {
        // 键按编码字节序：短键在前
        let v = map(vec![("bb", uint(1)), ("a", uint(300)), ("c", bytes(&[1, 2]))]);
        let enc = encode(&v);
        assert_eq!(hex(&enc), "a3616119012c616342010262626201");
        assert_eq!(decode(&enc).unwrap(), canonical(v));

        // 非规范：键顺序错误
        let mut bad = Vec::new();
        ciborium::into_writer(&map(vec![("bb", uint(1)), ("a", uint(2))]), &mut bad).unwrap();
        assert!(decode(&bad).is_err());
        // 非最短整数：1 编码为 0x18 0x01
        assert!(decode(&[0x18, 0x01]).is_err());
        // 多余字节
        assert!(decode(&[0x01, 0x00]).is_err());
        // 重复键
        assert!(decode(&[0xa2, 0x61, 0x61, 0x01, 0x61, 0x61, 0x02]).is_err());
        // 浮点
        assert!(decode(&[0xf9, 0x3c, 0x00]).is_err());
        // 前缀解码用于去填充
        let (_, used) = decode_prefix(&[0x01, 0x00, 0x00]).unwrap();
        assert_eq!(used, 1);
    }

    #[test]
    fn exact_fields() {
        let v = map(vec![("a", uint(1)), ("b", text("x"))]);
        assert!(Fields::exact(v.clone(), &["a"]).is_err());
        assert!(Fields::exact(v.clone(), &["a", "b", "c"]).is_err());
        let mut f = Fields::exact(v, &["a", "b"]).unwrap();
        assert_eq!(f.u64("a").unwrap(), 1);
        assert_eq!(f.text("b").unwrap(), "x");
    }
}
