//! 撤销设备与密钥轮换（docs/sync-design.md 第 7 节）。
//!
//! 1. CAS 追加 `remove_device(D)`；2. 生成新 VK，CAS 追加 `rotate(e+1, vk_commit)`，成功后才继续；
//! 3. 为剩余设备与恢复公钥写信封（钥匙环含新旧 VK）；4. 用新 VK 重新加密所有记录，counter + 1；
//! 5. 追加 `rotate_done(e+1)`，上报 min-epoch，丢弃旧 VK。中断后由发起设备在下次同步时继续；
//!    `rotate` 之后 24 小时没有 `rotate_done`，任何在册设备都可以发起下一次轮换接管。

use std::collections::BTreeSet;

use zeroize::Zeroizing;

use super::crypto::{self, RecoveryKeys};
use super::engine::{Core, Verdict};
use super::record::Kind;
use super::roster::{Entry, Op, Signer};
use super::{SyncError, SyncReport};

const TAKEOVER_SECS: i64 = 24 * 60 * 60;

fn security(message: impl Into<String>) -> SyncError {
    SyncError::Security { message: message.into() }
}

pub fn revoke_device(core: &mut Core, device_id: &str, recovery_phrase: Option<&str>) -> Result<SyncReport, SyncError> {
    let me = core.require_enrolled()?;
    core.refresh_roster()?;
    if device_id == me {
        return Err(SyncError::InvalidInput { message: "不能在本机上移除本机".into() });
    }
    if !core.roster()?.is_enrolled(device_id) {
        return Err(SyncError::InvalidInput { message: "该设备不在册".into() });
    }
    let vault = core.vault()?;
    let keys = core.keys()?;
    let rk = recovery_phrase.map(RecoveryKeys::from_phrase).transpose().map_err(|message| SyncError::InvalidInput { message })?;
    if let Some(rk) = &rk {
        let rec = core.roster()?.recovery().ok_or_else(|| security("花名册中没有恢复密钥"))?;
        if rec.ed25519 != rk.s_public() {
            return Err(SyncError::InvalidInput { message: "恢复短语与此保险库不匹配".into() });
        }
    }
    let target = device_id.to_string();
    core.append_roster(|r| {
        if !r.is_enrolled(&target) {
            return Ok(None);
        }
        let others: Vec<String> = r.current_devices().into_iter().map(|(id, _)| id).filter(|id| *id != target).collect();
        // 移除除本机外的最后一台设备需要恢复短语签名（§6.4）
        let need_recovery = others == [me.clone()];
        let (idx, prev) = r.head().unwrap();
        let op = Op::RemoveDevice { device_id: target.clone() };
        let entry = match (&rk, need_recovery) {
            (Some(rk), _) => Entry::signed(&vault, idx + 1, prev, op, Signer::Recovery, |m| rk.sign(m)),
            (None, true) => return Err(SyncError::RecoveryRequired { message: "移除最后一台其他设备需要输入恢复短语".into() }),
            (None, false) => Entry::signed(&vault, idx + 1, prev, op, Signer::Device(me.clone()), |m| keys.roster_sign(m)),
        };
        Ok(Some(entry.encode()))
    })?;
    // 服务端没有让一台设备吊销另一台设备令牌的接口，只能由用户在官网"设备"页吊销访问权
    let mut report = SyncReport::default();
    rotate(core, &mut report)?;
    Ok(report)
}

/// 用户手动轮换（不撤销设备）：他人的轮换进行中且未超过 24 小时时拒绝。
pub fn rotate_keys(core: &mut Core) -> Result<SyncReport, SyncError> {
    let me = core.require_enrolled()?;
    core.refresh_roster()?;
    let mut report = SyncReport::default();
    if let Some((epoch, info)) = core.roster()?.pending_rotation() {
        if info.signer == me {
            finish_rotation(core, epoch, &mut report)?;
            return Ok(report);
        }
        let seen = core.state.pending_rotation_seen.filter(|(e, _)| *e == epoch).map(|(_, t)| t).unwrap_or(core.now());
        if core.now() - seen < TAKEOVER_SECS {
            return Err(SyncError::InvalidState { message: "其他设备正在轮换密钥，请稍后再试".into() });
        }
    }
    rotate(core, &mut report)?;
    Ok(report)
}

/// 同步开始时调用：继续本机未完成的轮换；他人的轮换超过 24 小时未完成时接管。
pub fn handle_pending(core: &mut Core, report: &mut SyncReport) -> Result<(), SyncError> {
    let me = core.me()?;
    let Some((epoch, info)) = core.roster()?.pending_rotation().map(|(e, i)| (e, i.clone())) else {
        core.state.pending_rotation_seen = None;
        return Ok(());
    };
    if info.signer == me {
        return finish_rotation(core, epoch, report);
    }
    let now = core.now();
    match core.state.pending_rotation_seen {
        Some((e, t)) if e == epoch => {
            if now - t >= TAKEOVER_SECS {
                core.alert("rotation_takeover", format!("设备 {} 发起的密钥轮换超过 24 小时未完成，本机已接管", &info.signer[..6]));
                rotate(core, report)?;
            }
        }
        _ => core.state.pending_rotation_seen = Some((epoch, now)),
    }
    Ok(())
}

/// 生成新 VK 并 CAS 追加 rotate，然后完成轮换。
fn rotate(core: &mut Core, report: &mut SyncReport) -> Result<(), SyncError> {
    let vault = core.vault()?;
    let keys = core.keys()?;
    let me = keys.device_id();
    core.ensure_keys()?;
    let mut new_epoch = 0;
    for _ in 0..5 {
        let roster = core.roster()?.clone();
        let epoch = roster.max_epoch() + 1;
        let vk: Zeroizing<[u8; 32]> = Zeroizing::new(crypto::random());
        // 先把新 VK 存入安全存储，再追加花名册：避免追加成功后崩溃导致新 VK 丢失
        let mut ring = core.keyring()?;
        ring.insert(epoch, *vk);
        core.store_keyring(&ring)?;
        let (idx, prev) = roster.head().unwrap();
        let entry = Entry::signed(
            &vault,
            idx + 1,
            prev,
            Op::Rotate { epoch, vk_commit: crypto::vk_commit(&vk, &vault, epoch) },
            Signer::Device(me.clone()),
            |m| keys.roster_sign(m),
        );
        let raw = entry.encode();
        let next = roster.extended(std::slice::from_ref(&raw)).map_err(|e| security(format!("本机构造的花名册条目无效：{e}")))?;
        if core.api.append_roster(&vault, roster.len(), &raw)? {
            core.set_roster(next);
            core.save()?;
            new_epoch = epoch;
            break;
        }
        // 并发操作：撤回本地的新 VK，刷新花名册后重来
        ring.remove(epoch);
        core.store_keyring(&ring)?;
        core.refresh_roster()?;
    }
    if new_epoch == 0 {
        return Err(SyncError::Conflict { message: "花名册被其他设备频繁更新，轮换未能开始".into() });
    }
    finish_rotation(core, new_epoch, report)
}

/// 轮换的第 3–5 步（可重复执行）。
fn finish_rotation(core: &mut Core, epoch: u32, report: &mut SyncReport) -> Result<(), SyncError> {
    let vault = core.vault()?;
    let keys = core.keys()?;
    let me = keys.device_id();
    let ring = core.keyring()?;
    if ring.get(epoch).is_none() {
        return Err(security("本机缺少新密钥，无法完成轮换"));
    }
    // 3. 为剩余每台设备与恢复公钥写信封
    let roster = core.roster()?.clone();
    for (id, m) in roster.current_devices() {
        core.write_envelope(&ring, &id, &m.keys.x25519)?;
    }
    if let Some(rec) = roster.recovery() {
        core.write_envelope(&ring, crypto::RECOVERY_RECIPIENT, &rec.x25519)?;
    }
    // 4. 重新加密
    reencrypt_all(core, epoch, report)?;
    // 5. rotate_done：只有该轮换仍是本机发起的、进行中的轮换才追加
    let done = core.append_roster(|r| {
        match r.pending_rotation() {
            Some((e, info)) if e == epoch && info.signer == me => {}
            _ => return Ok(None),
        }
        let (idx, prev) = r.head().unwrap();
        Ok(Some(Entry::signed(&vault, idx + 1, prev, Op::RotateDone { epoch }, Signer::Device(me.clone()), |m| keys.roster_sign(m)).encode()))
    })?;
    if done || core.roster()?.min_epoch() >= epoch {
        core.api.raise_min_epoch(&vault, epoch)?;
        let mut ring = core.keyring()?;
        ring.retain_from(core.roster()?.min_epoch());
        core.store_keyring(&ring)?;
    }
    core.save()
}

/// 把旧 epoch 的记录用新 VK 重新加密，counter + 1（§7 第 4 步）。其他设备的状态记录由它们自己重写。
fn reencrypt_all(core: &mut Core, epoch: u32, report: &mut SyncReport) -> Result<(), SyncError> {
    let me = core.me()?;
    let key = (epoch, *core.keyring()?.get(epoch).ok_or_else(|| security("本机缺少新密钥"))?);
    let mut skipped = BTreeSet::new();
    for _ in 0..5 {
        let targets: Vec<_> = core.list_all()?.into_iter().filter(|w| w.epoch < epoch && !skipped.contains(&w.record_id)).collect();
        if targets.is_empty() {
            return Ok(());
        }
        let mut pending = Vec::new();
        for w in targets {
            let acc = match core.verify_record(&w)? {
                Verdict::Accept(acc) => {
                    core.apply(&acc, report)?;
                    acc
                }
                Verdict::Same(acc) => acc,
                Verdict::Reject | Verdict::Retry => {
                    skipped.insert(w.record_id.clone());
                    continue;
                }
            };
            if acc.pt.kind == Kind::DeviceState && acc.pt.writer != me {
                skipped.insert(w.record_id.clone());
                continue;
            }
            let meta = core.state.records.get(&w.record_id).cloned();
            let mut p = core.build_write(&w.record_id, acc.pt.kind, acc.pt.deleted, acc.pt.body.clone(), meta.as_ref(), &key)?;
            p.req.base_seq = Some(w.seq);
            // 模板文件映射保持不变（内容未变）
            p.file = meta.and_then(|m| m.file.zip(m.file_hash));
            pending.push(p);
        }
        core.push(pending, report)?;
    }
    Err(SyncError::Conflict { message: "重新加密时多次遇到并发修改，请稍后重试".into() })
}
