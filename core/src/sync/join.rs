//! 登录（PKCE，§9）、创建保险库、新设备加入（§6.2）与恢复短语加入（§6.3）。
//!
//! 加入协议的四段消息（确定性 CBOR，服务端只转发；协议版本 2，v0.5）：
//! - commit（N → E）：`{ v: 2, commit }`，`commit = SHA-256("qantrip/v1/commit" || vault_id || pk_N || n_N)`
//! - reply（E → N）：`{ pk: pk_E, nonce: n_E, roster: [条目…] }`——E 回复时锁定 commit 的完整字节
//! - reveal（N → E）：`{ pk: pk_N, nonce: n_N }`——N 在收到并锁定 pk_E 与花名册头部之后才发送
//! - result（E → N）：`{ body, sig }`，`body = { roster: [头部之后的新条目…], envelope, epoch, roster_index, anchor }`，
//!   `anchor` 为 E 自己的状态记录锚点 `{ seq, counter, hash }` 或 null（E 还没有写过状态记录）；
//!   `sig` 为 E 的花名册密钥对 [`crypto::join_result_message`] 的签名
//!
//! 确认方式（v0.5）：两台设备显示同一个 SAS。E 上同时显示 SAS 与两个随机码，用户选出与 N 上相同的那个
//! （选错即取消本次加入）。E 选对之后才追加 add_device 并发出 result；N 收到通过校验的 result 后采用花名册
//! 与钥匙环，在此之前不向服务端写入任何数据。v0.5.1 起 N 上不再需要用户确认（D40，C2 记为已接受的风险，
//! 见 docs/sync-design.md 附录 C）。
//!
//! 双方的进行中状态保存在平台安全存储中（`join_new_device` / `join_approver`），应用重启后可以继续；
//! 完成、过期、拒绝或失败时删除。
//!
//! `pk` 为三把公钥按 x25519 || roster || auth 拼接的 96 字节。

use std::collections::BTreeMap;

use ciborium::Value;
use zeroize::Zeroizing;

use super::codec::{self, Fields};
use super::crypto::{self, PhraseLanguage, PublicKeys, RecoveryKeys};
use super::engine::{Core, Keyring, open_verified_envelope};
use super::net::{CLIENT_ID, JoinView, WireEnvelope};
use super::roster::{Entry, Op, Roster, Signer, ZERO_HASH, recovery_bind_message};
use super::state::JoinAnchor;
use super::{JoinProgress, JoinRequest, JoinRequestStatus, JoinStage, SecretStore, SyncError, SyncReport};

/// 加入协议版本（v0.5 起为 2：两台设备比对同一个校验码，结果由批准设备签名）
const JOIN_VERSION: u64 = 2;
/// 本地保存的加入状态的有效期（服务端加入请求 10 分钟过期；留出余量）
const JOIN_LOCAL_TTL: i64 = 15 * 60;
/// 每次列出加入请求时最多处理的请求数（服务端同一时间只允许一个，L3）
const MAX_JOINS_PER_CALL: usize = 3;

pub const SECRET_JOIN_N: &str = "join_new_device";
pub const SECRET_JOIN_E: &str = "join_approver";

fn security(message: impl Into<String>) -> SyncError {
    SyncError::Security { message: message.into() }
}

fn invalid(message: impl Into<String>) -> SyncError {
    SyncError::InvalidState { message: message.into() }
}

/// 可选字节串：`null` 或字节串。
fn opt_bytes(v: Value) -> Result<Option<Vec<u8>>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Bytes(b) => Ok(Some(b)),
        _ => Err("应为字节串或 null".into()),
    }
}

fn opt_value(b: &Option<Vec<u8>>) -> Value {
    b.as_ref().map_or(Value::Null, |b| codec::bytes(b))
}

// MARK: - 登录（PKCE S256）

pub struct PendingLogin {
    verifier: Zeroizing<String>,
    state: String,
    redirect_uri: String,
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// PKCE：`code_challenge = base64url(SHA-256(code_verifier))`
pub fn pkce_challenge(verifier: &str) -> String {
    codec::b64u(&crypto::sha256(&[verifier.as_bytes()]))
}

pub fn begin_sign_in(core: &mut Core, redirect_uri: &str) -> Result<String, SyncError> {
    let keys = core.ensure_device_keys()?;
    let verifier = Zeroizing::new(codec::b64u(&crypto::random::<32>()));
    let state = codec::b64u(&crypto::random::<16>());
    let url = format!(
        "{}/oauth/authorize?client_id={CLIENT_ID}&response_type=code&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}&device_id={}",
        core.server.trim_end_matches('/'),
        url_encode(redirect_uri),
        pkce_challenge(&verifier),
        state,
        keys.device_id(),
    );
    core.login = Some(PendingLogin { verifier, state, redirect_uri: redirect_uri.to_string() });
    Ok(url)
}

pub fn finish_sign_in(core: &mut Core, code: &str, state: &str) -> Result<String, SyncError> {
    let login = core.login.take().ok_or_else(|| invalid("没有进行中的登录，请重新登录"))?;
    if !crypto::ct_eq(login.state.as_bytes(), state.as_bytes()) {
        return Err(security("登录回调的 state 不符，已拒绝"));
    }
    let me = core.me()?;
    let tokens = core.api.exchange_code(code, &login.redirect_uri, &me, &login.verifier)?;
    core.api.store_tokens(&tokens)?;
    core.state.account = Some(tokens.user.login.clone());
    core.save()?;
    Ok(tokens.user.login)
}

// MARK: - 创建保险库

pub fn create_vault(core: &mut Core, lang: PhraseLanguage) -> Result<Zeroizing<String>, SyncError> {
    let keys = core.ensure_device_keys()?;
    let me = keys.device_id();
    if core.enrolled() {
        return Err(invalid("本机已加入保险库"));
    }
    let vault = match core.api.vault()? {
        Some(v) => {
            if core.api.head(&v)?.roster_index > 0 {
                return Err(invalid("此账号已有保险库：请在已登录的设备上批准本机加入，或使用恢复短语"));
            }
            v
        }
        None => core.api.create_vault()?,
    };
    let vk: Zeroizing<[u8; 32]> = Zeroizing::new(crypto::random());
    let entropy: Zeroizing<[u8; 32]> = Zeroizing::new(crypto::random());
    let rk = RecoveryKeys::from_entropy(&entropy);
    let genesis = Entry::signed(
        &vault,
        0,
        ZERO_HASH,
        Op::Genesis { device: keys.public(), vk_commit: crypto::vk_commit(&vk, &vault, 1) },
        Signer::Device(me.clone()),
        |m| keys.roster_sign(m),
    );
    let bind = rk.sign(&recovery_bind_message(&vault, &genesis.hash()));
    let set_recovery = Entry::signed(
        &vault,
        1,
        genesis.hash(),
        Op::SetRecovery { x25519: rk.x_public(), ed25519: rk.s_public(), bind },
        Signer::Device(me.clone()),
        |m| keys.roster_sign(m),
    );
    let raws = vec![genesis.encode(), set_recovery.encode()];
    let roster = Roster::from_entries(&vault, &raws).map_err(|_| security("本机构造的花名册无效"))?;
    if !core.api.append_roster(&vault, 0, &raws[0])? {
        return Err(SyncError::Conflict { message: "其他设备正在创建保险库，请稍后加入".into() });
    }
    if !core.api.append_roster(&vault, 1, &raws[1])? {
        return Err(SyncError::Conflict { message: "保险库创建过程被打断，请重试".into() });
    }
    let mut ring = Keyring::default();
    ring.insert(1, *vk);
    core.adopt_vault(&vault, roster, &ring)?;
    core.write_envelope(&ring, &me, &keys.public().x25519)?;
    core.write_envelope(&ring, crypto::RECOVERY_RECIPIENT, &rk.x_public())?;
    // 立即写入本机（创建者）的状态记录：其他设备据此要求它必须存在（§4.4，v0.3）。失败时由首次同步补写。
    let _ = core.write_own_state(&mut SyncReport::default());
    let _ = core.save();
    Ok(crypto::phrase_from_entropy(&entropy, lang))
}

// MARK: - 新设备（N）

/// 新设备的加入状态。只保存原始消息、n_N 与用户的确认，其余（锁定的花名册、SAS、钥匙环）每次由消息重新推出并校验。
#[derive(Clone)]
pub struct JoinN {
    join_id: String,
    vault: String,
    nonce: Zeroizing<[u8; 32]>,
    started: i64,
    /// 本机发出的 commit 消息
    commit: Vec<u8>,
    /// 收到的 reply 与本机据此发出的 reveal（同时出现）
    reply: Option<Vec<u8>>,
    reveal: Option<Vec<u8>>,
    /// 收到并通过校验的 result
    result: Option<Vec<u8>>,
    /// 已加入（只在内存中）
    done: bool,
}

impl JoinN {
    fn encode(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(codec::encode(&codec::map(vec![
            ("join_id", codec::text(&self.join_id)),
            ("vault", codec::text(&self.vault)),
            ("nonce", codec::bytes(&self.nonce[..])),
            ("started", codec::uint(self.started.max(0) as u64)),
            ("commit", codec::bytes(&self.commit)),
            ("reply", opt_value(&self.reply)),
            ("reveal", opt_value(&self.reveal)),
            ("result", opt_value(&self.result)),
        ])))
    }

    fn decode(b: &[u8]) -> Result<JoinN, String> {
        const KEYS: [&str; 8] = ["join_id", "vault", "nonce", "started", "commit", "reply", "reveal", "result"];
        // 0.5.0 还保存了用户是否已确认（"confirmed"，0.5.1 起不再需要）：升级时进行中的加入照常继续
        const KEYS_050: [&str; 9] = ["join_id", "vault", "nonce", "started", "commit", "reply", "reveal", "result", "confirmed"];
        let v = codec::decode(b)?;
        let mut f = Fields::exact(v.clone(), &KEYS).or_else(|_| Fields::exact(v, &KEYS_050))?;
        Ok(JoinN {
            join_id: f.text("join_id")?,
            vault: f.text("vault")?,
            nonce: Zeroizing::new(f.array("nonce")?),
            started: f.u64("started")? as i64,
            commit: f.bytes("commit")?,
            reply: opt_bytes(f.value("reply"))?,
            reveal: opt_bytes(f.value("reveal"))?,
            result: opt_bytes(f.value("result"))?,
            done: false,
        })
    }
}

pub fn load_join_n(secrets: &dyn SecretStore) -> Option<JoinN> {
    let b = Zeroizing::new(secrets.get(SECRET_JOIN_N.into()).ok()??);
    JoinN::decode(&b).ok()
}

/// 把 `core.join_n` 写入（或从）安全存储（删除）。
fn persist_n(core: &Core) -> Result<(), SyncError> {
    match &core.join_n {
        Some(j) if !j.done => core.secrets.set(SECRET_JOIN_N.into(), j.encode().to_vec())?,
        _ => core.secrets.delete(SECRET_JOIN_N.into())?,
    }
    Ok(())
}

fn set_join_n(core: &mut Core, j: Option<JoinN>) -> Result<(), SyncError> {
    core.join_n = j;
    persist_n(core)
}

fn progress(join_id: &str, stage: JoinStage, sas: Option<String>) -> JoinProgress {
    JoinProgress { join_id: join_id.to_string(), stage, sas }
}

pub fn start_join(core: &mut Core) -> Result<JoinProgress, SyncError> {
    let keys = core.ensure_device_keys()?;
    if core.enrolled() {
        return Err(invalid("本机已加入保险库"));
    }
    cancel_join(core);
    let vault = core.api.vault()?.ok_or_else(|| SyncError::NoVault { message: "此账号还没有保险库，请先在一台设备上创建".into() })?;
    let nonce = Zeroizing::new(crypto::random::<32>());
    let commit = crypto::join_commit(&vault, &keys.public().concat(), &nonce);
    let msg = codec::encode(&codec::map(vec![("v", codec::uint(JOIN_VERSION)), ("commit", codec::bytes(&commit))]));
    let join_id = core.api.create_join(&vault, &msg)?;
    let j = JoinN {
        join_id: join_id.clone(),
        vault,
        nonce,
        started: core.now(),
        commit: msg,
        reply: None,
        reveal: None,
        result: None,
        done: false,
    };
    set_join_n(core, Some(j))?;
    Ok(progress(&join_id, JoinStage::WaitingForApprover, None))
}

pub fn cancel_join(core: &mut Core) {
    if let Some(j) = core.join_n.take()
        && !j.done
    {
        let _ = core.api.delete_join(&j.vault, &j.join_id);
    }
    let _ = persist_n(core);
}

/// 失败时清理加入请求并报错。告警与错误只使用固定文本（不回显解析细节，L4）。
fn fail_join(core: &mut Core, message: &str) -> SyncError {
    cancel_join(core);
    core.alert("join_failed", format!("加入失败：{message}"));
    let _ = core.save();
    security(message)
}

/// 本机曾被移出保险库：同一套设备密钥不能再次加入。
const MSG_REMOVED: &str = "本机曾被移出保险库，请在 设置 › 同步 中重置本机后重新加入";

/// 解析并校验 reply：花名册链、pk_E 在册、本机没有被移出过。返回 (锁定的花名册, pk_E, n_E)。
///
/// 本机已在册（上一次加入时批准设备已追加了本机，但本机没有完成确认）时允许继续：批准设备不再追加，只重新发送钥匙环。
fn parse_reply(vault: &str, reply: &[u8], me: &str, pk_n: &PublicKeys) -> Result<(Roster, PublicKeys, [u8; 32]), String> {
    let mut f = Fields::exact(codec::decode(reply)?, &["pk", "nonce", "roster"])?;
    let pk_e = PublicKeys::from_concat(&f.bytes("pk")?)?;
    let n_e: [u8; 32] = f.array("nonce")?;
    let Value::Array(items) = f.value("roster") else { return Err("花名册格式不正确".into()) };
    if items.len() > super::net::ROSTER_MAX_ENTRIES as usize {
        return Err("花名册过长".into());
    }
    let raws: Vec<Vec<u8>> = items.into_iter().map(codec::as_bytes).collect::<Result<_, _>>()?;
    let roster = Roster::from_entries(vault, &raws)?;
    // pk_E 必须是当前在册设备的公钥
    let e_id = pk_e.device_id();
    if roster.member(&e_id).is_none_or(|m| m.removed.is_some() || m.keys != pk_e) {
        return Err("批准设备不在册".into());
    }
    if let Some(m) = roster.member(me)
        && (m.removed.is_some() || m.keys != *pk_n)
    {
        return Err(MSG_REMOVED.into());
    }
    Ok((roster, pk_e, n_e))
}

/// 校验 result：批准设备对本次加入的签名、新花名册从锁定的头部延伸且本机在册（由 E 追加，或锁定时已在册）、
/// 钥匙环来自 E 且通过承诺校验。返回 (新花名册, 钥匙环, E 的状态记录锚点)。
fn verify_result(core: &Core, roster: &Roster, pk_e: &PublicKeys, transcript: [&[u8]; 3], result: &[u8]) -> Result<(Roster, Keyring, Option<JoinAnchor>), String> {
    let keys = core.keys().map_err(|_| "本机缺少设备密钥".to_string())?;
    let me = keys.device_id();
    let pk_n = keys.public();
    let e_id = pk_e.device_id();
    let mut outer = Fields::exact(codec::decode(result)?, &["body", "sig"])?;
    let body = outer.bytes("body")?;
    let sig: [u8; 64] = outer.array("sig")?;
    crypto::verify(&pk_e.roster, &crypto::join_result_message(roster.vault_id(), transcript, &body), &sig)?;
    let mut f = Fields::exact(codec::decode(&body)?, &["roster", "envelope", "epoch", "roster_index", "anchor"])?;
    let Value::Array(items) = f.value("roster") else { return Err("花名册格式不正确".into()) };
    if items.len() > super::net::ROSTER_MAX_ENTRIES as usize {
        return Err("花名册过长".into());
    }
    let raws: Vec<Vec<u8>> = items.into_iter().map(codec::as_bytes).collect::<Result<_, _>>()?;
    let start = roster.len() as usize;
    let next = roster.extended(&raws)?;
    let added_by_e = next.entries()[start..].iter().any(|e| e.signer == Signer::Device(e_id.clone()) && e.op == Op::AddDevice { device: pk_n.clone() });
    if !(added_by_e || roster.is_enrolled(&me)) || !next.is_enrolled(&me) {
        return Err("花名册中没有本机的加入条目".into());
    }
    let envelope = f.bytes("envelope")?;
    let env = WireEnvelope { epoch: f.u32("epoch")?, roster_index: f.u32("roster_index")?, data: codec::b64u(&envelope) };
    let sender = Fields::exact(codec::decode(&envelope)?, &["sender", "enc", "ct"])?.text("sender")?;
    if sender != e_id || next.member(&e_id).map(|m| &m.keys) != Some(pk_e) {
        return Err("钥匙环不是由批准设备发送的".into());
    }
    let ring = open_verified_envelope(&next, &env, &me, keys.enc_secret())?;
    if ring.get(next.max_epoch()).is_none() {
        return Err("钥匙环缺少最新的密钥版本".into());
    }
    let anchor = match f.value("anchor") {
        Value::Null => None,
        v => {
            let mut a = Fields::exact(v, &["seq", "counter", "hash"])?;
            let hash: [u8; 32] = a.array("hash")?;
            Some(JoinAnchor { device: e_id, seq: a.u64("seq")?, counter: a.u64("counter")?, hash: codec::b64u(&hash) })
        }
    };
    Ok((next, ring, anchor))
}

/// 由保存的 reply 重新推出锁定的花名册、pk_E 与 SAS。
fn locked(j: &JoinN, reply: &[u8], me: &str, pk_n: &PublicKeys) -> Result<(Roster, PublicKeys, String), String> {
    let (roster, pk_e, n_e) = parse_reply(&j.vault, reply, me, pk_n)?;
    let (_, head) = roster.head().ok_or("花名册为空")?;
    let sas = crypto::sas(&j.vault, &head, &pk_n.concat(), &pk_e.concat(), &j.nonce, &n_e);
    Ok((roster, pk_e, sas))
}

/// reply 校验失败时的报错：本机曾被移除时给出明确的处理方法，其余一律使用固定文本。
fn fail_reply(core: &mut Core, e: &str) -> SyncError {
    fail_join(core, if e == MSG_REMOVED { MSG_REMOVED } else { "批准设备的回复未通过校验" })
}

/// 新设备：轮询加入进度。收到 reply 后发出 reveal 并显示 SAS；收到通过校验的 result 时完成加入。
pub fn poll_join(core: &mut Core) -> Result<JoinProgress, SyncError> {
    let keys = core.keys()?;
    let me = keys.device_id();
    let pk_n = keys.public();
    let Some(mut j) = core.join_n.clone() else { return Err(invalid("没有进行中的加入请求")) };
    let join_id = j.join_id.clone();
    if j.done {
        return Ok(progress(&join_id, JoinStage::Completed, None));
    }
    if core.now() - j.started > JOIN_LOCAL_TTL {
        cancel_join(core);
        return Ok(progress(&join_id, JoinStage::Expired, None));
    }
    let vault = j.vault.clone();
    let Some(reply) = j.reply.clone() else {
        let Some(view) = core.api.join(&vault, &join_id)? else {
            set_join_n(core, None)?;
            return Ok(progress(&join_id, JoinStage::Expired, None));
        };
        let Some(reply_b64) = view.reply else { return Ok(progress(&join_id, JoinStage::WaitingForApprover, None)) };
        let Ok(reply) = codec::from_b64u(&reply_b64) else { return Err(fail_join(core, "批准设备的回复格式不正确")) };
        let sas = match locked(&j, &reply, &me, &pk_n) {
            Ok((_, _, sas)) => sas,
            Err(e) => return Err(fail_reply(core, &e)),
        };
        // 锁定 pk_E 与花名册头部之后才公开 n_N（先保存，崩溃重启后可以重发同一个 reveal）
        let reveal = codec::encode(&codec::map(vec![("pk", codec::bytes(&pk_n.concat())), ("nonce", codec::bytes(&j.nonce[..]))]));
        j.reply = Some(reply);
        j.reveal = Some(reveal.clone());
        set_join_n(core, Some(j.clone()))?;
        if !core.api.put_join_step(&vault, &join_id, "reveal", &reveal)? {
            return Err(fail_join(core, "加入流程的状态异常"));
        }
        return Ok(progress(&join_id, JoinStage::ShowCode, Some(sas)));
    };
    let (roster, pk_e, sas) = match locked(&j, &reply, &me, &pk_n) {
        Ok(v) => v,
        Err(e) => return Err(fail_reply(core, &e)),
    };
    if j.result.is_none() {
        let Some(view) = core.api.join(&vault, &join_id)? else {
            set_join_n(core, None)?;
            return Ok(progress(&join_id, JoinStage::Expired, None));
        };
        let reveal = j.reveal.clone().unwrap_or_default();
        // 服务端上的 reveal 必须是本机发出的那一份；缺失（例如发送前应用退出）时重发
        match view.reveal.as_deref().map(codec::from_b64u) {
            None => {
                core.api.put_join_step(&vault, &join_id, "reveal", &reveal)?;
            }
            Some(Ok(r)) if r == reveal => {}
            Some(_) => return Err(fail_join(core, "加入流程的状态异常")),
        }
        if let Some(result_b64) = view.result {
            let Ok(result) = codec::from_b64u(&result_b64) else { return Err(fail_join(core, "批准设备的结果格式不正确")) };
            if verify_result(core, &roster, &pk_e, [&j.commit, &reply, &reveal], &result).is_err() {
                return Err(fail_join(core, "批准设备的结果未通过校验"));
            }
            j.result = Some(result);
            set_join_n(core, Some(j.clone()))?;
        }
    }
    // 已登录设备上选对了码：result 通过校验即完成加入（v0.5.1 起新设备上不再需要确认，见 D40）
    if j.result.is_some() {
        finish_join(core, j)?;
        return Ok(progress(&join_id, JoinStage::Completed, None));
    }
    Ok(progress(&join_id, JoinStage::ShowCode, Some(sas)))
}

/// 采用 result 中的花名册与钥匙环。调用前 result 已通过校验；这里重新校验一遍。
fn finish_join(core: &mut Core, j: JoinN) -> Result<(), SyncError> {
    let keys = core.keys()?;
    let me = keys.device_id();
    let (Some(reply), Some(reveal), Some(result)) = (&j.reply, &j.reveal, &j.result) else {
        return Err(invalid("尚未收到已登录设备的批准"));
    };
    let vault = j.vault.clone();
    let verified = parse_reply(&vault, reply, &me, &keys.public()).and_then(|(roster, pk_e, _)| verify_result(core, &roster, &pk_e, [&j.commit, reply, reveal], result));
    let Ok((next, ring, anchor)) = verified else { return Err(fail_join(core, "批准设备的结果未通过校验")) };
    // 服务端当前的花名册必须以本机将要采用的花名册（含 reply 中锁定的部分）为前缀
    let (raws, _) = core.api.roster(&vault, 0)?;
    if raws.len() < next.raw().len() || raws[..next.raw().len()] != *next.raw() {
        return Err(fail_join(core, "服务端的花名册与批准设备发来的不一致"));
    }
    core.adopt_vault(&vault, next, &ring)?;
    core.state.join_anchor = anchor;
    core.save()?;
    let _ = core.api.delete_join(&vault, &j.join_id);
    set_join_n(core, Some(JoinN { done: true, ..j }))?;
    Ok(())
}

// MARK: - 已授权设备（E）

/// 批准设备的加入状态（每个加入请求一份）。
#[derive(Clone)]
pub struct JoinE {
    nonce: Zeroizing<[u8; 32]>,
    /// 回复时发送的花名册长度与头部哈希（SAS 绑定的头部）
    roster_len: u32,
    head: [u8; 32],
    /// 回复时锁定的 commit 消息完整字节（C1：批准时逐字节比对，服务端不能在看到 n_E 之后替换承诺）
    commit: Vec<u8>,
    /// 本机发出的 reply 消息
    reply: Vec<u8>,
    at: i64,
    /// 与 SAS 一起显示的两个随机码，以及 SAS 在三个选项中的位置（回复时生成，每次显示都相同）
    decoys: Vec<String>,
    slot: u32,
}

impl JoinE {
    fn value(&self) -> Value {
        codec::map(vec![
            ("nonce", codec::bytes(&self.nonce[..])),
            ("roster_len", codec::uint(self.roster_len as u64)),
            ("head", codec::bytes(&self.head)),
            ("commit", codec::bytes(&self.commit)),
            ("reply", codec::bytes(&self.reply)),
            ("at", codec::uint(self.at.max(0) as u64)),
            ("decoys", Value::Array(self.decoys.iter().map(|d| codec::text(d)).collect())),
            ("slot", codec::uint(self.slot as u64)),
        ])
    }

    fn parse(v: Value) -> Result<JoinE, String> {
        let mut f = Fields::exact(v, &["nonce", "roster_len", "head", "commit", "reply", "at", "decoys", "slot"])?;
        let Value::Array(items) = f.value("decoys") else { return Err("格式不正确".into()) };
        let decoys: Vec<String> = items.into_iter().map(|d| d.into_text().map_err(|_| "格式不正确".to_string())).collect::<Result<_, _>>()?;
        if decoys.len() != 2 {
            return Err("格式不正确".into());
        }
        Ok(JoinE {
            nonce: Zeroizing::new(f.array("nonce")?),
            roster_len: f.u32("roster_len")?,
            head: f.array("head")?,
            commit: f.bytes("commit")?,
            reply: f.bytes("reply")?,
            at: f.u64("at")? as i64,
            decoys,
            slot: f.u32("slot")? % 3,
        })
    }

    /// 三个选项：SAS 放在回复时随机选定的位置。
    fn choices(&self, sas: String) -> Vec<String> {
        let mut v = self.decoys.clone();
        v.insert(self.slot as usize, sas);
        v
    }
}

pub fn load_join_e(secrets: &dyn SecretStore) -> BTreeMap<String, JoinE> {
    let parse = |b: &[u8]| -> Result<BTreeMap<String, JoinE>, String> {
        let Value::Map(items) = codec::decode(b)? else { return Err("格式不正确".into()) };
        items
            .into_iter()
            .map(|(k, v)| match k {
                Value::Text(id) => Ok((id, JoinE::parse(v)?)),
                _ => Err("格式不正确".into()),
            })
            .collect()
    };
    secrets.get(SECRET_JOIN_E.into()).ok().flatten().and_then(|b| parse(&Zeroizing::new(b)).ok()).unwrap_or_default()
}

fn persist_e(core: &Core) -> Result<(), SyncError> {
    if core.join_e.is_empty() {
        core.secrets.delete(SECRET_JOIN_E.into())?;
    } else {
        let v = Value::Map(core.join_e.iter().map(|(id, e)| (codec::text(id), e.value())).collect());
        core.secrets.set(SECRET_JOIN_E.into(), Zeroizing::new(codec::encode(&v)).to_vec())?;
    }
    Ok(())
}

fn drop_join_e(core: &mut Core, join_id: &str) -> Result<(), SyncError> {
    if core.join_e.remove(join_id).is_some() {
        persist_e(core)?;
    }
    Ok(())
}

/// 结束一个加入请求：删除本机状态与服务端上的请求。
fn abort_join_e(core: &mut Core, vault: &str, join_id: &str) -> Result<(), SyncError> {
    drop_join_e(core, join_id)?;
    let _ = core.api.delete_join(vault, join_id);
    Ok(())
}

/// commit 消息：`{ v, commit }`。返回协议版本与承诺值；版本不是 [`JOIN_VERSION`] 时由调用方处理。
fn parse_commit(msg: &[u8]) -> Result<(u64, [u8; 32]), String> {
    let mut c = Fields::exact(codec::decode(msg)?, &["v", "commit"])?;
    Ok((c.u64("v")?, c.array("commit")?))
}

/// 通过校验的新设备公开值：(pk_N, n_N, reveal 消息的完整字节)
type Revealed = (PublicKeys, [u8; 32], Vec<u8>);

/// 校验新设备的 reveal：服务端当前的 commit 与回复时锁定的逐字节相同，reveal 与承诺相符。
/// 返回 None 表示新设备还没有发出 reveal。
fn checked_reveal(vault: &str, e: &JoinE, view: &JoinView) -> Result<Option<Revealed>, &'static str> {
    let Some(reveal) = &view.reveal else { return Ok(None) };
    // C1：服务端当前的 commit 必须与回复时锁定的逐字节相同；校验只使用锁定的那一份
    let current = view.commit.as_deref().and_then(|c| codec::from_b64u(c).ok());
    if current.as_deref() != Some(&e.commit[..]) {
        return Err("新设备的承诺在本机回复之后被替换");
    }
    let (_, c_value) = parse_commit(&e.commit).map_err(|_| "新设备的承诺格式不正确")?;
    let reveal = codec::from_b64u(reveal).map_err(|_| "新设备的公开值格式不正确")?;
    let mut r = codec::decode(&reveal).and_then(|v| Fields::exact(v, &["pk", "nonce"])).map_err(|_| "新设备的公开值格式不正确")?;
    let pk_n = r.bytes("pk").and_then(|b| PublicKeys::from_concat(&b)).map_err(|_| "新设备的公开值格式不正确")?;
    let n_n: [u8; 32] = r.array("nonce").map_err(|_| "新设备的公开值格式不正确")?;
    if !crypto::ct_eq(&crypto::join_commit(vault, &pk_n.concat(), &n_n), &c_value) {
        return Err("新设备的承诺校验失败");
    }
    Ok(Some((pk_n, n_n, reveal)))
}

/// reveal 未通过校验：取消请求并告警。
fn reject_tampered(core: &mut Core, vault: &str, join_id: &str, msg: &str) -> SyncError {
    let _ = abort_join_e(core, vault, join_id);
    core.alert("join_failed", format!("加入请求被拒绝：{msg}"));
    let _ = core.save();
    security(msg)
}

/// 列出进行中的加入请求。对尚未回复的请求，本机发送回复（pk_E、完整花名册、n_E；都不是秘密），
/// 同时锁定该请求的 commit。新设备发出 reveal 之后给出三个候选校验码。每次最多处理 3 个请求。
pub fn list_join_requests(core: &mut Core) -> Result<Vec<JoinRequest>, SyncError> {
    core.require_enrolled()?;
    core.refresh_roster()?;
    let vault = core.vault()?;
    let keys = core.keys()?;
    let now = core.now();
    let mut views = core.api.joins(&vault)?;
    views.retain(|v| super::net::valid_server_id(&v.join_id));
    views.truncate(MAX_JOINS_PER_CALL);
    let mut out = Vec::new();
    for v in &views {
        if v.reply.is_none() && !core.join_e.contains_key(&v.join_id) {
            let commit = v.commit.as_deref().and_then(|c| codec::from_b64u(c).ok());
            let Some((version, commit)) = commit.and_then(|c| parse_commit(&c).ok().map(|(version, _)| (version, c))) else { continue };
            if version != JOIN_VERSION {
                // 对方的 Qantrip 版本不同：不回复，提示用户升级
                out.push(JoinRequest { join_id: v.join_id.clone(), expires_at: v.expires_at, status: JoinRequestStatus::Incompatible, choices: vec![] });
                continue;
            }
            let nonce = Zeroizing::new(crypto::random::<32>());
            let roster = core.roster()?;
            let (_, head) = roster.head().unwrap();
            let msg = codec::encode(&codec::map(vec![
                ("pk", codec::bytes(&keys.public().concat())),
                ("nonce", codec::bytes(&nonce[..])),
                ("roster", Value::Array(roster.raw().iter().map(|r| codec::bytes(r)).collect())),
            ]));
            let len = roster.len();
            let e = JoinE {
                nonce,
                roster_len: len,
                head,
                commit,
                reply: msg.clone(),
                at: now,
                decoys: vec![crypto::random_code(), crypto::random_code()],
                slot: u32::from(crypto::random::<1>()[0]) % 3,
            };
            // 先保存再发送：回复发出后应用重启也能继续批准
            core.join_e.insert(v.join_id.clone(), e);
            persist_e(core)?;
            if !core.api.put_join_step(&vault, &v.join_id, "reply", &msg)? {
                drop_join_e(core, &v.join_id)?;
            }
        }
        let (status, choices) = match core.join_e.get(&v.join_id).cloned() {
            None => (JoinRequestStatus::HandledElsewhere, vec![]),
            Some(e) => match checked_reveal(&vault, &e, v) {
                Ok(None) => (JoinRequestStatus::WaitingForNewDevice, vec![]),
                Ok(Some((pk_n, n_n, _))) => {
                    let sas = crypto::sas(&vault, &e.head, &pk_n.concat(), &keys.public().concat(), &n_n, &e.nonce);
                    (JoinRequestStatus::ReadyForCode, e.choices(sas))
                }
                Err(msg) => {
                    reject_tampered(core, &vault, &v.join_id, msg);
                    continue;
                }
            },
        };
        out.push(JoinRequest { join_id: v.join_id.clone(), expires_at: v.expires_at, status, choices });
    }
    let before = core.join_e.len();
    core.join_e.retain(|id, e| views.iter().any(|v| v.join_id == *id) && now - e.at <= JOIN_LOCAL_TTL);
    if core.join_e.len() != before {
        persist_e(core)?;
    }
    Ok(out)
}

pub fn reject_join(core: &mut Core, join_id: &str) -> Result<(), SyncError> {
    let vault = core.vault()?;
    drop_join_e(core, join_id)?;
    core.api.delete_join(&vault, join_id)
}

/// 用户在本机三个候选码中选出新设备上显示的那个。选错即取消本次加入（新设备需要重新发起）。
/// 选对后追加 add_device（新设备已在册时不再追加）、发送钥匙环与签名的结果。
pub fn approve_join(core: &mut Core, join_id: &str, chosen: &str) -> Result<(), SyncError> {
    let me = core.require_enrolled()?;
    let vault = core.vault()?;
    let keys = core.keys()?;
    let Some(e) = core.join_e.get(join_id).cloned() else { return Err(invalid("该加入请求不是由本机处理的，请在列表中重新打开")) };
    if core.now() - e.at > JOIN_LOCAL_TTL {
        drop_join_e(core, join_id)?;
        return Err(invalid("加入请求已过期"));
    }
    let Some(view) = core.api.join(&vault, join_id)? else {
        drop_join_e(core, join_id)?;
        return Err(invalid("加入请求已过期"));
    };
    let (pk_n, n_n, reveal) = match checked_reveal(&vault, &e, &view) {
        Ok(Some(v)) => v,
        Ok(None) => return Err(invalid("新设备尚未显示校验码")),
        Err(msg) => return Err(reject_tampered(core, &vault, join_id, msg)),
    };
    let expected = crypto::sas(&vault, &e.head, &pk_n.concat(), &keys.public().concat(), &n_n, &e.nonce);
    if !crypto::ct_eq(crypto::normalize_sas(chosen).as_bytes(), crypto::normalize_sas(&expected).as_bytes()) {
        abort_join_e(core, &vault, join_id)?;
        core.save()?;
        return Err(SyncError::CodeMismatch { message: "选择的校验码与新设备上显示的不一致，本次加入已取消，请在新设备上重新开始".into() });
    }

    // CAS 追加 add_device(N)，然后发送 HPKE Auth 钥匙环
    let n_id = pk_n.device_id();
    core.refresh_roster()?;
    match core.roster()?.member(&n_id) {
        Some(m) if m.removed.is_some() || m.keys != pk_n => {
            abort_join_e(core, &vault, join_id)?;
            return Err(invalid("这台设备曾被移出保险库，请在它上面重置同步身份后重新加入"));
        }
        // 上一次加入已追加、但新设备没有完成确认：不再追加，重新发送钥匙环
        Some(_) => {}
        None => {
            let added = core.append_roster(|r| {
                if r.member(&n_id).is_some() {
                    return Ok(None);
                }
                let (idx, prev) = r.head().unwrap();
                let entry = Entry::signed(&vault, idx + 1, prev, Op::AddDevice { device: pk_n.clone() }, Signer::Device(me.clone()), |m| keys.roster_sign(m));
                Ok(Some(entry.encode()))
            })?;
            if !added && !core.roster()?.is_enrolled(&n_id) {
                return Err(invalid("无法加入该设备"));
            }
        }
    }
    let ring = core.ensure_keys()?;
    let (epoch, roster_index, envelope) = core.write_envelope(&ring, &n_id, &pk_n.x25519)?;
    let new_entries: Vec<Value> = core.roster()?.raw()[e.roster_len as usize..].iter().map(|r| codec::bytes(r)).collect();
    // 本机状态记录的锚点（M1）：新设备首次同步必须看到不早于它的版本
    let state_rid = crypto::state_record_id(&vault, &me);
    let anchor = core.state.records.get(&state_rid).and_then(|m| {
        let hash = codec::from_b64u(&m.hash).ok()?;
        Some(codec::map(vec![("seq", codec::uint(m.seq?)), ("counter", codec::uint(m.counter)), ("hash", codec::bytes(&hash))]))
    });
    let body = codec::encode(&codec::map(vec![
        ("roster", Value::Array(new_entries)),
        ("envelope", codec::bytes(&envelope)),
        ("epoch", codec::uint(epoch as u64)),
        ("roster_index", codec::uint(roster_index as u64)),
        ("anchor", anchor.unwrap_or(Value::Null)),
    ]));
    let sig = keys.roster_sign(&crypto::join_result_message(&vault, [&e.commit, &e.reply, &reveal], &body));
    let result = codec::encode(&codec::map(vec![("body", codec::bytes(&body)), ("sig", codec::bytes(&sig))]));
    if !core.api.put_join_step(&vault, join_id, "result", &result)? {
        return Err(invalid("加入流程的状态异常，请让新设备重新发起"));
    }
    drop_join_e(core, join_id)?;
    Ok(())
}

// MARK: - 恢复短语加入（§6.3）

pub fn recover(core: &mut Core, phrase: &str) -> Result<(), SyncError> {
    let rk = RecoveryKeys::from_phrase(phrase).map_err(|message| SyncError::InvalidInput { message })?;
    let keys = core.ensure_device_keys()?;
    let me = keys.device_id();
    let vault = core.api.vault()?.ok_or_else(|| SyncError::NoVault { message: "此账号还没有保险库".into() })?;
    let (raws, _) = core.api.roster(&vault, 0)?;
    let roster = Roster::from_entries(&vault, &raws).map_err(|_| security("花名册校验失败"))?;
    let rec = roster.recovery().ok_or_else(|| security("花名册中没有恢复密钥"))?;
    if rec.x25519 != rk.x_public() || rec.ed25519 != rk.s_public() {
        return Err(SyncError::InvalidInput { message: "恢复短语与此保险库不匹配".into() });
    }
    if roster.member(&me).is_some_and(|m| m.removed.is_some()) {
        return Err(invalid("本机曾被移出保险库，请先重置本机的同步身份"));
    }
    let mut envs = core.api.envelopes(&vault, crypto::RECOVERY_RECIPIENT)?;
    envs.sort_by_key(|e| std::cmp::Reverse(e.epoch));
    let env = envs.first().ok_or_else(|| security("服务端没有恢复信封"))?;
    // 信封明文中的花名册头部是下限：花名册早于它会在校验中失败。
    // 已知局限（设计附录 B M3）：服务端可以同时给出一个较旧但真实的花名册前缀和当时的旧恢复信封。
    let ring = open_verified_envelope(&roster, env, crypto::RECOVERY_RECIPIENT, rk.x_secret()).map_err(|_| security("恢复信封校验失败"))?;
    if ring.get(roster.max_epoch()).is_none() {
        return Err(security("恢复信封不是最新的密钥版本"));
    }
    core.adopt_vault(&vault, roster, &ring)?;
    if !core.roster()?.is_enrolled(&me) {
        let pk = keys.public();
        core.append_roster(|r| {
            let (idx, prev) = r.head().unwrap();
            Ok(Some(Entry::signed(&vault, idx + 1, prev, Op::AddDevice { device: pk.clone() }, Signer::Recovery, |m| rk.sign(m)).encode()))
        })?;
    }
    core.write_envelope(&ring, &me, &keys.public().x25519)?;
    Ok(())
}
