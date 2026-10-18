//! 进程内的假服务端：按 apps/cloud/server/api.ts、sync.ts、auth.ts 的契约实现（内存存储，含 CAS、冲突、幂等、gone）。
//! 测试可以直接修改 [`World`]，或用 `hook` 改写响应，模拟恶意服务端。

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use serde_json::{Value as Json, json};

use crate::sync::codec::{b64u, from_b64u};
use crate::sync::crypto;
use crate::sync::net::{Response, Transport};

#[derive(Debug, Clone)]
pub struct Rec {
    pub seq: u64,
    pub epoch: u32,
    pub ct: Vec<u8>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Join {
    pub id: String,
    /// 发起设备（版本检查的 pending_joins 不计入本机发起的请求）
    pub creator: String,
    pub commit: Vec<u8>,
    pub reply: Option<Vec<u8>>,
    pub reveal: Option<Vec<u8>>,
    pub result: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
pub struct Vault {
    pub id: String,
    pub seq: u64,
    pub roster: Vec<Vec<u8>>,
    pub min_epoch: u32,
    pub records: BTreeMap<String, Rec>,
    pub envelopes: BTreeMap<(String, u32), (u32, Vec<u8>)>,
    pub joins: Vec<Join>,
    pub idem: HashMap<String, Json>,
    pub purged_seq: u64,
    /// 测试辅助：下一次花名册追加成功后立即插入这条记录（模拟服务端在轮换窗口内注入）
    pub inject_after_roster: Option<(String, Rec)>,
}

impl Vault {
    /// 测试辅助：把一条记录改为指定密文并分配新 seq（模拟服务端改写）。
    pub fn put(&mut self, record_id: &str, epoch: u32, ct: Vec<u8>, deleted: bool) {
        self.seq += 1;
        self.records.insert(record_id.to_string(), Rec { seq: self.seq, epoch, ct, deleted });
    }
}

pub struct Grant {
    pub id: String,
    pub device_id: String,
    pub access: String,
    pub refresh: String,
    pub revoked: bool,
    pub expired: bool,
    /// 超出设备数量限制
    pub limited: bool,
    /// 登录或刷新时上报的设备信息
    pub label: Option<(String, String, String)>,
    pub created_at: i64,
}

static JOIN_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1000);

type Hook = Box<dyn FnMut(&str, &str, &mut Json) + Send>;

pub struct World {
    pub vault: Option<Vault>,
    pub grants: Vec<Grant>,
    /// 账号的设备数量上限（与 auth.ts 一致：超出时新设备以受限状态登录）
    pub max_devices: u32,
    pub quota_bytes: u64,
    /// 版本检查下发的策略
    pub policy: Json,
    codes: Vec<(String, String, String, String)>,
    /// 改写已登录接口的 JSON 响应：(method, path, 响应)
    pub hook: Option<Hook>,
    counter: u64,
    pub log: Vec<String>,
}

impl Default for World {
    fn default() -> Self {
        World {
            vault: None,
            grants: vec![],
            max_devices: 10,
            quota_bytes: 204_800,
            policy: json!({ "check_interval_secs": 3600, "panel_check_secs": 900, "min_push_interval_secs": 900 }),
            codes: vec![],
            hook: None,
            counter: 0,
            log: vec![],
        }
    }
}

#[derive(Default)]
pub struct FakeServer {
    pub world: Mutex<World>,
}

fn ok(v: Json) -> Response {
    Response { status: 200, body: v.to_string().into_bytes() }
}

fn status(code: u16, v: Json) -> Response {
    Response { status: code, body: v.to_string().into_bytes() }
}

fn err(code: u16, error: &str, message: &str) -> Response {
    status(code, json!({ "error": error, "message": message }))
}

fn query(path: &str) -> (String, BTreeMap<String, String>) {
    let (p, q) = path.split_once('?').unwrap_or((path, ""));
    let params = q
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k.to_string(), percent_decode(v))
        })
        .collect();
    (p.to_string(), params)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn wire(id: &str, r: &Rec) -> Json {
    json!({ "record_id": id, "seq": r.seq, "epoch": r.epoch, "ciphertext": b64u(&r.ct), "deleted": r.deleted })
}

impl World {
    fn token(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    /// 直接为设备签发令牌（跳过浏览器授权）。同一设备重新登录保留原有名额；新设备在名额已满时受限。
    pub fn issue(&mut self, device_id: &str) -> (String, String) {
        let access = self.token("qta_");
        let refresh = self.token("qtr_");
        let previous = self.grants.iter().filter(|g| !g.revoked && g.device_id == device_id).map(|g| g.limited).min();
        self.grants.iter_mut().filter(|g| g.device_id == device_id).for_each(|g| g.revoked = true);
        let slots = self.grants.iter().filter(|g| !g.revoked && !g.limited).count() as u32;
        let limited = previous.unwrap_or(slots >= self.max_devices);
        let id = self.token("g_");
        let created_at = 1_792_000_000 + self.counter as i64;
        self.grants.push(Grant { id, device_id: device_id.into(), access: access.clone(), refresh: refresh.clone(), revoked: false, expired: false, limited, label: None, created_at });
        (access, refresh)
    }

    fn label_of(body: &Json) -> Option<(String, String, String)> {
        let s = |k: &str| body[k].as_str().map(str::to_string);
        Some((s("device_name")?, s("platform")?, s("app_version")?))
    }

    /// 模拟官网 /oauth/authorize：校验参数（与 auth.ts 的 parseDesktopAuthorize 一致），返回 (code, state)。
    pub fn authorize(&mut self, url: &str) -> (String, String) {
        let (path, q) = query(url.split_once("://").map(|(_, r)| &r[r.find('/').unwrap()..]).unwrap());
        assert_eq!(path, "/oauth/authorize");
        assert_eq!(q["client_id"], "qantrip-desktop");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"].len(), 43);
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
        let dev = &q["device_id"];
        assert!(dev.len() == 26 && dev.bytes().all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b)));
        let code = self.token("code_");
        self.codes.push((code.clone(), dev.clone(), q["code_challenge"].clone(), q["redirect_uri"].clone()));
        (code, q["state"].clone())
    }

    fn token_endpoint(&mut self, body: &Json) -> Response {
        let s = |k: &str| body[k].as_str().unwrap_or("").to_string();
        match s("grant_type").as_str() {
            "authorization_code" => {
                let Some(pos) = self.codes.iter().position(|c| c.0 == s("code")) else { return err(400, "invalid_grant", "授权码无效") };
                let (_, dev, challenge, redirect) = self.codes.remove(pos);
                if redirect != s("redirect_uri") || dev != s("device_id") {
                    return err(400, "invalid_grant", "授权码与请求不匹配");
                }
                if b64u(&crypto::sha256(&[s("code_verifier").as_bytes()])) != challenge {
                    return err(400, "invalid_grant", "PKCE 校验失败");
                }
                let (a, r) = self.issue(&dev);
                self.grants.last_mut().unwrap().label = World::label_of(body);
                ok(json!({ "access_token": a, "refresh_token": r, "token_type": "Bearer", "expires_in": 3600, "user": { "login": "tester", "avatar_url": null } }))
            }
            "refresh_token" => {
                let refresh = s("refresh_token");
                let Some(i) = self.grants.iter().position(|g| g.refresh == refresh && !g.revoked) else { return err(400, "invalid_grant", "登录已失效") };
                let a = self.token("qta_");
                let r = self.token("qtr_");
                let g = &mut self.grants[i];
                g.access = a.clone();
                g.refresh = r.clone();
                g.expired = false;
                if let Some(l) = World::label_of(body) {
                    g.label = Some(l);
                }
                ok(json!({ "access_token": a, "refresh_token": r, "token_type": "Bearer", "expires_in": 3600, "user": { "login": "tester", "avatar_url": null } }))
            }
            _ => err(400, "unsupported_grant_type", "不支持"),
        }
    }

    fn handle(&mut self, method: &str, full: &str, bearer: Option<&str>, body: Option<&Json>) -> Response {
        self.log.push(format!("{method} {full}"));
        let (path, q) = query(full);
        let empty = Json::Null;
        let body = body.unwrap_or(&empty);
        match (method, path.as_str()) {
            ("POST", "/api/v1/oauth/token") => return self.token_endpoint(body),
            ("POST", "/api/v1/oauth/revoke") => {
                let t = body["token"].as_str().unwrap_or("");
                self.grants.iter_mut().filter(|g| g.access == t || g.refresh == t).for_each(|g| g.revoked = true);
                return ok(Json::Null);
            }
            _ => {}
        }
        let Some(token) = bearer else { return err(401, "unauthorized", "需要登录") };
        let Some(gi) = self.grants.iter().position(|g| g.access == token && !g.revoked) else { return err(401, "unauthorized", "登录已失效") };
        if self.grants[gi].expired {
            return err(401, "token_expired", "访问令牌已过期");
        }
        // 受限设备访问保险库前重新计数（与 auth.ts 的 enforceDeviceLimit 一致）
        if self.grants[gi].limited && !path.starts_with("/api/v1/account") {
            let others = self.grants.iter().enumerate().filter(|(i, g)| *i != gi && !g.revoked && !g.limited).count() as u32;
            if others < self.max_devices {
                self.grants[gi].limited = false;
            } else {
                return status(403, json!({ "error": "device_limit", "message": "设备数量已达上限", "max_devices": self.max_devices }));
            }
        }
        let resp = if path.starts_with("/api/v1/account") { self.account(method, &path, gi) } else { self.route(method, &path, &q, body, gi) };
        let mut resp = resp;
        if let Some(hook) = self.hook.as_mut()
            && (200..300).contains(&resp.status)
        {
            let mut v: Json = serde_json::from_slice(&resp.body).unwrap_or(Json::Null);
            hook(method, &path, &mut v);
            resp.body = v.to_string().into_bytes();
        }
        resp
    }

    fn usage(&self) -> Option<Json> {
        let v = self.vault.as_ref()?;
        let bytes: usize = v.records.values().map(|r| r.ct.len()).sum();
        Some(json!({ "bytes": bytes, "quota_bytes": self.quota_bytes, "records": v.records.len() }))
    }

    fn account(&mut self, method: &str, path: &str, gi: usize) -> Response {
        let me = self.grants[gi].id.clone();
        match (method, path.trim_start_matches("/api/v1/")) {
            ("GET", "account") => ok(json!({
                "login": "tester",
                "limits": { "quota_bytes": self.quota_bytes, "max_devices": self.max_devices },
                "usage": self.usage(),
            })),
            ("GET", "account/devices") => {
                let mut list: Vec<&Grant> = self.grants.iter().filter(|g| !g.revoked).collect();
                list.sort_by_key(|g| std::cmp::Reverse(g.created_at));
                let devices: Vec<Json> = list
                    .iter()
                    .map(|g| {
                        let (n, p, a) = g.label.clone().map_or((Json::Null, Json::Null, Json::Null), |(n, p, a)| (n.into(), p.into(), a.into()));
                        json!({ "grant_id": g.id, "device_id": g.device_id, "device_name": n, "platform": p, "app_version": a,
                                "created_at": g.created_at, "last_seen_at": g.created_at, "is_self": g.id == me, "limited": g.limited })
                    })
                    .collect();
                ok(json!({ "max_devices": self.max_devices, "devices": devices }))
            }
            ("DELETE", p) if p.starts_with("account/devices/") => {
                let id = &p["account/devices/".len()..];
                match self.grants.iter_mut().find(|g| g.id == id && !g.revoked) {
                    Some(g) => {
                        g.revoked = true;
                        ok(json!({ "is_self": id == me }))
                    }
                    None => err(404, "device_not_found", "设备不存在"),
                }
            }
            _ => err(404, "not_found", "接口不存在"),
        }
    }

    fn route(&mut self, method: &str, path: &str, q: &BTreeMap<String, String>, body: &Json, gi: usize) -> Response {
        let me = self.grants[gi].device_id.clone();
        let usage = self.usage();
        let policy = self.policy.clone();
        let parts: Vec<&str> = path.trim_start_matches("/api/v1/").split('/').collect();
        if parts == ["vault"] {
            if method == "POST" && self.vault.is_none() {
                self.vault = Some(Vault { id: "v_testvault0000000000001".into(), min_epoch: 1, ..Default::default() });
            }
            return match &self.vault {
                Some(v) => status(if method == "POST" { 201 } else { 200 }, json!({ "vault_id": v.id, "seq": v.seq, "roster_len": v.roster.len(), "min_epoch": v.min_epoch })),
                None => ok(json!({ "vault_id": null })),
            };
        }
        if parts.len() < 3 || parts[0] != "vaults" {
            return err(404, "not_found", "接口不存在");
        }
        let Some(v) = self.vault.as_mut().filter(|v| v.id == parts[1]) else { return err(404, "vault_not_found", "保险库不存在") };
        let b64 = |k: &str| from_b64u(body[k].as_str().unwrap_or("")).unwrap();
        match (method, &parts[2..]) {
            ("GET", ["head"]) => {
                let pending = v.joins.iter().filter(|j| j.creator != me).count();
                ok(json!({ "seq": v.seq, "roster_index": v.roster.len(), "min_epoch": v.min_epoch, "pending_joins": pending, "usage": usage, "policy": policy }))
            }
            ("GET", ["changes"]) => {
                let since: u64 = q.get("since").and_then(|s| s.parse().ok()).unwrap_or(0);
                let limit: usize = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(200).clamp(1, 200);
                let mut rows: Vec<(&String, &Rec)> = v.records.iter().filter(|(_, r)| r.seq > since).collect();
                rows.sort_by_key(|(_, r)| r.seq);
                let more = rows.len() > limit;
                let page: Vec<Json> = rows.iter().take(limit).map(|(id, r)| wire(id, r)).collect();
                let next = if more { Some(rows[limit - 1].1.seq) } else { None };
                ok(json!({ "records": page, "next": next, "head": v.seq, "purged_seq": v.purged_seq }))
            }
            ("POST", ["records"]) => {
                let writes = body["writes"].as_array().cloned().unwrap_or_default();
                if writes.is_empty() || writes.len() > 50 {
                    return err(400, "batch_too_large", "每批最多 50 条");
                }
                let results: Vec<Json> = writes.iter().map(|w| write_one(v, w)).collect();
                ok(json!({ "results": results }))
            }
            ("POST", ["min-epoch"]) => {
                v.min_epoch = v.min_epoch.max(body["epoch"].as_u64().unwrap() as u32);
                ok(json!({ "min_epoch": v.min_epoch }))
            }
            ("GET", ["roster"]) => {
                let since: usize = q.get("since").and_then(|s| s.parse().ok()).unwrap_or(0);
                let entries: Vec<Json> = v.roster.iter().enumerate().skip(since).take(1000).map(|(i, e)| json!({ "index": i, "entry": b64u(e) })).collect();
                ok(json!({ "entries": entries, "length": v.roster.len() }))
            }
            ("POST", ["roster"]) => {
                let expected = body["expected_index"].as_u64().unwrap() as usize;
                if expected != v.roster.len() {
                    return err(409, "roster_conflict", "花名册已被其他设备更新");
                }
                v.roster.push(b64("entry"));
                if let Some((id, mut r)) = v.inject_after_roster.take() {
                    v.seq += 1;
                    r.seq = v.seq;
                    v.records.insert(id, r);
                }
                status(201, json!({ "index": expected }))
            }
            ("PUT", ["envelopes", recipient, epoch]) => {
                let idx = body["roster_index"].as_u64().unwrap() as u32;
                v.envelopes.insert((recipient.to_string(), epoch.parse().unwrap()), (idx, b64("data")));
                Response { status: 204, body: vec![] }
            }
            ("GET", ["envelopes", recipient]) => {
                let mut envs: Vec<Json> = v
                    .envelopes
                    .iter()
                    .filter(|((r, _), _)| r == recipient)
                    .map(|((_, e), (idx, d))| json!({ "epoch": e, "roster_index": idx, "data": b64u(d) }))
                    .collect();
                envs.reverse();
                ok(json!({ "envelopes": envs }))
            }
            ("POST", ["joins"]) => {
                if !v.joins.is_empty() {
                    return err(409, "join_in_progress", "已有一个进行中的加入请求");
                }
                let id = format!("j_{}", JOIN_IDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
                v.joins.push(Join { id: id.clone(), creator: me.clone(), commit: b64("data"), ..Default::default() });
                status(201, json!({ "join_id": id, "expires_at": 0 }))
            }
            ("GET", ["joins"]) => ok(json!({ "joins": v.joins.iter().map(join_view).collect::<Vec<_>>() })),
            ("GET", ["joins", id]) => match v.joins.iter().find(|j| j.id == *id) {
                Some(j) => ok(join_view(j)),
                None => err(404, "join_not_found", "加入请求不存在或已过期"),
            },
            ("PUT", ["joins", id, step]) => {
                let data = b64("data");
                let Some(j) = v.joins.iter_mut().find(|j| j.id == *id) else { return err(409, "join_step_conflict", "状态不符") };
                let slot = match *step {
                    "reply" => (&mut j.reply, true),
                    "reveal" => {
                        let prev = j.reply.is_some();
                        (&mut j.reveal, prev)
                    }
                    "result" => {
                        let prev = j.reveal.is_some();
                        (&mut j.result, prev)
                    }
                    _ => return err(404, "not_found", "未知的步骤"),
                };
                if slot.0.is_some() || !slot.1 {
                    return err(409, "join_step_conflict", "状态不符");
                }
                *slot.0 = Some(data);
                Response { status: 204, body: vec![] }
            }
            ("DELETE", ["joins", id]) => {
                v.joins.retain(|j| j.id != *id);
                Response { status: 204, body: vec![] }
            }
            _ => err(404, "not_found", "接口不存在"),
        }
    }
}

fn join_view(j: &Join) -> Json {
    let enc = |b: &Option<Vec<u8>>| b.as_ref().map(|b| b64u(b));
    json!({ "join_id": j.id, "commit": b64u(&j.commit), "reply": enc(&j.reply), "reveal": enc(&j.reveal), "result": enc(&j.result), "expires_at": 0 })
}

/// 与 sync.ts 的 writeOne 一致：幂等、epoch 下限、CAS、冲突返回当前版本、gone。
fn write_one(v: &mut Vault, w: &Json) -> Json {
    let id = w["record_id"].as_str().unwrap().to_string();
    let key = w["idempotency_key"].as_str().unwrap().to_string();
    if let Some(r) = v.idem.get(&key) {
        return r.clone();
    }
    let epoch = w["epoch"].as_u64().unwrap() as u32;
    let ct = from_b64u(w["ciphertext"].as_str().unwrap()).unwrap();
    let deleted = w["deleted"].as_bool().unwrap();
    let base = w["base_seq"].as_u64();
    let remember = |v: &mut Vault, r: Json| {
        v.idem.insert(key.clone(), r.clone());
        r
    };
    if ct.is_empty() || ct.len() > 256 * 1024 {
        return remember(v, json!({ "record_id": id, "status": "invalid", "message": "记录为空或超过 256 KiB" }));
    }
    if epoch < v.min_epoch {
        return remember(v, json!({ "record_id": id, "status": "stale_epoch", "message": "密钥版本已过期" }));
    }
    match (base, v.records.get(&id)) {
        (None, Some(cur)) => json!({ "record_id": id, "status": "conflict", "current": wire(&id, cur) }),
        (Some(_), None) => json!({ "record_id": id, "status": "gone", "message": "记录不存在，请先全量对账" }),
        (Some(b), Some(cur)) if cur.seq != b => json!({ "record_id": id, "status": "conflict", "current": wire(&id, cur) }),
        _ => {
            v.put(&id, epoch, ct, deleted);
            let seq = v.seq;
            remember(v, json!({ "record_id": id, "status": "ok", "seq": seq }))
        }
    }
}

impl Transport for FakeServer {
    fn send(&self, method: &str, path: &str, bearer: Option<&str>, json: Option<&Json>) -> Result<Response, String> {
        Ok(self.world.lock().unwrap().handle(method, path, bearer, json))
    }
}
