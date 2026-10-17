//! 登录（PKCE，§9）、创建保险库、新设备加入（§6.2）与恢复短语加入（§6.3）。
//!
//! 加入协议的四段消息（确定性 CBOR，服务端只转发）：
//! - commit（N → E）：`{ v: 1, commit }`，`commit = SHA-256("qantrip/v1/commit" || vault_id || pk_N || n_N)`
//! - reply（E → N）：`{ pk: pk_E, nonce: n_E, roster: [条目…] }`
//! - reveal（N → E）：`{ pk: pk_N, nonce: n_N }`——N 在收到并锁定 pk_E 与花名册头部之后才发送
//! - result（E → N）：`{ roster: [头部之后的新条目…], envelope, epoch, roster_index }`
//!
//! `pk` 为三把公钥按 x25519 || roster || auth 拼接的 96 字节。

use zeroize::Zeroizing;

use super::codec::{self, Fields};
use super::crypto::{self, PhraseLanguage, PublicKeys, RecoveryKeys};
use super::engine::{Core, Keyring, open_verified_envelope};
use super::net::{CLIENT_ID, WireEnvelope};
use super::roster::{Entry, Op, Roster, Signer, ZERO_HASH, recovery_bind_message};
use super::{JoinProgress, JoinRequest, JoinRequestStatus, JoinStage, SyncError};

const SAS_MAX_FAILURES: u32 = 3;
const SAS_COOLDOWN_SECS: i64 = 10 * 60;

fn security(message: impl Into<String>) -> SyncError {
    SyncError::Security { message: message.into() }
}

fn invalid(message: impl Into<String>) -> SyncError {
    SyncError::InvalidState { message: message.into() }
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
    let roster = Roster::from_entries(&vault, &raws).map_err(|e| security(format!("本机构造的花名册无效：{e}")))?;
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
    Ok(crypto::phrase_from_entropy(&entropy, lang))
}

// MARK: - 新设备（N）

enum NStage {
    AwaitReply,
    AwaitResult { roster: Box<Roster>, e_id: String, pk_e: PublicKeys, sas: String },
}

pub struct JoinN {
    join_id: String,
    vault: String,
    nonce: Zeroizing<[u8; 32]>,
    stage: NStage,
}

fn progress(join_id: &str, stage: JoinStage, sas: Option<String>) -> JoinProgress {
    JoinProgress { join_id: join_id.to_string(), stage, sas }
}

pub fn start_join(core: &mut Core) -> Result<JoinProgress, SyncError> {
    let keys = core.ensure_device_keys()?;
    if core.enrolled() {
        return Err(invalid("本机已加入保险库"));
    }
    let vault = core.api.vault()?.ok_or_else(|| SyncError::NoVault { message: "此账号还没有保险库，请先在一台设备上创建".into() })?;
    let nonce = Zeroizing::new(crypto::random::<32>());
    let commit = crypto::join_commit(&vault, &keys.public().concat(), &nonce);
    let msg = codec::encode(&codec::map(vec![("v", codec::uint(1)), ("commit", codec::bytes(&commit))]));
    let join_id = core.api.create_join(&vault, &msg)?;
    core.join_n = Some(JoinN { join_id: join_id.clone(), vault, nonce, stage: NStage::AwaitReply });
    Ok(progress(&join_id, JoinStage::WaitingForApprover, None))
}

pub fn cancel_join(core: &mut Core) {
    if let Some(j) = core.join_n.take() {
        let _ = core.api.delete_join(&j.vault, &j.join_id);
    }
}

/// 失败时清理加入请求并报错。
fn fail_join(core: &mut Core, message: String) -> SyncError {
    cancel_join(core);
    core.alert("join_failed", format!("加入失败：{message}"));
    let _ = core.save();
    security(message)
}

pub fn poll_join(core: &mut Core) -> Result<JoinProgress, SyncError> {
    let keys = core.keys()?;
    let me = keys.device_id();
    let pk_n = keys.public();
    let (join_id, vault) = match &core.join_n {
        Some(j) => (j.join_id.clone(), j.vault.clone()),
        None => return Err(invalid("没有进行中的加入请求")),
    };
    let Some(view) = core.api.join(&vault, &join_id)? else {
        core.join_n = None;
        return Ok(progress(&join_id, JoinStage::Expired, None));
    };
    let j = core.join_n.as_ref().unwrap();
    match &j.stage {
        NStage::AwaitReply => {
            let Some(reply) = view.reply else { return Ok(progress(&join_id, JoinStage::WaitingForApprover, None)) };
            let parsed = (|| -> Result<(Roster, PublicKeys, [u8; 32]), String> {
                let mut f = Fields::exact(codec::decode(&codec::from_b64u(&reply)?)?, &["pk", "nonce", "roster"])?;
                let pk_e = PublicKeys::from_concat(&f.bytes("pk")?)?;
                let n_e: [u8; 32] = f.array("nonce")?;
                let ciborium::Value::Array(items) = f.value("roster") else { return Err("花名册格式不正确".into()) };
                let raws: Vec<Vec<u8>> = items.into_iter().map(codec::as_bytes).collect::<Result<_, _>>()?;
                let roster = Roster::from_entries(&vault, &raws)?;
                // pk_E 必须是当前在册设备的公钥
                let e_id = pk_e.device_id();
                if roster.member(&e_id).is_none_or(|m| m.removed.is_some() || m.keys != pk_e) {
                    return Err("批准设备不在册".into());
                }
                if roster.member(&me).is_some() {
                    return Err("本机已在花名册中出现过".into());
                }
                Ok((roster, pk_e, n_e))
            })();
            let (roster, pk_e, n_e) = match parsed {
                Ok(v) => v,
                Err(e) => return Err(fail_join(core, e)),
            };
            // 锁定 pk_E 与花名册头部之后才公开 n_N
            let (_, head) = roster.head().unwrap();
            let j = core.join_n.as_ref().unwrap();
            let sas = crypto::sas(&vault, &head, &pk_n.concat(), &pk_e.concat(), &j.nonce, &n_e);
            let reveal = codec::encode(&codec::map(vec![("pk", codec::bytes(&pk_n.concat())), ("nonce", codec::bytes(&j.nonce[..]))]));
            if !core.api.put_join_step(&vault, &join_id, "reveal", &reveal)? {
                return Err(fail_join(core, "加入流程的状态异常".into()));
            }
            let e_id = pk_e.device_id();
            core.join_n.as_mut().unwrap().stage = NStage::AwaitResult { roster: Box::new(roster), e_id, pk_e, sas: sas.clone() };
            Ok(progress(&join_id, JoinStage::EnterCodeOnOtherDevice, Some(sas)))
        }
        NStage::AwaitResult { roster, e_id, pk_e, sas } => {
            let sas = sas.clone();
            let Some(result) = view.result else { return Ok(progress(&join_id, JoinStage::EnterCodeOnOtherDevice, Some(sas))) };
            let verified = (|| -> Result<(Roster, Keyring), String> {
                let mut f = Fields::exact(codec::decode(&codec::from_b64u(&result)?)?, &["roster", "envelope", "epoch", "roster_index"])?;
                let ciborium::Value::Array(items) = f.value("roster") else { return Err("花名册格式不正确".into()) };
                let raws: Vec<Vec<u8>> = items.into_iter().map(codec::as_bytes).collect::<Result<_, _>>()?;
                // 新花名册必须从锁定的头部延伸，且由 E 签名的 add_device 加入本机
                let start = roster.len() as usize;
                let next = roster.extended(&raws)?;
                let added_by_e = next.entries()[start..]
                    .iter()
                    .any(|e| e.signer == Signer::Device(e_id.clone()) && e.op == Op::AddDevice { device: pk_n.clone() });
                if !added_by_e || !next.is_enrolled(&me) {
                    return Err("花名册中没有由批准设备签名的加入条目".into());
                }
                let envelope = f.bytes("envelope")?;
                let env = WireEnvelope { epoch: f.u32("epoch")?, roster_index: f.u32("roster_index")?, data: codec::b64u(&envelope) };
                let sender = Fields::exact(codec::decode(&envelope)?, &["sender", "enc", "ct"])?.text("sender")?;
                if sender != *e_id || next.member(e_id).map(|m| &m.keys) != Some(pk_e) {
                    return Err("钥匙环不是由批准设备发送的".into());
                }
                let ring = open_verified_envelope(&next, &env, &me, keys.enc_secret())?;
                if ring.get(next.max_epoch()).is_none() {
                    return Err("钥匙环缺少最新的密钥版本".into());
                }
                Ok((next, ring))
            })();
            let (next, ring) = match verified {
                Ok(v) => v,
                Err(e) => return Err(fail_join(core, e)),
            };
            core.adopt_vault(&vault, next, &ring)?;
            let _ = core.api.delete_join(&vault, &join_id);
            core.join_n = None;
            Ok(progress(&join_id, JoinStage::Completed, None))
        }
    }
}

// MARK: - 已授权设备（E）

pub struct JoinE {
    nonce: Zeroizing<[u8; 32]>,
    /// 回复时发送的花名册长度与头部哈希（SAS 绑定的头部）
    roster_len: u32,
    head: [u8; 32],
}

/// 列出进行中的加入请求。对尚未回复的请求，本机发送回复（pk_E、完整花名册、n_E；都不是秘密）。
pub fn list_join_requests(core: &mut Core) -> Result<Vec<JoinRequest>, SyncError> {
    core.require_enrolled()?;
    core.refresh_roster()?;
    let vault = core.vault()?;
    let keys = core.keys()?;
    let views = core.api.joins(&vault)?;
    let mut out = Vec::new();
    for v in &views {
        if v.reply.is_none() && !core.join_e.contains_key(&v.join_id) {
            let nonce = Zeroizing::new(crypto::random::<32>());
            let roster = core.roster()?;
            let (_, head) = roster.head().unwrap();
            let msg = codec::encode(&codec::map(vec![
                ("pk", codec::bytes(&keys.public().concat())),
                ("nonce", codec::bytes(&nonce[..])),
                ("roster", ciborium::Value::Array(roster.raw().iter().map(|r| codec::bytes(r)).collect())),
            ]));
            let len = roster.len();
            if core.api.put_join_step(&vault, &v.join_id, "reply", &msg)? {
                core.join_e.insert(v.join_id.clone(), JoinE { nonce, roster_len: len, head });
            }
        }
        let status = match (core.join_e.contains_key(&v.join_id), v.reveal.is_some()) {
            (true, true) => JoinRequestStatus::ReadyForCode,
            (true, false) => JoinRequestStatus::WaitingForNewDevice,
            (false, _) => JoinRequestStatus::HandledElsewhere,
        };
        out.push(JoinRequest { join_id: v.join_id.clone(), expires_at: v.expires_at, status });
    }
    core.join_e.retain(|id, _| views.iter().any(|v| v.join_id == *id));
    Ok(out)
}

pub fn reject_join(core: &mut Core, join_id: &str) -> Result<(), SyncError> {
    let vault = core.vault()?;
    core.join_e.remove(join_id);
    core.api.delete_join(&vault, join_id)
}

/// 用户在本机输入新设备上显示的校验码。输入不符时本地计数，3 次后冷却 10 分钟。
pub fn approve_join(core: &mut Core, join_id: &str, typed_sas: &str) -> Result<(), SyncError> {
    let me = core.require_enrolled()?;
    let now = core.now();
    if let Some(until) = core.state.sas_cooldown_until
        && until > now
    {
        return Err(SyncError::Cooldown { seconds: (until - now) as u64 });
    }
    let vault = core.vault()?;
    let keys = core.keys()?;
    let Some(e) = core.join_e.get(join_id) else { return Err(invalid("该加入请求不是由本机处理的，请在列表中重新打开")) };
    let (nonce_e, sent_len, head) = (e.nonce.clone(), e.roster_len, e.head);
    let view = core.api.join(&vault, join_id)?.ok_or_else(|| invalid("加入请求已过期"))?;
    let (Some(commit), Some(reveal)) = (view.commit, view.reveal) else { return Err(invalid("新设备尚未显示校验码")) };
    let checked = (|| -> Result<(PublicKeys, [u8; 32]), String> {
        let mut c = Fields::exact(codec::decode(&codec::from_b64u(&commit)?)?, &["v", "commit"])?;
        let c_value: [u8; 32] = c.array("commit")?;
        let mut r = Fields::exact(codec::decode(&codec::from_b64u(&reveal)?)?, &["pk", "nonce"])?;
        let pk_n = PublicKeys::from_concat(&r.bytes("pk")?)?;
        let n_n: [u8; 32] = r.array("nonce")?;
        if !crypto::ct_eq(&crypto::join_commit(&vault, &pk_n.concat(), &n_n), &c_value) {
            return Err("新设备的承诺校验失败".into());
        }
        Ok((pk_n, n_n))
    })();
    let (pk_n, n_n) = match checked {
        Ok(v) => v,
        Err(msg) => {
            core.join_e.remove(join_id);
            let _ = core.api.delete_join(&vault, join_id);
            core.alert("join_failed", format!("加入请求被拒绝：{msg}"));
            core.save()?;
            return Err(security(msg));
        }
    };
    let expected = crypto::sas(&vault, &head, &pk_n.concat(), &keys.public().concat(), &n_n, &nonce_e);
    if !crypto::ct_eq(crypto::normalize_sas(typed_sas).as_bytes(), crypto::normalize_sas(&expected).as_bytes()) {
        core.state.sas_failures += 1;
        if core.state.sas_failures >= SAS_MAX_FAILURES {
            core.state.sas_failures = 0;
            core.state.sas_cooldown_until = Some(now + SAS_COOLDOWN_SECS);
            core.join_e.remove(join_id);
            let _ = core.api.delete_join(&vault, join_id);
            core.alert("sas_cooldown", "校验码连续输入错误 3 次，加入请求已取消，10 分钟后才能再次批准");
            core.save()?;
            return Err(SyncError::Cooldown { seconds: SAS_COOLDOWN_SECS as u64 });
        }
        core.save()?;
        return Err(SyncError::SasMismatch { remaining: SAS_MAX_FAILURES - core.state.sas_failures });
    }
    core.state.sas_failures = 0;
    core.save()?;

    // CAS 追加 add_device(N)，然后发送 HPKE Auth 钥匙环
    let n_id = pk_n.device_id();
    core.refresh_roster()?;
    let added = core.append_roster(|r| {
        if r.member(&n_id).is_some() {
            return Err(invalid("该设备已在花名册中出现过"));
        }
        let (idx, prev) = r.head().unwrap();
        let entry = Entry::signed(&vault, idx + 1, prev, Op::AddDevice { device: pk_n.clone() }, Signer::Device(me.clone()), |m| keys.roster_sign(m));
        Ok(Some(entry.encode()))
    })?;
    if !added {
        return Err(invalid("无法加入该设备"));
    }
    let ring = core.ensure_keys()?;
    let (epoch, roster_index, envelope) = core.write_envelope(&ring, &n_id, &pk_n.x25519)?;
    let new_entries: Vec<ciborium::Value> = core.roster()?.raw()[sent_len as usize..].iter().map(|r| codec::bytes(r)).collect();
    let result = codec::encode(&codec::map(vec![
        ("roster", ciborium::Value::Array(new_entries)),
        ("envelope", codec::bytes(&envelope)),
        ("epoch", codec::uint(epoch as u64)),
        ("roster_index", codec::uint(roster_index as u64)),
    ]));
    if !core.api.put_join_step(&vault, join_id, "result", &result)? {
        return Err(invalid("加入流程的状态异常，请让新设备重新发起"));
    }
    core.join_e.remove(join_id);
    Ok(())
}

// MARK: - 恢复短语加入（§6.3）

pub fn recover(core: &mut Core, phrase: &str) -> Result<(), SyncError> {
    let rk = RecoveryKeys::from_phrase(phrase).map_err(|message| SyncError::InvalidInput { message })?;
    let keys = core.ensure_device_keys()?;
    let me = keys.device_id();
    let vault = core.api.vault()?.ok_or_else(|| SyncError::NoVault { message: "此账号还没有保险库".into() })?;
    let (raws, _) = core.api.roster(&vault, 0)?;
    let roster = Roster::from_entries(&vault, &raws).map_err(|e| security(format!("花名册校验失败：{e}")))?;
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
    // 信封明文中的花名册头部是下限：花名册早于它会在校验中失败
    let ring = open_verified_envelope(&roster, env, crypto::RECOVERY_RECIPIENT, rk.x_secret()).map_err(|e| security(format!("恢复信封校验失败：{e}")))?;
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
