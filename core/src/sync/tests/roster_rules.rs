//! 花名册：编码、哈希链、签名严格校验与各 op 的签名者规则（§6.1、§6.4、§7）。

use crate::sync::crypto::{self, DeviceKeys, RecoveryKeys};
use crate::sync::roster::{Entry, Op, Roster, Signer, ZERO_HASH, recovery_bind_message};

const VAULT: &str = "v_rosterrules";

fn dev(n: u8) -> DeviceKeys {
    DeviceKeys::from_seeds([n; 32], [n + 100; 32], [n + 200; 32])
}

struct Builder {
    roster: Roster,
}

impl Builder {
    fn new(first: &DeviceKeys, rk: &RecoveryKeys) -> Builder {
        let g = Entry::signed(VAULT, 0, ZERO_HASH, Op::Genesis { device: first.public(), vk_commit: [1; 32] }, Signer::Device(first.device_id()), |m| first.roster_sign(m));
        let bind = rk.sign(&recovery_bind_message(VAULT, &g.hash()));
        let s = Entry::signed(VAULT, 1, g.hash(), Op::SetRecovery { x25519: rk.x_public(), ed25519: rk.s_public(), bind }, Signer::Device(first.device_id()), |m| first.roster_sign(m));
        Builder { roster: Roster::from_entries(VAULT, &[g.encode(), s.encode()]).unwrap() }
    }

    fn entry(&self, op: Op, signer: Signer, sign: impl FnOnce(&[u8]) -> [u8; 64]) -> Vec<u8> {
        let (idx, prev) = self.roster.head().unwrap();
        Entry::signed(VAULT, idx + 1, prev, op, signer, sign).encode()
    }

    fn by_dev(&self, op: Op, d: &DeviceKeys) -> Vec<u8> {
        self.entry(op, Signer::Device(d.device_id()), |m| d.roster_sign(m))
    }

    fn by_recovery(&self, op: Op, rk: &RecoveryKeys) -> Vec<u8> {
        self.entry(op, Signer::Recovery, |m| rk.sign(m))
    }

    fn ok(&mut self, raw: Vec<u8>) {
        self.roster.append(&raw).unwrap();
    }

    fn err(&self, raw: Vec<u8>) -> String {
        self.roster.clone().append(&raw).unwrap_err()
    }
}

#[test]
fn signer_rules() {
    let (a, b, c, x) = (dev(1), dev(2), dev(3), dev(4));
    let rk = RecoveryKeys::from_entropy(&[7; 32]);
    let rk2 = RecoveryKeys::from_entropy(&[8; 32]);
    let mut r = Builder::new(&a, &rk);
    assert!(r.roster.is_enrolled(&a.device_id()));

    // add_device：在册设备或恢复密钥；不在册设备不行
    r.err(r.by_dev(Op::AddDevice { device: c.public() }, &x));
    r.ok(r.by_dev(Op::AddDevice { device: b.public() }, &a));
    r.ok(r.by_recovery(Op::AddDevice { device: c.public() }, &rk));
    // 同一设备不能重复加入
    assert!(r.err(r.by_dev(Op::AddDevice { device: b.public() }, &a)).contains("出现过"));

    // set_recovery：创建之后只能由（旧）恢复密钥签名（§6.4）
    let new_rec = |r: &Builder, rk_new: &RecoveryKeys| {
        let (_, prev) = r.roster.head().unwrap();
        Op::SetRecovery { x25519: rk_new.x_public(), ed25519: rk_new.s_public(), bind: rk_new.sign(&recovery_bind_message(VAULT, &prev)) }
    };
    r.err(r.by_dev(new_rec(&r, &rk2), &a));
    r.err(r.by_recovery(new_rec(&r, &rk2), &rk2));
    // 绑定签名必须覆盖 prev
    let bad_bind = Op::SetRecovery { x25519: rk2.x_public(), ed25519: rk2.s_public(), bind: rk2.sign(b"other") };
    r.err(r.by_recovery(bad_bind, &rk));
    r.ok(r.by_recovery(new_rec(&r, &rk2), &rk));
    // 旧恢复密钥失效
    r.err(r.by_recovery(Op::AddDevice { device: x.public() }, &rk));

    // rotate：epoch 必须递增 1，只能由在册设备签名
    r.err(r.by_dev(Op::Rotate { epoch: 3, vk_commit: [2; 32] }, &b));
    r.err(r.by_recovery(Op::Rotate { epoch: 2, vk_commit: [2; 32] }, &rk2));
    r.ok(r.by_dev(Op::Rotate { epoch: 2, vk_commit: [2; 32] }, &b));
    assert_eq!(r.roster.pending_rotation().unwrap().0, 2);
    // rotate_done：只能由发起者完成
    r.err(r.by_dev(Op::RotateDone { epoch: 2 }, &a));
    r.err(r.by_dev(Op::RotateDone { epoch: 1 }, &b));
    // 接管：另一台设备发起 e+1 并完成
    r.ok(r.by_dev(Op::Rotate { epoch: 3, vk_commit: [3; 32] }, &a));
    r.err(r.by_dev(Op::RotateDone { epoch: 2 }, &b));
    r.ok(r.by_dev(Op::RotateDone { epoch: 3 }, &a));
    assert_eq!((r.roster.max_epoch(), r.roster.min_epoch()), (3, 3));
    r.err(r.by_dev(Op::RotateDone { epoch: 3 }, &a));

    // remove_device：移除除签名者外的最后一台设备只能由恢复密钥签名
    r.ok(r.by_dev(Op::RemoveDevice { device_id: c.device_id() }, &a));
    assert!(r.err(r.by_dev(Op::RemoveDevice { device_id: b.device_id() }, &a)).contains("恢复短语"));
    r.err(r.by_dev(Op::RemoveDevice { device_id: c.device_id() }, &a));
    // 被移除的设备不能再签名
    r.err(r.by_dev(Op::AddDevice { device: x.public() }, &c));
    r.ok(r.by_recovery(Op::RemoveDevice { device_id: b.device_id() }, &rk2));
    assert_eq!(r.roster.current_devices().len(), 1);
    // 在册历史
    let removed_at = r.roster.member(&b.device_id()).unwrap().removed.unwrap();
    assert!(r.roster.enrolled_at(&b.device_id(), removed_at - 1).is_some());
    assert!(r.roster.enrolled_at(&b.device_id(), removed_at).is_none());
    // 设备可以移除自己
    r.ok(r.by_recovery(Op::AddDevice { device: x.public() }, &rk2));
    r.ok(r.by_dev(Op::RemoveDevice { device_id: x.device_id() }, &x));
}

#[test]
fn chain_and_signature_checks() {
    let (a, b) = (dev(1), dev(2));
    let rk = RecoveryKeys::from_entropy(&[7; 32]);
    let base = Builder::new(&a, &rk);
    let (idx, prev) = base.roster.head().unwrap();
    let op = || Op::AddDevice { device: b.public() };
    let sign = |e: &Entry| a.roster_sign(&e.signing_message(VAULT));

    // 正确
    let good = Entry::signed(VAULT, idx + 1, prev, op(), Signer::Device(a.device_id()), |m| a.roster_sign(m));
    assert!(base.roster.clone().append(&good.encode()).is_ok());
    // 序号不连续
    let mut e = Entry { index: idx + 2, ..good.clone() };
    e.sig = sign(&e);
    assert!(base.err(e.encode()).contains("不连续"));
    // prev 不衔接（分叉）
    let mut e = Entry { prev: [9; 32], ..good.clone() };
    e.sig = sign(&e);
    assert!(base.err(e.encode()).contains("不衔接"));
    // 签名被篡改
    let mut e = good.clone();
    e.sig[0] ^= 1;
    assert!(base.err(e.encode()).contains("签名"));
    // 签名覆盖 vault_id：别的保险库的签名无效
    let e = Entry::signed("v_other", idx + 1, prev, op(), Signer::Device(a.device_id()), |m| a.roster_sign(m));
    assert!(base.err(e.encode()).contains("签名"));
    // 非规范编码：S 值不规范的签名（verify_strict 拒绝），以及多余字段
    let mut e = good.clone();
    e.sig[63] |= 0xF0;
    assert!(base.err(e.encode()).contains("签名"));
    let mut v = crate::sync::codec::decode(&good.encode()).unwrap();
    if let ciborium::Value::Map(m) = &mut v {
        m.push((crate::sync::codec::text("extra"), crate::sync::codec::uint(1)));
    }
    assert!(base.err(crate::sync::codec::encode(&v)).contains("未知字段"));

    // genesis 只能在第 0 条、只能由自己签名
    let g = Entry::signed(VAULT, 0, ZERO_HASH, Op::Genesis { device: b.public(), vk_commit: [0; 32] }, Signer::Device(a.device_id()), |m| a.roster_sign(m));
    assert!(Roster::from_entries(VAULT, &[g.encode()]).is_err());
    let g = Entry::signed(VAULT, idx + 1, prev, Op::Genesis { device: b.public(), vk_commit: [0; 32] }, Signer::Device(b.device_id()), |m| b.roster_sign(m));
    assert!(base.err(g.encode()).contains("genesis"));
    // 第 0 条之后的第一个 set_recovery 必须由 genesis 设备签名
    let g = Entry::signed(VAULT, 0, ZERO_HASH, Op::Genesis { device: a.public(), vk_commit: [0; 32] }, Signer::Device(a.device_id()), |m| a.roster_sign(m));
    let bind = rk.sign(&recovery_bind_message(VAULT, &g.hash()));
    let s = Entry::signed(VAULT, 1, g.hash(), Op::SetRecovery { x25519: rk.x_public(), ed25519: rk.s_public(), bind }, Signer::Recovery, |m| rk.sign(m));
    assert!(Roster::from_entries(VAULT, &[g.encode(), s.encode()]).is_err());
    // 编码往返
    assert_eq!(Entry::decode(&good.encode()).unwrap(), good);
    assert_eq!(good.hash(), crypto::sha256(&[&good.encode()]));
}
