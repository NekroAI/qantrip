//! 服务端接口（apps/cloud/server/api.ts）。所有二进制字段为 base64url（无填充）。
//!
//! HTTP 由 [`Transport`] 抽象：正式环境用 `ureq`（rustls、阻塞式，调用方在后台线程调用），测试用进程内的假服务端。
//!
//! 错误信息一律由本地固定文本组成，不包含服务端返回的任何文本（服务端不可信，其文本可能用于诱导用户）；
//! 服务端的错误码经过字符集与长度检查后保留在 `SyncError::Server.code` 中供程序判断。

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value as Json, json};
use zeroize::Zeroizing;

use super::{SecretStore, SyncError};

pub const SECRET_ACCESS: &str = "access_token";
pub const SECRET_REFRESH: &str = "refresh_token";
pub const CLIENT_ID: &str = "qantrip-desktop";

/// 花名册分页上限（L3）：单次响应的条目数、单个条目的字节数、总条目数
const ROSTER_PAGE_MAX: usize = 1000;
const ROSTER_ENTRY_MAX_BYTES: usize = 16 * 1024;
pub const ROSTER_MAX_ENTRIES: u32 = 10_000;
/// 单页记录数上限（请求的 limit 为 200）
const CHANGES_PAGE_MAX: usize = 200;

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// 一次 HTTP 请求。`path` 以 `/` 开头，含查询串；有 `json` 时以 `application/json` 发送。
pub trait Transport: Send + Sync {
    fn send(&self, method: &str, path: &str, bearer: Option<&str>, json: Option<&Json>) -> Result<Response, String>;
}

pub struct UreqTransport {
    base: String,
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new(base: &str) -> UreqTransport {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .user_agent(concat!("qantrip/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        UreqTransport { base: base.trim_end_matches('/').to_string(), agent }
    }
}

impl Transport for UreqTransport {
    fn send(&self, method: &str, path: &str, bearer: Option<&str>, json: Option<&Json>) -> Result<Response, String> {
        let url = format!("{}{}", self.base, path);
        let auth = bearer.map(|t| Zeroizing::new(format!("Bearer {t}")));
        let result = match (method, json) {
            ("GET", _) | ("DELETE", None) => {
                let mut req = if method == "GET" { self.agent.get(&url) } else { self.agent.delete(&url) };
                if let Some(a) = &auth {
                    req = req.header("authorization", a.as_str());
                }
                req.call()
            }
            (_, body) => {
                let mut req = match method {
                    "POST" => self.agent.post(&url),
                    "PUT" => self.agent.put(&url),
                    _ => return Err(format!("不支持的请求方法 {method}")),
                };
                if let Some(a) = &auth {
                    req = req.header("authorization", a.as_str());
                }
                let body = body.map(|b| b.to_string()).unwrap_or_default();
                req.header("content-type", "application/json").send(body.as_bytes())
            }
        };
        let mut resp = result.map_err(network_message)?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec().map_err(network_message)?;
        Ok(Response { status, body })
    }
}

/// 网络错误转为固定的本地文本（不回显库或服务端给出的细节）。
fn network_message(e: ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "网络请求超时",
        ureq::Error::HostNotFound => "无法解析服务器地址",
        ureq::Error::ConnectionFailed => "无法连接服务器",
        ureq::Error::BodyExceedsLimit(_) => "服务端响应过大",
        ureq::Error::Io(_) => "网络连接中断",
        _ => "网络请求失败",
    }
    .to_string()
}

// MARK: - 接口数据

#[derive(Debug, Clone, Deserialize, serde::Serialize, PartialEq)]
pub struct WireRecord {
    pub record_id: String,
    pub seq: u64,
    pub epoch: u32,
    pub ciphertext: String,
    pub deleted: bool,
}

#[derive(Debug, Deserialize)]
pub struct VaultInfo {
    pub vault_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Head {
    pub seq: u64,
    pub roster_index: u32,
    #[allow(dead_code)]
    pub min_epoch: u32,
}

#[derive(Debug, Deserialize)]
pub struct ChangesPage {
    pub records: Vec<WireRecord>,
    pub next: Option<u64>,
    pub head: u64,
    #[serde(default)]
    pub purged_seq: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WriteReq {
    pub record_id: String,
    pub base_seq: Option<u64>,
    pub epoch: u32,
    pub ciphertext: String,
    pub deleted: bool,
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize)]
pub struct WriteResult {
    pub record_id: String,
    pub status: String,
    pub seq: Option<u64>,
    pub current: Option<WireRecord>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RosterRow {
    index: u32,
    entry: String,
}

#[derive(Debug, Deserialize)]
pub struct WireEnvelope {
    pub epoch: u32,
    pub roster_index: u32,
    pub data: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JoinView {
    pub join_id: String,
    pub commit: Option<String>,
    pub reply: Option<String>,
    pub reveal: Option<String>,
    pub result: Option<String>,
    pub expires_at: i64,
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: TokenUser,
}

#[derive(Deserialize)]
pub struct TokenUser {
    pub login: String,
}

fn parse<T: for<'de> Deserialize<'de>>(v: Json) -> Result<T, SyncError> {
    // serde 的错误信息会引用响应内容，不回显
    serde_json::from_value(v).map_err(|_| SyncError::Security { message: "服务端响应格式不正确".into() })
}

pub fn decode_b64(s: &str) -> Result<Vec<u8>, SyncError> {
    super::codec::from_b64u(s).map_err(|message| SyncError::Security { message })
}

/// 服务端分配的 ID（保险库、加入请求）只能是 `[A-Za-z0-9_-]{1,64}`：它们会出现在请求路径与界面中。
pub fn valid_server_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn checked_id(id: Option<String>) -> Result<Option<String>, SyncError> {
    match id {
        Some(id) if !valid_server_id(&id) => Err(SyncError::Security { message: "服务端返回的 ID 格式不正确".into() }),
        other => Ok(other),
    }
}

fn too_large() -> SyncError {
    SyncError::Security { message: "服务端返回的数据超出上限".into() }
}

// MARK: - 客户端

pub struct Api {
    transport: Arc<dyn Transport>,
    secrets: Arc<dyn SecretStore>,
}

/// 服务端错误：错误码只保留 `[a-z0-9_]{1,64}`，说明文字取本地固定文本（不使用服务端的 `message`）。
pub fn server_error(status: u16, body: &[u8]) -> SyncError {
    let v: Json = serde_json::from_slice(body).unwrap_or(Json::Null);
    let code = v["error"]
        .as_str()
        .filter(|c| !c.is_empty() && c.len() <= 64 && c.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
        .unwrap_or("unknown")
        .to_string();
    let message = match code.as_str() {
        "unauthorized" | "token_expired" | "invalid_grant" => "登录已失效，请重新登录",
        "rate_limited" => "请求过于频繁，请稍后再试",
        "vault_not_found" => "保险库不存在",
        "join_in_progress" => "已有一个进行中的加入请求，请完成或等待其过期（10 分钟）",
        "join_not_found" => "加入请求不存在或已过期",
        "join_step_conflict" => "加入流程的状态不符，请重新开始",
        "roster_conflict" => "花名册已被其他设备更新",
        "too_many_devices" => "在册设备数量已达上限",
        "roster_full" | "too_many_envelopes" => "保险库的条目数量已达上限",
        "too_many_vaults" => "今天创建的保险库过多，请明天再试",
        "message_too_large" | "entry_too_large" | "envelope_too_large" | "batch_too_large" => "数据超过服务端的大小限制",
        "login_unavailable" | "github_error" => "登录服务暂时不可用",
        "internal" => "服务端内部错误",
        _ => "",
    };
    let message = if message.is_empty() { format!("服务端返回错误（HTTP {status}）") } else { message.to_string() };
    SyncError::Server { status, code, message }
}

impl Api {
    pub fn new(transport: Arc<dyn Transport>, secrets: Arc<dyn SecretStore>) -> Api {
        Api { transport, secrets }
    }

    fn secret(&self, key: &str) -> Result<Option<Zeroizing<String>>, SyncError> {
        let v = self.secrets.get(key.to_string())?;
        Ok(v.map(|b| Zeroizing::new(String::from_utf8_lossy(&b).to_string())))
    }

    pub fn signed_in(&self) -> Result<bool, SyncError> {
        Ok(self.secret(SECRET_REFRESH)?.is_some())
    }

    pub fn store_tokens(&self, t: &TokenResponse) -> Result<(), SyncError> {
        self.secrets.set(SECRET_ACCESS.into(), t.access_token.as_bytes().to_vec())?;
        self.secrets.set(SECRET_REFRESH.into(), t.refresh_token.as_bytes().to_vec())?;
        Ok(())
    }

    pub fn clear_tokens(&self) -> Result<(), SyncError> {
        self.secrets.delete(SECRET_ACCESS.into())?;
        self.secrets.delete(SECRET_REFRESH.into())?;
        Ok(())
    }

    fn send(&self, method: &str, path: &str, bearer: Option<&str>, body: Option<&Json>) -> Result<Response, SyncError> {
        self.transport.send(method, path, bearer, body).map_err(|message| SyncError::Network { message })
    }

    /// 不需要登录的接口（令牌端点）。
    pub fn public(&self, method: &str, path: &str, body: Option<&Json>) -> Result<Json, SyncError> {
        let r = self.send(method, path, None, body)?;
        if !(200..300).contains(&r.status) {
            return Err(server_error(r.status, &r.body));
        }
        Ok(serde_json::from_slice(&r.body).unwrap_or(Json::Null))
    }

    /// 用刷新令牌换新令牌（刷新令牌会轮换）。
    fn refresh(&self) -> Result<(), SyncError> {
        let Some(refresh) = self.secret(SECRET_REFRESH)? else { return Err(SyncError::not_signed_in()) };
        let body = json!({ "grant_type": "refresh_token", "refresh_token": refresh.as_str() });
        match self.public("POST", "/api/v1/oauth/token", Some(&body)) {
            Ok(v) => self.store_tokens(&parse(v)?),
            Err(SyncError::Server { status: 400, .. }) => {
                self.clear_tokens()?;
                Err(SyncError::not_signed_in())
            }
            Err(e) => Err(e),
        }
    }

    /// 需要登录的接口：返回状态码与 JSON。401 `token_expired` 时自动刷新一次。非 2xx 转为错误。
    pub fn call(&self, method: &str, path: &str, body: Option<&Json>) -> Result<(u16, Json), SyncError> {
        let mut refreshed = false;
        loop {
            let Some(access) = self.secret(SECRET_ACCESS)? else { return Err(SyncError::not_signed_in()) };
            let r = self.send(method, path, Some(&access), body)?;
            if (200..300).contains(&r.status) {
                return Ok((r.status, serde_json::from_slice(&r.body).unwrap_or(Json::Null)));
            }
            let err = server_error(r.status, &r.body);
            match &err {
                SyncError::Server { status: 401, code, .. } if code == "token_expired" && !refreshed => {
                    self.refresh()?;
                    refreshed = true;
                }
                SyncError::Server { status: 401, .. } => return Err(SyncError::not_signed_in()),
                _ => return Err(err),
            }
        }
    }

    fn get(&self, path: &str) -> Result<Json, SyncError> {
        self.call("GET", path, None).map(|r| r.1)
    }

    // MARK: 令牌

    pub fn exchange_code(&self, code: &str, redirect_uri: &str, device_id: &str, verifier: &str) -> Result<TokenResponse, SyncError> {
        let body = json!({
            "grant_type": "authorization_code", "code": code, "redirect_uri": redirect_uri,
            "device_id": device_id, "code_verifier": verifier, "client_id": CLIENT_ID,
        });
        parse(self.public("POST", "/api/v1/oauth/token", Some(&body))?)
    }

    pub fn revoke(&self) -> Result<(), SyncError> {
        if let Some(refresh) = self.secret(SECRET_REFRESH)? {
            let _ = self.public("POST", "/api/v1/oauth/revoke", Some(&json!({ "token": refresh.as_str() })));
        }
        self.clear_tokens()
    }

    // MARK: 保险库与记录

    pub fn vault(&self) -> Result<Option<String>, SyncError> {
        checked_id(parse::<VaultInfo>(self.get("/api/v1/vault")?)?.vault_id)
    }

    pub fn create_vault(&self) -> Result<String, SyncError> {
        let (_, v) = self.call("POST", "/api/v1/vault", Some(&json!({})))?;
        checked_id(parse::<VaultInfo>(v)?.vault_id)?.ok_or_else(|| SyncError::Security { message: "服务端未返回保险库 ID".into() })
    }

    pub fn head(&self, vault: &str) -> Result<Head, SyncError> {
        parse(self.get(&format!("/api/v1/vaults/{vault}/head"))?)
    }

    pub fn changes(&self, vault: &str, since: u64) -> Result<ChangesPage, SyncError> {
        let page: ChangesPage = parse(self.get(&format!("/api/v1/vaults/{vault}/changes?since={since}&limit=200"))?)?;
        if page.records.len() > CHANGES_PAGE_MAX {
            return Err(too_large());
        }
        Ok(page)
    }

    pub fn push(&self, vault: &str, writes: &[WriteReq]) -> Result<Vec<WriteResult>, SyncError> {
        let (_, v) = self.call("POST", &format!("/api/v1/vaults/{vault}/records"), Some(&json!({ "writes": writes })))?;
        let results: Vec<WriteResult> = parse(v["results"].clone())?;
        if results.len() != writes.len() || results.iter().zip(writes).any(|(r, w)| r.record_id != w.record_id) {
            return Err(SyncError::Security { message: "服务端返回的写入结果与请求不符".into() });
        }
        Ok(results)
    }

    pub fn raise_min_epoch(&self, vault: &str, epoch: u32) -> Result<(), SyncError> {
        self.call("POST", &format!("/api/v1/vaults/{vault}/min-epoch"), Some(&json!({ "epoch": epoch }))).map(|_| ())
    }

    // MARK: 花名册

    /// 拉取 `since` 之后的全部条目（服务端每次最多返回 1000 条）。返回条目与服务端声称的长度。
    /// 单次响应、单个条目与总条目数都有上限（L3），超出视为服务端异常。
    pub fn roster(&self, vault: &str, since: u32) -> Result<(Vec<Vec<u8>>, u32), SyncError> {
        let mut out = Vec::new();
        loop {
            let from = since + out.len() as u32;
            let v = self.get(&format!("/api/v1/vaults/{vault}/roster?since={from}"))?;
            let rows: Vec<RosterRow> = parse(v["entries"].clone())?;
            let length = v["length"].as_u64().unwrap_or(0);
            if length > ROSTER_MAX_ENTRIES as u64 || rows.len() > ROSTER_PAGE_MAX || from as usize + rows.len() > ROSTER_MAX_ENTRIES as usize {
                return Err(too_large());
            }
            let length = length as u32;
            if rows.is_empty() {
                return Ok((out, length));
            }
            for (i, row) in rows.iter().enumerate() {
                if row.index != from + i as u32 {
                    return Err(SyncError::Security { message: "服务端返回的花名册序号不连续".into() });
                }
                if row.entry.len() > ROSTER_ENTRY_MAX_BYTES * 4 / 3 + 4 {
                    return Err(too_large());
                }
                out.push(decode_b64(&row.entry)?);
            }
            if since + out.len() as u32 >= length {
                return Ok((out, length));
            }
        }
    }

    /// CAS 追加：成功返回 true，409（他人已追加）返回 false。
    pub fn append_roster(&self, vault: &str, expected: u32, entry: &[u8]) -> Result<bool, SyncError> {
        let body = json!({ "expected_index": expected, "entry": super::codec::b64u(entry) });
        match self.call("POST", &format!("/api/v1/vaults/{vault}/roster"), Some(&body)) {
            Ok(_) => Ok(true),
            Err(SyncError::Server { status: 409, .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    // MARK: 信封

    pub fn put_envelope(&self, vault: &str, recipient: &str, epoch: u32, roster_index: u32, data: &[u8]) -> Result<(), SyncError> {
        let body = json!({ "roster_index": roster_index, "data": super::codec::b64u(data) });
        self.call("PUT", &format!("/api/v1/vaults/{vault}/envelopes/{recipient}/{epoch}"), Some(&body)).map(|_| ())
    }

    pub fn envelopes(&self, vault: &str, recipient: &str) -> Result<Vec<WireEnvelope>, SyncError> {
        parse(self.get(&format!("/api/v1/vaults/{vault}/envelopes/{recipient}"))?["envelopes"].clone())
    }

    // MARK: 加入

    pub fn create_join(&self, vault: &str, data: &[u8]) -> Result<String, SyncError> {
        let (_, v) = self.call("POST", &format!("/api/v1/vaults/{vault}/joins"), Some(&json!({ "data": super::codec::b64u(data) })))?;
        checked_id(v["join_id"].as_str().map(str::to_string))?.ok_or_else(|| SyncError::Security { message: "服务端未返回加入请求 ID".into() })
    }

    pub fn joins(&self, vault: &str) -> Result<Vec<JoinView>, SyncError> {
        parse(self.get(&format!("/api/v1/vaults/{vault}/joins"))?["joins"].clone())
    }

    pub fn join(&self, vault: &str, join_id: &str) -> Result<Option<JoinView>, SyncError> {
        match self.get(&format!("/api/v1/vaults/{vault}/joins/{join_id}")) {
            Ok(v) => Ok(Some(parse(v)?)),
            Err(SyncError::Server { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// 写入加入流程的一段消息；409（已被写入或顺序不符）返回 false。
    pub fn put_join_step(&self, vault: &str, join_id: &str, step: &str, data: &[u8]) -> Result<bool, SyncError> {
        let body = json!({ "data": super::codec::b64u(data) });
        match self.call("PUT", &format!("/api/v1/vaults/{vault}/joins/{join_id}/{step}"), Some(&body)) {
            Ok(_) => Ok(true),
            Err(SyncError::Server { status: 409, .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn delete_join(&self, vault: &str, join_id: &str) -> Result<(), SyncError> {
        self.call("DELETE", &format!("/api/v1/vaults/{vault}/joins/{join_id}"), None).map(|_| ())
    }
}
