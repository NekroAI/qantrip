//! 同步引擎（docs/sync-design.md 第 4、5 节）：花名册刷新、钥匙环、记录校验、拉取、推送、冲突与对账。
//!
//! 加入、恢复见 `join.rs`，撤销与轮换见 `rotate.rs`。
//!
//! 告警与错误信息只由本地固定文本和本机已验证花名册中的设备简称组成，不包含服务端可控的内容
//! （记录 ID、epoch 标签、解析错误细节等），也让服务端无法用大量不同的告警挤掉真正的告警。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use ciborium::Value;
use zeroize::Zeroizing;

use super::codec::{self, Fields};
use super::crypto::{self, DeviceKeys};
use super::files;
use super::net::{Api, WireEnvelope, WireRecord, WriteReq, WriteResult};
use super::record::{Body, Kind, MAX_COUNTER, Plaintext, StateEntry};
use super::roster::Roster;
use super::state::{LocalState, PeerEntry, RecordMeta, StoredAlert};
use super::{SecretStore, SyncError, SyncReport};

pub const SECRET_DEVICE_KEYS: &str = "device_keys";
pub const SECRET_KEYRING: &str = "vault_keyring";
const BATCH: usize = 50;
const MAX_ALERTS: usize = 100;
/// 一次拉取处理的记录总数上限（服务端每个保险库最多 5,000 条记录，L3）
const MAX_PULL_RECORDS: usize = 20_000;

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

fn security(message: impl Into<String>) -> SyncError {
    SyncError::Security { message: message.into() }
}

fn short(id: &str) -> &str {
    &id[..id.len().min(6)]
}

pub fn valid_record_id(id: &str) -> bool {
    (16..=43).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

// MARK: - 钥匙环

/// VK 钥匙环 `{epoch → VK}`，析构时清零。
#[derive(Default, Clone)]
pub struct Keyring(BTreeMap<u32, Zeroizing<[u8; 32]>>);

impl Keyring {
    pub fn get(&self, epoch: u32) -> Option<&[u8; 32]> {
        self.0.get(&epoch).map(|k| &**k)
    }

    pub fn insert(&mut self, epoch: u32, vk: [u8; 32]) {
        self.0.insert(epoch, Zeroizing::new(vk));
    }

    pub fn remove(&mut self, epoch: u32) {
        self.0.remove(&epoch);
    }

    pub fn max_epoch(&self) -> u32 {
        self.0.keys().next_back().copied().unwrap_or(0)
    }

    pub fn epochs(&self) -> Vec<u32> {
        self.0.keys().copied().collect()
    }

    pub fn retain_from(&mut self, min: u32) {
        self.0.retain(|e, _| *e >= min);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn value(&self) -> Value {
        Value::Map(self.0.iter().map(|(e, k)| (codec::uint(*e as u64), codec::bytes(&k[..]))).collect())
    }

    fn parse(v: Value) -> Result<Keyring, String> {
        let Value::Map(items) = v else { return Err("钥匙环格式不正确".into()) };
        let mut out = Keyring::default();
        for (e, k) in items {
            let epoch = codec::as_u64(&e).and_then(|e| u32::try_from(e).ok()).filter(|e| *e >= 1).ok_or("钥匙环的 epoch 不正确")?;
            let mut vk = Zeroizing::new(codec::as_bytes(k)?);
            let key: [u8; 32] = vk.as_slice().try_into().map_err(|_| "VK 长度不正确".to_string())?;
            vk.iter_mut().for_each(|b| *b = 0);
            out.insert(epoch, key);
        }
        Ok(out)
    }

    pub fn encode(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(codec::encode(&self.value()))
    }

    pub fn decode(b: &[u8]) -> Result<Keyring, String> {
        Keyring::parse(codec::decode(b)?)
    }
}

/// 信封明文：`{ keys: {epoch → VK}, roster: { index, head } }`；
/// 信封数据：`{ sender, enc, ct }`（发送方 device_id、HPKE 封装密钥、密文）。
pub fn envelope_plaintext(ring: &Keyring, index: u32, head: &[u8; 32]) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(codec::encode(&codec::map(vec![
        ("keys", ring.value()),
        ("roster", codec::map(vec![("index", codec::uint(index as u64)), ("head", codec::bytes(head))])),
    ])))
}

/// 打开并验证信封（§3.4）：发送方在信封生成时在册、HPKE Auth、info 绑定、明文中的花名册头部、每个 epoch 的 vk_commit。
/// 任何一项不符就整体拒绝。
pub fn open_verified_envelope(roster: &Roster, env: &WireEnvelope, recipient_id: &str, recipient_sk: &[u8; 32]) -> Result<Keyring, String> {
    let data = codec::from_b64u(&env.data)?;
    let mut f = Fields::exact(codec::decode(&data)?, &["sender", "enc", "ct"])?;
    let sender = f.text("sender")?;
    let enc: [u8; 32] = f.array("enc")?;
    let ct = f.bytes("ct")?;
    let sender_pk = roster.enrolled_at(&sender, env.roster_index).ok_or("信封发送方在信封生成时不在册")?.x25519;
    let info = crypto::envelope_info(roster.vault_id(), recipient_id, env.epoch, env.roster_index);
    let pt = crypto::open_envelope(recipient_sk, &sender_pk, &enc, &info, &ct)?;
    let mut f = Fields::exact(codec::decode(&pt)?, &["keys", "roster"])?;
    let ring = Keyring::parse(f.value("keys"))?;
    let mut r = Fields::exact(f.value("roster"), &["index", "head"])?;
    let (index, head) = (r.u32("index")?, r.array::<32>("head")?);
    // 恢复信封的头部是下限：本地花名册必须不早于它（§6.3）
    if index != env.roster_index || roster.hash_at(index) != Some(head) {
        return Err("信封中的花名册头部与本地花名册不符".into());
    }
    if ring.is_empty() || ring.max_epoch() != env.epoch {
        return Err("信封中的钥匙环与 epoch 不符".into());
    }
    for e in ring.epochs() {
        let commit = roster.epoch(e).ok_or("钥匙环包含花名册中不存在的 epoch")?.vk_commit;
        if !crypto::ct_eq(&crypto::vk_commit(ring.get(e).unwrap(), roster.vault_id(), e), &commit) {
            return Err(format!("epoch {e} 的 VK 与花名册中的承诺不符"));
        }
    }
    Ok(ring)
}

// MARK: - 记录校验结果

pub struct Accepted {
    pub pt: Plaintext,
    pub hash: [u8; 32],
    pub wire: WireRecord,
}

pub enum Verdict {
    /// 新版本，已通过全部校验
    Accept(Box<Accepted>),
    /// 与本地已知版本相同
    Same(Box<Accepted>),
    /// 校验未通过（已报警）
    Reject,
    /// 暂时无法校验（缺少密钥版本、花名册头部未传播）：保留在增量拉取范围内，下次重试
    Retry,
}

/// 一条待推送的写入。
pub struct Pending {
    pub req: WriteReq,
    pub pt: Plaintext,
    pub hash: [u8; 32],
    /// 模板文件名与写入时的文件内容哈希
    pub file: Option<(String, String)>,
}

// MARK: - 核心

pub struct Core {
    pub api: Api,
    pub secrets: Arc<dyn SecretStore>,
    pub server: String,
    pub data_dir: PathBuf,
    pub template_dir: PathBuf,
    pub state: LocalState,
    pub roster: Option<Roster>,
    pub clock: Clock,
    /// 本次操作新增的告警数
    pub new_alerts: u32,
    pub login: Option<super::join::PendingLogin>,
    pub join_n: Option<super::join::JoinN>,
    pub join_e: BTreeMap<String, super::join::JoinE>,
}

impl Core {
    pub fn new(api: Api, secrets: Arc<dyn SecretStore>, server: &str, data_dir: PathBuf, template_dir: PathBuf, clock: Clock) -> Core {
        let state = LocalState::load(&data_dir, server);
        let roster = state.vault_id.as_ref().and_then(|v| {
            let raws: Option<Vec<Vec<u8>>> = state.roster.iter().map(|r| codec::from_b64u(r).ok()).collect();
            Roster::from_entries(v, &raws?).ok()
        });
        let join_n = super::join::load_join_n(&*secrets);
        let join_e = super::join::load_join_e(&*secrets);
        Core {
            api,
            secrets,
            server: server.to_string(),
            data_dir,
            template_dir,
            state,
            roster,
            clock,
            new_alerts: 0,
            login: None,
            join_n,
            join_e,
        }
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    pub fn save(&self) -> Result<(), SyncError> {
        self.state.save(&self.data_dir).map_err(|message| SyncError::Storage { message })
    }

    pub fn alert(&mut self, code: &str, message: impl Into<String>) {
        let message = message.into();
        if self.state.alerts.iter().any(|a| a.code == code && a.message == message) {
            return;
        }
        self.new_alerts += 1;
        let at = self.now();
        self.state.alerts.push(StoredAlert { code: code.to_string(), message, at });
        if self.state.alerts.len() > MAX_ALERTS {
            self.state.alerts.remove(0);
        }
    }

    // MARK: 密钥

    pub fn device_keys(&self) -> Result<Option<DeviceKeys>, SyncError> {
        match self.secrets.get(SECRET_DEVICE_KEYS.into())? {
            Some(b) => {
                let b = Zeroizing::new(b);
                DeviceKeys::from_bytes(&b).map(Some).map_err(|message| SyncError::Storage { message })
            }
            None => Ok(None),
        }
    }

    pub fn ensure_device_keys(&mut self) -> Result<DeviceKeys, SyncError> {
        if let Some(k) = self.device_keys()? {
            return Ok(k);
        }
        let k = DeviceKeys::generate();
        self.secrets.set(SECRET_DEVICE_KEYS.into(), k.to_bytes().to_vec())?;
        Ok(k)
    }

    pub fn keys(&self) -> Result<DeviceKeys, SyncError> {
        self.device_keys()?.ok_or_else(|| SyncError::InvalidState { message: "本机尚未生成设备密钥，请先登录".into() })
    }

    pub fn me(&self) -> Result<String, SyncError> {
        Ok(self.keys()?.device_id())
    }

    pub fn keyring(&self) -> Result<Keyring, SyncError> {
        match self.secrets.get(SECRET_KEYRING.into())? {
            Some(b) => Keyring::decode(&Zeroizing::new(b)).map_err(|message| SyncError::Storage { message }),
            None => Ok(Keyring::default()),
        }
    }

    pub fn store_keyring(&self, ring: &Keyring) -> Result<(), SyncError> {
        Ok(self.secrets.set(SECRET_KEYRING.into(), ring.encode().to_vec())?)
    }

    // MARK: 保险库与花名册

    pub fn vault(&self) -> Result<String, SyncError> {
        self.state.vault_id.clone().ok_or_else(|| SyncError::NoVault { message: "本机尚未加入保险库".into() })
    }

    pub fn roster(&self) -> Result<&Roster, SyncError> {
        self.roster.as_ref().ok_or_else(|| SyncError::NoVault { message: "本机尚未加入保险库".into() })
    }

    pub fn set_roster(&mut self, r: Roster) {
        self.state.roster = r.raw().iter().map(|b| codec::b64u(b)).collect();
        self.roster = Some(r);
    }

    /// 加入或恢复成功后，建立新的本地保险库状态。
    pub fn adopt_vault(&mut self, vault: &str, roster: Roster, ring: &Keyring) -> Result<(), SyncError> {
        self.store_keyring(ring)?;
        self.state.reset_vault();
        self.state.vault_id = Some(vault.to_string());
        self.state.needs_full = true;
        self.set_roster(roster);
        self.save()
    }

    pub fn enrolled(&self) -> bool {
        match (&self.roster, self.device_keys()) {
            (Some(r), Ok(Some(k))) => r.is_enrolled(&k.device_id()),
            _ => false,
        }
    }

    pub fn require_enrolled(&self) -> Result<String, SyncError> {
        let me = self.me()?;
        if !self.roster()?.is_enrolled(&me) {
            return Err(SyncError::NotEnrolled { message: "本机不在保险库的设备名单中".into() });
        }
        Ok(me)
    }

    /// 拉取新的花名册条目并验证它从本地已验证头部延伸（§6.1）。失败时停止写入并报警。
    pub fn refresh_roster(&mut self) -> Result<bool, SyncError> {
        let vault = self.vault()?;
        let roster = self.roster()?.clone();
        let (raws, length) = self.api.roster(&vault, roster.len())?;
        if length < roster.len() {
            self.alert("roster_truncated", "服务端返回的花名册比本机已验证的更短（疑似截断），已停止写入");
            self.state.writes_blocked = Some(u32::MAX);
            self.save()?;
            return Err(security("花名册被截断"));
        }
        if raws.is_empty() {
            return Ok(false);
        }
        match roster.extended(&raws) {
            Ok(r) => {
                self.set_roster(r);
                self.save()?;
                Ok(true)
            }
            Err(_) => {
                self.alert("roster_invalid", "服务端返回的花名册未通过校验，已停止写入");
                self.state.writes_blocked = Some(u32::MAX);
                self.save()?;
                Err(security("花名册校验失败"))
            }
        }
    }

    /// 确保钥匙环含有花名册中最新的 epoch：缺少时取本机信封并验证。丢弃已完成轮换之前的旧 VK。
    pub fn ensure_keys(&mut self) -> Result<Keyring, SyncError> {
        let vault = self.vault()?;
        let keys = self.keys()?;
        let me = keys.device_id();
        let roster = self.roster()?.clone();
        let mut ring = self.keyring()?;
        let before = ring.epochs();
        if ring.get(roster.max_epoch()).is_none() {
            let mut envs = self.api.envelopes(&vault, &me)?;
            envs.sort_by_key(|e| std::cmp::Reverse(e.epoch));
            for env in envs.iter().filter(|e| e.epoch > ring.max_epoch()) {
                if env.roster_index >= roster.len() {
                    // 信封引用了更新的花名册：先刷新再验证
                    self.refresh_roster()?;
                }
                let roster = self.roster()?;
                match open_verified_envelope(roster, env, &me, keys.enc_secret()) {
                    Ok(got) => {
                        for e in got.epochs() {
                            ring.insert(e, *got.get(e).unwrap());
                        }
                        break;
                    }
                    Err(_) => self.alert("envelope_invalid", "本机的密钥信封未通过校验，已忽略"),
                }
            }
        }
        ring.retain_from(self.roster()?.min_epoch());
        if ring.epochs() != before {
            self.store_keyring(&ring)?;
        }
        Ok(ring)
    }

    /// 写入使用的 epoch：花名册中的最新 epoch（§7：轮换进行中时一律使用新 epoch）。还没有拿到它的 VK 时返回 None。
    pub fn write_key(&mut self) -> Result<Option<(u32, [u8; 32])>, SyncError> {
        let ring = self.ensure_keys()?;
        let epoch = self.roster()?.max_epoch();
        Ok(ring.get(epoch).map(|k| (epoch, *k)))
    }

    /// 写入一个信封（发送方为本机）。返回 (epoch, roster_index, 信封数据)。
    pub fn write_envelope(&self, ring: &Keyring, recipient_id: &str, recipient_pk: &[u8; 32]) -> Result<(u32, u32, Vec<u8>), SyncError> {
        let vault = self.vault()?;
        let keys = self.keys()?;
        let (index, head) = self.roster()?.head().ok_or_else(|| security("花名册为空"))?;
        let epoch = ring.max_epoch();
        let pt = envelope_plaintext(ring, index, &head);
        let info = crypto::envelope_info(&vault, recipient_id, epoch, index);
        let (enc, ct) = crypto::seal_envelope(&mut crypto::system_rng(), keys.enc_secret(), recipient_pk, &info, &pt).map_err(security)?;
        let data = codec::encode(&codec::map(vec![("sender", codec::text(&keys.device_id())), ("enc", codec::bytes(&enc)), ("ct", codec::bytes(&ct))]));
        self.api.put_envelope(&vault, recipient_id, epoch, index, &data)?;
        Ok((epoch, index, data))
    }

    /// CAS 追加一条花名册条目。`build` 根据当前花名册构造条目（返回 None 表示条件已不满足，放弃）。
    /// 409 时刷新花名册后重试。
    pub fn append_roster(&mut self, mut build: impl FnMut(&Roster) -> Result<Option<Vec<u8>>, SyncError>) -> Result<bool, SyncError> {
        let vault = self.vault()?;
        for _ in 0..5 {
            let roster = self.roster()?.clone();
            let Some(raw) = build(&roster)? else { return Ok(false) };
            let next = roster.extended(std::slice::from_ref(&raw)).map_err(|_| security("本机构造的花名册条目无效"))?;
            if self.api.append_roster(&vault, roster.len(), &raw)? {
                self.set_roster(next);
                self.save()?;
                return Ok(true);
            }
            self.refresh_roster()?;
        }
        Err(SyncError::Conflict { message: "花名册被其他设备频繁更新，请稍后重试".into() })
    }

    // MARK: 记录校验（§4.3）

    /// 若 record_id 是花名册中某台设备的状态记录 ID，返回该设备。
    fn state_owner(&self, record_id: &str) -> Option<String> {
        let roster = self.roster.as_ref()?;
        roster.members().map(|(id, _)| id).find(|id| crypto::state_record_id(roster.vault_id(), id) == record_id).cloned()
    }

    /// 解密后的全部校验：AEAD、类型绑定、花名册头部传播、写入者在册、写入者签名、防回滚与分叉、epoch 下限、
    /// 已撤销写入者。
    pub fn verify_record(&mut self, w: &WireRecord) -> Result<Verdict, SyncError> {
        let vault = self.vault()?;
        let me = self.me()?;
        let rid = &w.record_id;
        if !valid_record_id(rid) || w.seq == 0 {
            self.alert("bad_record", "服务端返回了格式不正确的记录，已丢弃");
            return Ok(Verdict::Reject);
        }
        let owner = self.state_owner(rid);
        let min_epoch = self.roster()?.min_epoch();
        if w.epoch < min_epoch {
            // 本机写入、但轮换时没有被重新加密的版本（轮换期间才被接受的版本，§7）：用当前密钥重新推送
            if let Some(m) = self.state.records.get_mut(rid)
                && m.writer == me
                && m.kind == Kind::Template
                && m.epoch < min_epoch
            {
                m.dirty = true;
                m.seq = Some(w.seq);
                return Ok(Verdict::Reject);
            }
            // 已撤销设备的状态记录停留在旧 epoch，属于正常情况，不报警
            if owner.is_none() {
                self.alert("stale_epoch", "一条记录使用已作废的密钥版本，已拒收");
            }
            return Ok(Verdict::Reject);
        }
        let mut ring = self.keyring()?;
        if ring.get(w.epoch).is_none() && w.epoch > ring.max_epoch() {
            self.refresh_roster()?;
            ring = self.ensure_keys()?;
        }
        let Some(vk) = ring.get(w.epoch).copied().map(Zeroizing::new) else {
            self.alert("missing_key", "有记录使用了本机没有的密钥版本，暂时无法读取");
            return Ok(Verdict::Retry);
        };
        let opened = codec::from_b64u(&w.ciphertext).and_then(|ct| crypto::open_record(&vk, &vault, rid, w.epoch, &ct));
        let Ok(padded) = opened else {
            self.alert("tampered", "一条记录的密文校验失败（可能被篡改或挪用），已丢弃");
            return Ok(Verdict::Reject);
        };
        let Ok((pt, hash)) = Plaintext::decode_padded(&padded) else {
            self.alert("bad_plaintext", "一条记录的内容格式不正确，已丢弃");
            return Ok(Verdict::Reject);
        };
        // 状态记录只能由对应设备写入；其他类型不能占用状态记录 ID
        let is_state_id = crypto::state_record_id(&vault, &pt.writer) == *rid;
        if (pt.kind == Kind::DeviceState) != is_state_id || (pt.kind != Kind::DeviceState && owner.is_some()) {
            self.alert("bad_kind", "一条记录的类型与 ID 不符，已丢弃");
            return Ok(Verdict::Reject);
        }
        // 头部传播：写入者见过的花名册不能比本机新；拉不到就停止写入（§4.3 第 2 步）
        if pt.roster_index >= self.roster()?.len() {
            let _ = self.refresh_roster();
            if pt.roster_index >= self.roster()?.len() {
                self.alert("roster_withheld", "有记录引用了本机拿不到的花名册条目，疑似服务端扣留，已停止写入");
                let need = self.state.writes_blocked.unwrap_or(0).max(pt.roster_index);
                self.state.writes_blocked = Some(need);
                return Ok(Verdict::Retry);
            }
        }
        let roster = self.roster()?;
        if roster.hash_at(pt.roster_index) != Some(pt.roster_head) {
            self.alert("roster_fork", "有记录引用的花名册与本机不一致（疑似分叉），已停止写入");
            self.state.writes_blocked = Some(u32::MAX);
            return Ok(Verdict::Reject);
        }
        let Some(writer_pk) = roster.enrolled_at(&pt.writer, pt.roster_index).map(|k| k.roster) else {
            self.alert("writer_not_enrolled", "一条记录的写入者不在册，已丢弃");
            return Ok(Verdict::Reject);
        };
        // 写入者签名（v0.3）：持有 VK 的其他设备或被撤销的设备不能冒充写入者
        if pt.verify(&vault, rid, w.epoch, &writer_pk).is_err() {
            self.alert("bad_signature", "一条记录的写入者签名无效，已丢弃");
            return Ok(Verdict::Reject);
        }
        let writer_revoked = roster.member(&pt.writer).is_some_and(|m| m.removed.is_some());
        let acc = Box::new(Accepted { pt, hash, wire: w.clone() });
        // 防回滚与分叉（§4.3 第 3 步）
        if let Some(m) = self.state.records.get_mut(rid) {
            let hash_b64 = codec::b64u(&hash);
            if acc.pt.counter < m.counter {
                if m.writer == me {
                    // 自己写入的新版本被回滚：自动重新推送
                    m.dirty = true;
                    m.seq = Some(w.seq);
                }
                self.alert("rollback", "有记录被回滚到旧版本，已拒收");
                return Ok(Verdict::Reject);
            }
            if acc.pt.counter == m.counter {
                if hash_b64 != m.hash {
                    self.alert("fork", "有记录出现同一版本号的不同内容（疑似分叉），已拒收");
                    return Ok(Verdict::Reject);
                }
                return Ok(Verdict::Same(acc));
            }
        }
        // 写入者已被移除：不接受任何本机之前没见过的版本（§7，被撤销设备可能与服务端勾结写入旧 epoch）
        if writer_revoked {
            if acc.pt.kind != Kind::DeviceState {
                self.alert("revoked_writer", "有记录由已移除的设备写入，已拒收");
            }
            return Ok(Verdict::Reject);
        }
        Ok(Verdict::Accept(acc))
    }

    /// 记录一个已接受（或已推送成功）的版本，保留模板文件映射。
    pub fn ack(&mut self, rid: &str, pt: &Plaintext, hash: &[u8; 32], seq: u64, epoch: u32, file: Option<Option<(String, String)>>) {
        let accepted_len = self.roster.as_ref().map_or(0, |r| r.len());
        let prev = self.state.records.get(rid);
        let (file, file_hash) = match file {
            Some(Some((f, h))) => (Some(f), Some(h)),
            Some(None) => (None, None),
            None => (prev.and_then(|m| m.file.clone()), prev.and_then(|m| m.file_hash.clone())),
        };
        self.state.records.insert(
            rid.to_string(),
            RecordMeta {
                kind: pt.kind,
                counter: pt.counter,
                hash: codec::b64u(hash),
                deleted: pt.deleted,
                seq: Some(seq),
                epoch,
                writer: pt.writer.clone(),
                file,
                file_hash,
                dirty: false,
                accepted_len,
            },
        );
    }

    /// 应用一个通过校验的新版本：写入模板目录或保存其他设备的状态记录。
    pub fn apply(&mut self, acc: &Accepted, report: &mut SyncReport) -> Result<(), SyncError> {
        let rid = acc.wire.record_id.clone();
        let file = match (&acc.pt.kind, &acc.pt.body) {
            (Kind::DeviceState, Body::DeviceState { entries }) => {
                if acc.pt.writer != self.me()? {
                    let peer = entries
                        .iter()
                        .map(|(k, e)| (k.clone(), PeerEntry { counter: e.counter, hash: codec::b64u(&e.hash), deleted: e.deleted }))
                        .collect();
                    self.state.peers.insert(acc.pt.writer.clone(), peer);
                }
                None
            }
            (Kind::Template, _) => Some(self.apply_template(&rid, &acc.pt, report)?),
            // 设置与 AI 配置尚未接入：只记录版本，不落地
            _ => None,
        };
        self.ack(&rid, &acc.pt, &acc.hash, acc.wire.seq, acc.wire.epoch, file);
        report.pulled += 1;
        Ok(())
    }

    fn device_short(&self) -> String {
        self.me().map(|m| short(&m).to_lowercase()).unwrap_or_default()
    }

    /// 把本地文件另存为冲突副本（§5.5）。
    fn save_conflict(&mut self, name: &str, data: &[u8], report: &mut SyncReport) -> Result<(), SyncError> {
        let copy = files::conflict_name(&self.template_dir, name, &self.device_short(), self.now());
        files::write(&self.template_dir, &copy, data).map_err(|message| SyncError::Storage { message })?;
        report.conflicts += 1;
        report.conflict_files.push(copy);
        Ok(())
    }

    /// 把远端模板版本落到模板目录。本地有未同步的修改时，远端版本写入原文件，本地版本另存为冲突副本。
    /// 返回新的文件映射。
    fn apply_template(&mut self, rid: &str, pt: &Plaintext, report: &mut SyncReport) -> Result<Option<(String, String)>, SyncError> {
        let dir = self.template_dir.clone();
        let storage = |message: String| SyncError::Storage { message };
        let meta = self.state.records.get(rid).cloned();
        let old_file = meta.as_ref().and_then(|m| m.file.clone());
        let old_hash = meta.as_ref().and_then(|m| m.file_hash.clone());
        // 旧文件：内容未变则可以直接删除/覆盖，否则先保存冲突副本
        let release_old = |this: &mut Core, report: &mut SyncReport| -> Result<(), SyncError> {
            if let Some(f) = &old_file
                && let Some(bytes) = files::read(&dir, f)
            {
                if Some(files::hash(&bytes)) != old_hash {
                    this.save_conflict(f, &bytes, report)?;
                }
                files::remove(&dir, f).map_err(storage)?;
            }
            Ok(())
        };

        let Body::Template { file_name, source } = &pt.body else {
            // 墓碑
            release_old(self, report)?;
            return Ok(None);
        };
        if !files::valid_file_name(file_name) {
            self.alert("bad_file_name", "一条模板记录的文件名不合法，未写入模板目录");
            return Ok(None);
        }
        // 同名文件已被另一条记录占用（两台设备并发新建同名模板；比较不区分大小写与 Unicode 规范化形式）时，换一个名字
        let key = files::name_key(file_name);
        let taken = self.state.records.iter().any(|(id, m)| id != rid && !m.deleted && m.file.as_deref().is_some_and(|f| files::name_key(f) == key));
        let target = if taken { format!("{}-{}.md", file_name.trim_end_matches(".md"), short(rid)) } else { file_name.clone() };
        if old_file.as_deref() != Some(target.as_str()) {
            release_old(self, report)?;
        }
        if let Some(bytes) = files::read(&dir, &target)
            && bytes != source.as_bytes()
        {
            let unchanged = old_file.as_deref() == Some(target.as_str()) && Some(files::hash(&bytes)) == old_hash;
            if !unchanged {
                self.save_conflict(&target, &bytes, report)?;
            }
        }
        files::write(&dir, &target, source.as_bytes()).map_err(storage)?;
        Ok(Some((target, files::hash(source.as_bytes()))))
    }

    // MARK: 同步（§5）

    pub fn sync(&mut self) -> Result<SyncReport, SyncError> {
        self.new_alerts = 0;
        let mut report = SyncReport::default();
        let result = self.sync_inner(&mut report);
        if result.is_ok() {
            self.state.last_sync = Some(self.now());
        }
        self.save()?;
        report.alerts = self.new_alerts;
        result.map(|_| report)
    }

    fn sync_inner(&mut self, report: &mut SyncReport) -> Result<(), SyncError> {
        let vault = self.vault()?;
        let me = self.me()?;
        self.refresh_roster()?;
        if !self.roster()?.is_enrolled(&me) {
            if self.roster()?.member(&me).is_some() {
                self.alert("revoked", "本机已被移出保险库，同步已停止");
            }
            return Err(SyncError::NotEnrolled { message: "本机不在保险库的设备名单中".into() });
        }
        self.ensure_keys()?;
        if let Some(need) = self.state.writes_blocked
            && need != u32::MAX
            && self.roster()?.len() > need
        {
            self.state.writes_blocked = None;
        }
        super::rotate::handle_pending(self, report)?;

        let head = self.api.head(&vault)?;
        if self.state.needs_full || self.state.last_seq == 0 || head.seq != self.state.last_seq || head.roster_index != self.roster()?.len() {
            self.pull(report)?;
        }
        self.reconcile();
        if self.state.writes_blocked.is_some() {
            report.writes_blocked = true;
            return Ok(());
        }
        std::fs::create_dir_all(&self.template_dir).map_err(|e| SyncError::Storage { message: format!("无法创建模板目录：{e}") })?;
        self.push_local(report)?;
        self.write_own_state(report)?;
        Ok(())
    }

    /// 拉取全部记录（全量对账）。返回服务端当前的全部记录。
    pub fn list_all(&mut self) -> Result<Vec<WireRecord>, SyncError> {
        let vault = self.vault()?;
        let mut out = Vec::new();
        let mut since = 0;
        loop {
            let page = self.api.changes(&vault, since)?;
            out.extend(page.records);
            if out.len() > MAX_PULL_RECORDS {
                return Err(security("服务端返回的记录数超出上限"));
            }
            match page.next {
                Some(n) if n > since => since = n,
                _ => return Ok(out),
            }
        }
    }

    /// 增量拉取（分页）；首次同步、`gone`、墓碑清理越过上次 seq 时转为全量对账（§4.5）。
    fn pull(&mut self, report: &mut SyncReport) -> Result<(), SyncError> {
        let vault = self.vault()?;
        let mut full = self.state.needs_full || self.state.last_seq == 0;
        let mut since = if full { 0 } else { self.state.last_seq };
        let mut seen = BTreeSet::new();
        let mut max_seq = 0;
        let mut retry: Option<u64> = None;
        let mut total = 0usize;
        loop {
            let page = self.api.changes(&vault, since)?;
            if !full && page.purged_seq > self.state.last_seq {
                full = true;
                since = 0;
                continue;
            }
            total += page.records.len();
            if total > MAX_PULL_RECORDS {
                return Err(security("服务端返回的记录数超出上限"));
            }
            for w in &page.records {
                seen.insert(w.record_id.clone());
                max_seq = max_seq.max(w.seq);
                match self.verify_record(w)? {
                    Verdict::Accept(acc) => self.apply(&acc, report)?,
                    Verdict::Same(acc) => {
                        if let Some(m) = self.state.records.get_mut(&w.record_id) {
                            m.seq = Some(acc.wire.seq);
                            m.epoch = acc.wire.epoch;
                        }
                    }
                    Verdict::Reject => report.rejected += 1,
                    Verdict::Retry => {
                        report.rejected += 1;
                        retry = Some(retry.map_or(w.seq, |r: u64| r.min(w.seq)));
                    }
                }
            }
            match page.next {
                Some(n) if n > since => since = n,
                _ => {
                    max_seq = max_seq.max(page.head);
                    break;
                }
            }
        }
        if full {
            // 服务端应当保存本机知道的每一条未删除记录（§4.4）
            let missing: Vec<String> = self.state.records.iter().filter(|(id, m)| !seen.contains(*id) && m.seq.is_some()).map(|(id, _)| id.clone()).collect();
            for id in missing {
                let me = self.me()?;
                let m = self.state.records.get_mut(&id).unwrap();
                if m.deleted {
                    // 墓碑按 180 天规则被清理，属于正常情况
                    continue;
                }
                m.seq = None;
                if m.writer == me || m.kind == Kind::Template {
                    m.dirty = true;
                }
                self.alert("hidden", "服务端缺少本机已知的记录（疑似被隐藏或删除），将重新推送本机版本");
            }
            self.state.needs_full = false;
        }
        self.state.last_seq = self.state.last_seq.max(max_seq);
        if full {
            self.state.last_seq = max_seq;
        }
        if let Some(r) = retry {
            // verify_record 已拒绝 seq = 0 的记录；这里仍用饱和减法防止下溢
            self.state.last_seq = r.saturating_sub(1);
        }
        Ok(())
    }

    /// 对账（§4.4）：取所有在册设备状态记录的并集最大值，与本机已接受的版本比对。
    ///
    /// 必须存在状态记录的设备：写过本机已接受记录的在册设备、创建保险库的设备（仍在册时）、
    /// 加入时批准本机的设备（锚点，见 [`super::state::JoinAnchor`]）。已移除设备的状态记录不再参与比对。
    fn reconcile(&mut self) {
        let (Some(roster), Ok(me)) = (self.roster.clone(), self.me()) else { return };
        let mut problems = Vec::new();
        for (dev, entries) in &self.state.peers {
            if !roster.is_enrolled(dev) {
                continue;
            }
            for (rid, e) in entries {
                match self.state.records.get(rid) {
                    None if !e.deleted => problems.push(("hidden", format!("设备 {} 写过的记录在服务端缺失（疑似被隐藏）", short(dev)))),
                    Some(m) if m.counter < e.counter => problems.push(("withheld", format!("有记录落后于设备 {} 所见的版本（疑似被回滚或扣留）", short(dev)))),
                    Some(m) if m.counter == e.counter && m.hash != e.hash => {
                        problems.push(("fork", format!("有记录与设备 {} 所见的内容不同（疑似分叉）", short(dev))))
                    }
                    _ => {}
                }
            }
        }
        let mut required: BTreeSet<String> = self.state.records.values().map(|m| m.writer.clone()).collect();
        if let Some(g) = roster.genesis_device() {
            required.insert(g.to_string());
        }
        for w in &required {
            if *w != me && roster.is_enrolled(w) && !self.state.peers.contains_key(w) {
                problems.push(("state_missing", format!("设备 {} 的状态记录缺失（疑似被隐藏）", short(w))));
            }
        }
        if let Some(a) = self.state.join_anchor.clone() {
            if a.device == me || !roster.is_enrolled(&a.device) {
                self.state.join_anchor = None;
            } else {
                let rid = crypto::state_record_id(roster.vault_id(), &a.device);
                match self.state.records.get(&rid) {
                    None => problems.push(("state_missing", format!("设备 {} 的状态记录缺失（疑似被隐藏）", short(&a.device)))),
                    Some(m) if m.counter < a.counter || m.seq.unwrap_or(0) < a.seq => {
                        problems.push(("withheld", format!("设备 {} 的状态记录早于加入时的版本（疑似被回滚或扣留）", short(&a.device))))
                    }
                    Some(m) if m.counter == a.counter && m.hash != a.hash => {
                        problems.push(("fork", format!("设备 {} 的状态记录与加入时的版本不同（疑似分叉）", short(&a.device))))
                    }
                    _ => self.state.join_anchor = None,
                }
            }
        }
        for (code, msg) in problems {
            self.alert(code, msg);
        }
    }

    /// 重新拉取完整花名册与全部记录并逐条重新校验（L2）。本机已验证的花名册必须是服务端花名册的前缀；
    /// 只有这一轮校验没有再次触发停止写入时，才解除之前的停止写入。
    pub fn reverify(&mut self) -> Result<(), SyncError> {
        self.new_alerts = 0;
        let vault = self.vault()?;
        let me = self.me()?;
        let local = self.roster()?.clone();
        let (raws, length) = self.api.roster(&vault, 0)?;
        let consistent = length as usize == raws.len() && raws.len() >= local.raw().len() && raws[..local.raw().len()] == *local.raw();
        let fresh = Roster::from_entries(&vault, &raws).ok().filter(|_| consistent);
        let Some(fresh) = fresh else {
            self.alert("roster_invalid", "重新校验时服务端的花名册仍与本机已验证的不一致，继续停止写入");
            self.state.writes_blocked = Some(u32::MAX);
            self.save()?;
            return Err(security("花名册校验失败"));
        };
        self.set_roster(fresh);
        if !self.roster()?.is_enrolled(&me) {
            self.save()?;
            return Err(SyncError::NotEnrolled { message: "本机不在保险库的设备名单中".into() });
        }
        self.ensure_keys()?;
        let before = self.state.writes_blocked.take();
        self.state.needs_full = true;
        let mut report = SyncReport::default();
        if let Err(e) = self.pull(&mut report) {
            self.state.writes_blocked = before.max(self.state.writes_blocked);
            self.save()?;
            return Err(e);
        }
        self.reconcile();
        self.save()
    }

    /// 构造一条写入：counter 在本地已知最大值上加 1。
    pub fn build_write(&self, rid: &str, kind: Kind, deleted: bool, body: Body, base: Option<&RecordMeta>, key: &(u32, [u8; 32])) -> Result<Pending, SyncError> {
        let vault = self.vault()?;
        let (index, head) = self.roster()?.head().ok_or_else(|| security("花名册为空"))?;
        let counter = match base {
            None => 1,
            Some(m) => m.counter.checked_add(1).filter(|c| *c <= MAX_COUNTER).ok_or_else(|| security("记录的版本号已达上限"))?,
        };
        let keys = self.keys()?;
        let mut pt = Plaintext {
            kind,
            counter,
            deleted,
            writer: keys.device_id(),
            roster_index: index,
            roster_head: head,
            time: self.now().max(0) as u64,
            body,
            sig: [0; 64],
        };
        pt.sign(&vault, rid, key.0, |m| keys.roster_sign(m));
        let hash = pt.content_hash();
        let plain = Zeroizing::new(pt.encode());
        let ct = crypto::seal_record(&key.1, &crypto::random(), &vault, rid, key.0, &plain);
        let req = WriteReq {
            record_id: rid.to_string(),
            base_seq: base.and_then(|m| m.seq),
            epoch: key.0,
            ciphertext: codec::b64u(&ct),
            deleted,
            idempotency_key: codec::b64u(&crypto::random::<16>()),
        };
        Ok(Pending { req, pt, hash, file: None })
    }

    /// 发现本地模板目录的变化并推送（每批 ≤ 50 条）。
    fn push_local(&mut self, report: &mut SyncReport) -> Result<(), SyncError> {
        let Some(key) = self.write_key()? else {
            // 轮换进行中、新 VK 的信封还没到：本轮不写入
            return Ok(());
        };
        let dir = self.template_dir.clone();
        let mut pending = Vec::new();
        // 已映射的文件名（比较键，见 files::name_key）
        let mut mapped = BTreeSet::new();
        let metas: Vec<(String, RecordMeta)> = self.state.records.iter().filter(|(_, m)| m.kind == Kind::Template).map(|(k, m)| (k.clone(), m.clone())).collect();
        for (rid, meta) in &metas {
            if let Some(f) = &meta.file {
                mapped.insert(files::name_key(f));
            }
            // 版本号已达上限（只可能由恶意写入造成）：不再修改这条记录，避免整个同步失败
            if meta.counter >= MAX_COUNTER {
                self.alert("counter_exhausted", "有记录的版本号已达上限，本机不再修改它");
                continue;
            }
            let Some(f) = &meta.file else {
                if meta.dirty && meta.deleted {
                    pending.push(self.build_write(rid, Kind::Template, true, Body::Empty, Some(meta), &key)?);
                }
                continue;
            };
            match files::read(&dir, f) {
                None if !meta.deleted => {
                    let mut p = self.build_write(rid, Kind::Template, true, Body::Empty, Some(meta), &key)?;
                    p.file = None;
                    pending.push(p);
                }
                None => {}
                Some(bytes) => {
                    let h = files::hash(&bytes);
                    if Some(&h) != meta.file_hash.as_ref() || meta.dirty {
                        let Ok(source) = String::from_utf8(bytes) else {
                            self.alert("file_encoding", format!("{f} 不是 UTF-8 文本，未同步"));
                            continue;
                        };
                        let body = Body::Template { file_name: f.clone(), source };
                        let mut p = self.build_write(rid, Kind::Template, false, body, Some(meta), &key)?;
                        p.file = Some((f.clone(), h));
                        pending.push(p);
                    }
                }
            }
        }
        for name in files::list(&dir) {
            if !mapped.insert(files::name_key(&name)) {
                continue;
            }
            let Some(bytes) = files::read(&dir, &name) else { continue };
            let h = files::hash(&bytes);
            let Ok(source) = String::from_utf8(bytes) else {
                self.alert("file_encoding", format!("{name} 不是 UTF-8 文本，未同步"));
                continue;
            };
            let rid = crypto::random_record_id();
            let mut p = self.build_write(&rid, Kind::Template, false, Body::Template { file_name: name.clone(), source }, None, &key)?;
            p.file = Some((name, h));
            pending.push(p);
        }
        self.push(pending, report).map(|_| ())
    }

    /// 推送并处理结果：成功、冲突（§5.5）、gone（转全量对账）、epoch 过期、配额。
    /// 返回成功写入的条数。
    pub fn push(&mut self, pending: Vec<Pending>, report: &mut SyncReport) -> Result<usize, SyncError> {
        let vault = self.vault()?;
        let mut ok = 0;
        let mut queue = pending.into_iter().peekable();
        while queue.peek().is_some() {
            let batch: Vec<Pending> = queue.by_ref().take(BATCH).collect();
            let reqs: Vec<WriteReq> = batch.iter().map(|p| p.req.clone()).collect();
            let results = self.api.push(&vault, &reqs)?;
            for (p, r) in batch.into_iter().zip(results) {
                ok += usize::from(self.handle_result(p, r, report)?);
            }
            self.save()?;
        }
        Ok(ok)
    }

    fn handle_result(&mut self, p: Pending, r: WriteResult, report: &mut SyncReport) -> Result<bool, SyncError> {
        let rid = p.req.record_id.clone();
        match r.status.as_str() {
            "ok" => {
                let seq = r.seq.ok_or_else(|| security("服务端未返回 seq"))?;
                let file = if p.pt.kind == Kind::Template { Some(p.file.clone()) } else { None };
                self.ack(&rid, &p.pt, &p.hash, seq, p.req.epoch, file);
                report.pushed += 1;
                return Ok(true);
            }
            "conflict" => {
                let current = r.current.ok_or_else(|| security("服务端未返回冲突版本"))?;
                if current.record_id != rid {
                    return Err(security("服务端返回的冲突版本与请求不符"));
                }
                match self.verify_record(&current)? {
                    Verdict::Accept(acc) => {
                        let same = acc.pt.deleted == p.pt.deleted && acc.pt.body == p.pt.body;
                        if same && p.pt.kind == Kind::Template {
                            // 内容相同：直接采用远端版本
                            self.ack(&rid, &acc.pt, &acc.hash, acc.wire.seq, acc.wire.epoch, Some(p.file.clone()));
                        } else {
                            self.apply(&acc, report)?;
                        }
                    }
                    Verdict::Same(acc) => {
                        if let Some(m) = self.state.records.get_mut(&rid) {
                            m.seq = Some(acc.wire.seq);
                        }
                    }
                    Verdict::Reject | Verdict::Retry => report.rejected += 1,
                }
            }
            "gone" => {
                if let Some(m) = self.state.records.get_mut(&rid) {
                    m.seq = None;
                    m.dirty = true;
                }
                self.state.needs_full = true;
            }
            "stale_epoch" => report.deferred += 1,
            _ => self.alert("push_rejected", "服务端拒绝了本机的写入"),
        }
        Ok(false)
    }

    /// 更新本机的设备状态记录（§4.4）：每条已同步记录的 (max_counter, content_hash, deleted)。
    pub fn write_own_state(&mut self, report: &mut SyncReport) -> Result<(), SyncError> {
        let Some(key) = self.write_key()? else { return Ok(()) };
        let vault = self.vault()?;
        let me = self.me()?;
        let rid = crypto::state_record_id(&vault, &me);
        let entries: BTreeMap<String, StateEntry> = self
            .state
            .records
            .iter()
            .filter(|(_, m)| m.kind != Kind::DeviceState && m.seq.is_some())
            .filter_map(|(id, m)| {
                let hash: [u8; 32] = codec::from_b64u(&m.hash).ok()?.try_into().ok()?;
                Some((id.clone(), StateEntry { counter: m.counter, hash, deleted: m.deleted }))
            })
            .collect();
        let body = Body::DeviceState { entries };
        let digest = codec::b64u(&crypto::sha256(&[&codec::encode(&body.encode())]));
        for _ in 0..3 {
            let meta = self.state.records.get(&rid).cloned();
            let current = meta.as_ref().is_some_and(|m| m.seq.is_some() && !m.dirty && m.epoch == key.0);
            if current && self.state.own_state_digest.as_deref() == Some(&digest) {
                return Ok(());
            }
            let p = self.build_write(&rid, Kind::DeviceState, false, body.clone(), meta.as_ref(), &key)?;
            // 冲突时 push 会更新本地已知的 seq 与 counter，下一轮在其基础上重试
            if self.push(vec![p], report)? == 1 {
                self.state.own_state_digest = Some(digest);
                return Ok(());
            }
        }
        self.alert("state_write_failed", "无法更新本机的状态记录");
        Ok(())
    }
}
