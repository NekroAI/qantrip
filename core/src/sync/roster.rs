//! 签名花名册（docs/sync-design.md 6.1、6.4、第 7 节）：只追加的签名日志，决定哪些公钥有权收到 VK。
//!
//! 条目编码（确定性 CBOR 映射）：
//! `{ index, prev, op, subject, signer, sig }`，
//! `sig = Ed25519(signer 的花名册密钥, "qantrip/v1/roster" || vault_id || 规范编码(条目除 sig))`，
//! `prev = SHA-256(上一条完整编码)`，第 0 条为 32 字节零。
//!
//! 各 op 的 subject：
//! - `genesis`：`{ x25519, roster, auth, epoch: 1, vk_commit }`
//! - `set_recovery`：`{ x25519, ed25519, bind }`
//! - `add_device`：`{ x25519, roster, auth }`
//! - `remove_device`：`{ device_id }`
//! - `rotate`：`{ epoch, vk_commit }`
//! - `rotate_done`：`{ epoch }`
//!
//! `set_recovery.bind` 是设计之外的补充：恢复 Ed25519 私钥对 `"qantrip/v1/recovery-bind" || vault_id || prev`
//! 的签名。用恢复短语加入的设备没有任何本地状态，若没有这个绑定，服务端可以伪造一整条以自己设备为 genesis、
//! 但带有用户真实恢复公钥的花名册，再用自选 VK 写恢复信封；有了绑定，伪造花名册必须持有恢复私钥。

use std::collections::BTreeMap;

use ciborium::Value;

use super::codec::{self, Fields};
use super::crypto::{self, PublicKeys};

pub const ZERO_HASH: [u8; 32] = [0; 32];
pub const RECOVERY_SIGNER: &str = "recovery";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signer {
    Device(String),
    Recovery,
}

impl Signer {
    pub fn as_str(&self) -> &str {
        match self {
            Signer::Device(id) => id,
            Signer::Recovery => RECOVERY_SIGNER,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Genesis { device: PublicKeys, vk_commit: [u8; 32] },
    SetRecovery { x25519: [u8; 32], ed25519: [u8; 32], bind: [u8; 64] },
    AddDevice { device: PublicKeys },
    RemoveDevice { device_id: String },
    Rotate { epoch: u32, vk_commit: [u8; 32] },
    RotateDone { epoch: u32 },
}

impl Op {
    fn name(&self) -> &'static str {
        match self {
            Op::Genesis { .. } => "genesis",
            Op::SetRecovery { .. } => "set_recovery",
            Op::AddDevice { .. } => "add_device",
            Op::RemoveDevice { .. } => "remove_device",
            Op::Rotate { .. } => "rotate",
            Op::RotateDone { .. } => "rotate_done",
        }
    }

    fn subject(&self) -> Value {
        use codec::{bytes, map, text, uint};
        let keys = |d: &PublicKeys| vec![("x25519", bytes(&d.x25519)), ("roster", bytes(&d.roster)), ("auth", bytes(&d.auth))];
        match self {
            Op::Genesis { device, vk_commit } => {
                let mut f = keys(device);
                f.push(("epoch", uint(1)));
                f.push(("vk_commit", bytes(vk_commit)));
                map(f)
            }
            Op::SetRecovery { x25519, ed25519, bind } => map(vec![("x25519", bytes(x25519)), ("ed25519", bytes(ed25519)), ("bind", bytes(bind))]),
            Op::AddDevice { device } => map(keys(device)),
            Op::RemoveDevice { device_id } => map(vec![("device_id", text(device_id))]),
            Op::Rotate { epoch, vk_commit } => map(vec![("epoch", uint(*epoch as u64)), ("vk_commit", bytes(vk_commit))]),
            Op::RotateDone { epoch } => map(vec![("epoch", uint(*epoch as u64))]),
        }
    }

    fn parse(name: &str, subject: Value) -> Result<Op, String> {
        let device = |f: &mut Fields| -> Result<PublicKeys, String> {
            Ok(PublicKeys { x25519: f.array("x25519")?, roster: f.array("roster")?, auth: f.array("auth")? })
        };
        Ok(match name {
            "genesis" => {
                let mut f = Fields::exact(subject, &["x25519", "roster", "auth", "epoch", "vk_commit"])?;
                if f.u32("epoch")? != 1 {
                    return Err("genesis 的 epoch 必须为 1".into());
                }
                Op::Genesis { device: device(&mut f)?, vk_commit: f.array("vk_commit")? }
            }
            "set_recovery" => {
                let mut f = Fields::exact(subject, &["x25519", "ed25519", "bind"])?;
                Op::SetRecovery { x25519: f.array("x25519")?, ed25519: f.array("ed25519")?, bind: f.array("bind")? }
            }
            "add_device" => Op::AddDevice { device: device(&mut Fields::exact(subject, &["x25519", "roster", "auth"])?)? },
            "remove_device" => Op::RemoveDevice { device_id: Fields::exact(subject, &["device_id"])?.text("device_id")? },
            "rotate" => {
                let mut f = Fields::exact(subject, &["epoch", "vk_commit"])?;
                Op::Rotate { epoch: f.u32("epoch")?, vk_commit: f.array("vk_commit")? }
            }
            "rotate_done" => Op::RotateDone { epoch: Fields::exact(subject, &["epoch"])?.u32("epoch")? },
            _ => return Err("未知的花名册操作".into()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub index: u32,
    pub prev: [u8; 32],
    pub op: Op,
    pub signer: Signer,
    pub sig: [u8; 64],
}

/// 恢复公钥绑定签名的消息：`"qantrip/v1/recovery-bind" || vault_id || prev`
pub fn recovery_bind_message(vault_id: &str, prev: &[u8; 32]) -> Vec<u8> {
    [crypto::D_RECOVERY_BIND, vault_id.as_bytes(), prev].concat()
}

impl Entry {
    fn fields(&self) -> Vec<(&'static str, Value)> {
        use codec::{bytes, text, uint};
        vec![
            ("index", uint(self.index as u64)),
            ("prev", bytes(&self.prev)),
            ("op", text(self.op.name())),
            ("subject", self.op.subject()),
            ("signer", text(self.signer.as_str())),
        ]
    }

    /// 被签名的消息：`"qantrip/v1/roster" || vault_id || 规范编码(条目除 sig)`
    pub fn signing_message(&self, vault_id: &str) -> Vec<u8> {
        [crypto::D_ROSTER, vault_id.as_bytes(), &codec::encode(&codec::map(self.fields()))].concat()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut f = self.fields();
        f.push(("sig", codec::bytes(&self.sig)));
        codec::encode(&codec::map(f))
    }

    pub fn hash(&self) -> [u8; 32] {
        crypto::sha256(&[&self.encode()])
    }

    pub fn decode(raw: &[u8]) -> Result<Entry, String> {
        let mut f = Fields::exact(codec::decode(raw)?, &["index", "prev", "op", "subject", "signer", "sig"])?;
        let op_name = f.text("op")?;
        let signer = match f.text("signer")?.as_str() {
            RECOVERY_SIGNER => Signer::Recovery,
            id => Signer::Device(id.to_string()),
        };
        Ok(Entry {
            index: f.u32("index")?,
            prev: f.array("prev")?,
            op: Op::parse(&op_name, f.value("subject"))?,
            signer,
            sig: f.array("sig")?,
        })
    }

    /// 构造并签名。`sign` 接收待签消息，返回签名（设备花名册密钥或恢复密钥）。
    pub fn signed(vault_id: &str, index: u32, prev: [u8; 32], op: Op, signer: Signer, sign: impl FnOnce(&[u8]) -> [u8; 64]) -> Entry {
        let mut e = Entry { index, prev, op, signer, sig: [0; 64] };
        e.sig = sign(&e.signing_message(vault_id));
        e
    }
}

/// 一台出现过的设备。
#[derive(Debug, Clone)]
pub struct Member {
    pub keys: PublicKeys,
    pub added: u32,
    pub removed: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct EpochInfo {
    pub vk_commit: [u8; 32],
    pub index: u32,
    pub signer: String,
}

#[derive(Debug, Clone)]
pub struct RecoveryInfo {
    pub index: u32,
    pub x25519: [u8; 32],
    pub ed25519: [u8; 32],
}

/// 已验证的花名册及由它推出的状态。
#[derive(Debug, Clone)]
pub struct Roster {
    vault_id: String,
    raw: Vec<Vec<u8>>,
    entries: Vec<Entry>,
    hashes: Vec<[u8; 32]>,
    members: BTreeMap<String, Member>,
    recovery: Vec<RecoveryInfo>,
    epochs: BTreeMap<u32, EpochInfo>,
    /// 最近一次 rotate_done 的 epoch（未轮换过为 1）：拒收更早 epoch 的记录
    done: u32,
}

impl Roster {
    pub fn new(vault_id: &str) -> Roster {
        Roster {
            vault_id: vault_id.to_string(),
            raw: Vec::new(),
            entries: Vec::new(),
            hashes: Vec::new(),
            members: BTreeMap::new(),
            recovery: Vec::new(),
            epochs: BTreeMap::new(),
            done: 1,
        }
    }

    /// 从 genesis 开始逐条验证。
    pub fn from_entries(vault_id: &str, raws: &[Vec<u8>]) -> Result<Roster, String> {
        let mut r = Roster::new(vault_id);
        for raw in raws {
            r.append(raw)?;
        }
        Ok(r)
    }

    /// 在已验证的花名册之后追加（全部通过才返回新花名册，失败时原花名册不变）。
    pub fn extended(&self, raws: &[Vec<u8>]) -> Result<Roster, String> {
        let mut r = self.clone();
        for raw in raws {
            r.append(raw)?;
        }
        Ok(r)
    }

    pub fn vault_id(&self) -> &str {
        &self.vault_id
    }

    pub fn len(&self) -> u32 {
        self.entries.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn raw(&self) -> &[Vec<u8>] {
        &self.raw
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// 头部：最后一条的 index 与哈希。
    pub fn head(&self) -> Option<(u32, [u8; 32])> {
        self.hashes.last().map(|h| (self.len() - 1, *h))
    }

    pub fn hash_at(&self, index: u32) -> Option<[u8; 32]> {
        self.hashes.get(index as usize).copied()
    }

    pub fn member(&self, device_id: &str) -> Option<&Member> {
        self.members.get(device_id)
    }

    pub fn members(&self) -> impl Iterator<Item = (&String, &Member)> {
        self.members.iter()
    }

    /// 设备在第 `index` 条条目生效后是否在册。
    pub fn enrolled_at(&self, device_id: &str, index: u32) -> Option<&PublicKeys> {
        let m = self.members.get(device_id)?;
        (m.added <= index && m.removed.is_none_or(|r| r > index) && index < self.len()).then_some(&m.keys)
    }

    pub fn is_enrolled(&self, device_id: &str) -> bool {
        self.members.get(device_id).is_some_and(|m| m.removed.is_none())
    }

    pub fn current_devices(&self) -> Vec<(String, Member)> {
        self.members.iter().filter(|(_, m)| m.removed.is_none()).map(|(k, m)| (k.clone(), m.clone())).collect()
    }

    /// 创建保险库的设备（genesis 的签名者）。
    pub fn genesis_device(&self) -> Option<&str> {
        self.epochs.get(&1).map(|e| e.signer.as_str())
    }

    pub fn recovery(&self) -> Option<&RecoveryInfo> {
        self.recovery.last()
    }

    pub fn max_epoch(&self) -> u32 {
        self.epochs.keys().next_back().copied().unwrap_or(0)
    }

    pub fn min_epoch(&self) -> u32 {
        self.done
    }

    pub fn epoch(&self, epoch: u32) -> Option<&EpochInfo> {
        self.epochs.get(&epoch)
    }

    /// 尚未完成的轮换：最新 rotate 的 epoch 大于最近的 rotate_done。
    pub fn pending_rotation(&self) -> Option<(u32, &EpochInfo)> {
        let max = self.max_epoch();
        (max > self.done).then(|| (max, &self.epochs[&max]))
    }

    fn signer_key(&self, signer: &Signer) -> Result<[u8; 32], String> {
        match signer {
            Signer::Device(id) => self
                .members
                .get(id)
                .filter(|m| m.removed.is_none())
                .map(|m| m.keys.roster)
                .ok_or_else(|| "签名者不在册".to_string()),
            Signer::Recovery => self.recovery().map(|r| r.ed25519).ok_or_else(|| "尚未设置恢复密钥".to_string()),
        }
    }

    fn require_device(signer: &Signer, what: &str) -> Result<(), String> {
        match signer {
            Signer::Device(_) => Ok(()),
            Signer::Recovery => Err(format!("{what} 不能由恢复密钥签名")),
        }
    }

    /// 验证并追加一条：链接、签名与各 op 的签名者规则。
    pub fn append(&mut self, raw: &[u8]) -> Result<(), String> {
        let e = Entry::decode(raw)?;
        if e.encode() != raw {
            return Err("花名册条目不是规范编码".into());
        }
        let index = self.len();
        if e.index != index {
            return Err(format!("花名册条目序号不连续（期望 {index}，实际 {}）", e.index));
        }
        let prev = self.hashes.last().copied().unwrap_or(ZERO_HASH);
        if e.prev != prev {
            return Err(format!("花名册第 {index} 条与前一条不衔接"));
        }
        let msg = e.signing_message(&self.vault_id);

        match &e.op {
            Op::Genesis { device, vk_commit } => {
                if index != 0 {
                    return Err("genesis 只能是第 0 条".into());
                }
                let id = device.device_id();
                if e.signer != Signer::Device(id.clone()) {
                    return Err("genesis 必须由该设备自己签名".into());
                }
                crypto::verify(&device.roster, &msg, &e.sig)?;
                self.members.insert(id.clone(), Member { keys: device.clone(), added: 0, removed: None });
                self.epochs.insert(1, EpochInfo { vk_commit: *vk_commit, index: 0, signer: id });
            }
            _ if index == 0 => return Err("第 0 条必须是 genesis".into()),
            Op::SetRecovery { x25519, ed25519, bind } => {
                match (&e.signer, self.recovery()) {
                    // 创建时由第一台设备设置；之后只能由（旧的）恢复密钥签名（§6.4）
                    (Signer::Device(id), None) if index == 1 && self.epochs[&1].signer == *id => {}
                    (Signer::Recovery, Some(_)) => {}
                    _ => return Err("set_recovery 的签名者不符合规则".into()),
                }
                crypto::verify(&self.signer_key(&e.signer)?, &msg, &e.sig)?;
                crypto::verify(ed25519, &recovery_bind_message(&self.vault_id, &e.prev), bind).map_err(|_| "恢复公钥绑定签名无效".to_string())?;
                self.recovery.push(RecoveryInfo { index, x25519: *x25519, ed25519: *ed25519 });
            }
            Op::AddDevice { device } => {
                crypto::verify(&self.signer_key(&e.signer)?, &msg, &e.sig)?;
                let id = device.device_id();
                if self.members.contains_key(&id) {
                    return Err("该设备已在花名册中出现过".into());
                }
                self.members.insert(id, Member { keys: device.clone(), added: index, removed: None });
            }
            Op::RemoveDevice { device_id } => {
                crypto::verify(&self.signer_key(&e.signer)?, &msg, &e.sig)?;
                if !self.is_enrolled(device_id) {
                    return Err("被移除的设备不在册".into());
                }
                if let Signer::Device(signer) = &e.signer {
                    let remaining: Vec<_> = self.current_devices().into_iter().map(|(id, _)| id).filter(|id| id != device_id).collect();
                    // 移除除签名者外的最后一台设备只能由恢复密钥签名（§6.4）
                    if signer != device_id && remaining == [signer.clone()] {
                        return Err("移除最后一台其他设备需要恢复短语".into());
                    }
                }
                self.members.get_mut(device_id).unwrap().removed = Some(index);
            }
            Op::Rotate { epoch, vk_commit } => {
                Self::require_device(&e.signer, "rotate")?;
                crypto::verify(&self.signer_key(&e.signer)?, &msg, &e.sig)?;
                if *epoch != self.max_epoch() + 1 {
                    return Err("rotate 的 epoch 必须递增 1".into());
                }
                self.epochs.insert(*epoch, EpochInfo { vk_commit: *vk_commit, index, signer: e.signer.as_str().to_string() });
            }
            Op::RotateDone { epoch } => {
                Self::require_device(&e.signer, "rotate_done")?;
                crypto::verify(&self.signer_key(&e.signer)?, &msg, &e.sig)?;
                let Some((pending, info)) = self.pending_rotation() else { return Err("没有进行中的轮换".into()) };
                if *epoch != pending {
                    return Err("rotate_done 的 epoch 与进行中的轮换不符".into());
                }
                // 只能由发起该轮换的设备完成；超时接管通过发起下一次轮换实现（§7）
                if info.signer != e.signer.as_str() {
                    return Err("rotate_done 必须由发起轮换的设备签名".into());
                }
                self.done = *epoch;
            }
        }

        self.hashes.push(crypto::sha256(&[raw]));
        self.raw.push(raw.to_vec());
        self.entries.push(e);
        Ok(())
    }
}
