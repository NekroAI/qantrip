//! 恶意服务端测试：逐项覆盖 docs/sync-design.md 第 8 节表格。

use serde_json::Value as Json;

use super::*;
use crate::sync::codec::{self, b64u};
use crate::sync::crypto::{self, DeviceKeys};
use crate::sync::engine::envelope_plaintext;
use crate::sync::record::{Body, Kind, Plaintext};
use crate::sync::roster::{Entry, Op, Signer, ZERO_HASH};
use crate::sync::{JoinRequestStatus, SyncError};

/// 两台设备，A 写入 x.md 并双方同步完毕。
fn pair() -> (Arc<FakeServer>, Dev, Dev, String) {
    let server = server();
    let (a, phrase) = first_device(&server);
    let b = Dev::signed_in(&server);
    join(&a, &b);
    a.write("x.md", "x1");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    assert_eq!(b.read("x.md").unwrap(), "x1");
    (server, a, b, phrase)
}

fn record_id_of(dev: &Dev, file: &str) -> String {
    dev.core().state.records.iter().find(|(_, m)| m.file.as_deref() == Some(file)).map(|(k, _)| k.clone()).unwrap()
}

fn vault_mut<R>(server: &Arc<FakeServer>, f: impl FnOnce(&mut fake::Vault) -> R) -> R {
    f(server.world.lock().unwrap().vault.as_mut().unwrap())
}

/// 以设备 `dev` 的身份、签名密钥和钥匙环构造一条记录密文（模拟拿到 VK 的攻击者或被撤销的设备）。
pub fn craft(dev: &Dev, rid: &str, epoch: u32, mutate: impl FnOnce(&mut Plaintext)) -> Vec<u8> {
    let core = dev.core();
    let vault = core.vault().unwrap();
    let (index, head) = core.roster().unwrap().head().unwrap();
    let mut pt = Plaintext {
        kind: Kind::Template,
        counter: 100,
        deleted: false,
        writer: core.me().unwrap(),
        roster_index: index,
        roster_head: head,
        time: 0,
        body: Body::Template { file_name: "evil.md".into(), source: "evil".into() },
        sig: [0; 64],
    };
    mutate(&mut pt);
    let keys = core.keys().unwrap();
    pt.sign(&vault, rid, epoch, |m| keys.roster_sign(m));
    let vk = *core.keyring().unwrap().get(epoch).unwrap();
    crypto::seal_record(&vk, &[3; 24], &vault, rid, epoch, &pt.encode())
}

// MARK: 修改、伪造、挪用密文 → AEAD 校验失败，丢弃并报警

#[test]
fn tampered_ciphertext_is_rejected() {
    let (server, a, b, _) = pair();
    a.write("x.md", "x2");
    a.client.sync_now().unwrap();
    let rid = record_id_of(&a, "x.md");
    vault_mut(&server, |v| {
        let r = v.records.get_mut(&rid).unwrap();
        let n = r.ct.len();
        r.ct[n - 20] ^= 0x01;
    });
    let r = b.client.sync_now().unwrap();
    assert_eq!(r.rejected, 1);
    assert!(b.has_alert("tampered"));
    assert_eq!(b.read("x.md").unwrap(), "x1", "保留本地已验证的版本");
}

#[test]
fn moved_or_relabeled_ciphertext_is_rejected() {
    let (server, a, b, _) = pair();
    a.write("y.md", "y1");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    let x = record_id_of(&a, "x.md");
    let y = record_id_of(&a, "y.md");
    // 挪用：把 y 的密文放到 x 的位置（AAD 绑定 record_id）
    vault_mut(&server, |v| {
        let ct = v.records[&y].ct.clone();
        v.put(&x, 1, ct, false);
    });
    b.client.sync_now().unwrap();
    assert!(b.has_alert("tampered"));
    assert_eq!(b.read("x.md").unwrap(), "x1");
    // 改写 epoch 标签（AAD 绑定 epoch）
    b.client.dismiss_alerts();
    vault_mut(&server, |v| {
        let ct = v.records[&y].ct.clone();
        v.put(&y, 2, ct, false);
    });
    b.client.sync_now().unwrap();
    assert!(b.has_alert("missing_key") || b.has_alert("tampered"));
    assert_eq!(b.read("y.md").unwrap(), "y1");
}

#[test]
fn server_deleted_flag_is_ignored() {
    let (server, a, b, _) = pair();
    a.write("x.md", "x2");
    a.client.sync_now().unwrap();
    let rid = record_id_of(&a, "x.md");
    vault_mut(&server, |v| v.records.get_mut(&rid).unwrap().deleted = true);
    b.client.sync_now().unwrap();
    assert_eq!(b.read("x.md").unwrap(), "x2");
}

#[test]
fn writer_not_enrolled_is_rejected() {
    let (server, a, b, _) = pair();
    let rid = crypto::random_record_id();
    let ct = craft(&a, &rid, 1, |pt| pt.writer = DeviceKeys::from_seeds([9; 32], [9; 32], [9; 32]).device_id());
    vault_mut(&server, |v| v.put(&rid, 1, ct, false));
    b.client.sync_now().unwrap();
    assert!(b.has_alert("writer_not_enrolled"));
    assert!(b.read("evil.md").is_none());
}

#[test]
fn state_record_id_cannot_be_hijacked() {
    let (server, a, b, _) = pair();
    // 以模板类型占用 A 的状态记录 ID
    let vault = a.core().vault().unwrap();
    let a_id = a.id();
    let rid = crypto::state_record_id(&vault, &a_id);
    let ct = craft(&a, &rid, 1, |_| {});
    vault_mut(&server, |v| v.put(&rid, 1, ct, false));
    b.client.sync_now().unwrap();
    assert!(b.has_alert("bad_kind"));
    assert!(b.read("evil.md").is_none());
}

// MARK: 在信封中塞入自选的 VK → vk_commit 或 HPKE 认证失败，拒绝

#[test]
fn chosen_vk_in_envelope_is_rejected() {
    let (server, a, b, _) = pair();
    a.client.rotate_keys().unwrap();
    a.write("after.md", "new data");
    a.client.sync_now().unwrap();
    let b_id = b.id();
    let b_pk = b.core().keys().unwrap().public().x25519;
    let (vault, index, head) = {
        let core = a.core();
        let (i, h) = core.roster().unwrap().head().unwrap();
        (core.vault().unwrap(), i, h)
    };
    let good = vault_mut(&server, |v| v.envelopes[&(b_id.clone(), 2)].clone());

    let forge = |sender: &DeviceKeys, vk: [u8; 32]| -> Vec<u8> {
        let mut ring = crate::sync::engine::Keyring::default();
        ring.insert(2, vk);
        let pt = envelope_plaintext(&ring, index, &head);
        let info = crypto::envelope_info(&vault, &b_id, 2, index);
        let (enc, ct) = crypto::seal_envelope(&mut crypto::system_rng(), sender.enc_secret(), &b_pk, &info, &pt).unwrap();
        codec::encode(&codec::map(vec![("sender", codec::text(&sender.device_id())), ("enc", codec::bytes(&enc)), ("ct", codec::bytes(&ct))]))
    };
    // 1) 服务端自己的密钥冒充 A 发送（声称 sender = A，但 HPKE 认证用的是攻击者私钥）
    let attacker = DeviceKeys::from_seeds([5; 32], [6; 32], [7; 32]);
    let mut data = forge(&attacker, [0xAA; 32]);
    let a_id = a.id();
    let mut f = codec::Fields::exact(codec::decode(&data).unwrap(), &["sender", "enc", "ct"]).unwrap();
    let (enc, ct) = (f.bytes("enc").unwrap(), f.bytes("ct").unwrap());
    let _ = f.text("sender");
    data = codec::encode(&codec::map(vec![("sender", codec::text(&a_id)), ("enc", codec::bytes(&enc)), ("ct", codec::bytes(&ct))]));
    vault_mut(&server, |v| v.envelopes.insert((b_id.clone(), 2), (index, data)));
    b.write("b-edit.md", "should not be written with a server-chosen key");
    let seq_before = vault_mut(&server, |v| v.seq);
    b.client.sync_now().unwrap();
    assert!(b.has_alert("envelope_invalid"));
    assert!(b.core().keyring().unwrap().get(2).is_none());
    assert!(b.read("after.md").is_none(), "拿不到真正的新 VK，也不会用服务端的 VK");

    // 2) 用真实发送方的密钥（假设服务端同时控制了一台设备的加密私钥），但 VK 与花名册承诺不符
    b.client.dismiss_alerts();
    let a_keys = a.core().keys().unwrap();
    let data = forge(&a_keys, [0xBB; 32]);
    vault_mut(&server, |v| v.envelopes.insert((b_id.clone(), 2), (index, data)));
    b.client.sync_now().unwrap();
    assert!(b.has_alert("envelope_invalid"));
    assert!(b.core().keyring().unwrap().get(2).is_none());
    // 期间没有用任何服务端选定的 VK 写入
    assert_eq!(vault_mut(&server, |v| v.seq), seq_before);
    assert!(b.read("after.md").is_none());

    // 恢复真实信封后正常
    vault_mut(&server, |v| v.envelopes.insert((b_id.clone(), 2), good));
    b.client.sync_now().unwrap();
    assert_eq!(b.read("after.md").unwrap(), "new data");
    a.client.sync_now().unwrap();
    assert_eq!(a.read("b-edit.md").unwrap(), "should not be written with a server-chosen key");
}

// MARK: 返回旧版本 / 隐藏记录 → 计数器或状态记录比对发现，报警

#[test]
fn rollback_is_detected_and_own_version_repushed() {
    let (server, a, b, _) = pair();
    let rid = record_id_of(&a, "x.md");
    let old = vault_mut(&server, |v| v.records[&rid].clone());
    a.write("x.md", "x2");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    assert_eq!(b.read("x.md").unwrap(), "x2");
    // 服务端把 x 换回旧密文
    vault_mut(&server, |v| v.put(&rid, old.epoch, old.ct.clone(), false));
    b.client.sync_now().unwrap();
    assert!(b.has_alert("rollback"));
    assert_eq!(b.read("x.md").unwrap(), "x2");
    // A 发现自己的版本被回滚，自动重新推送
    a.client.sync_now().unwrap();
    assert!(a.has_alert("rollback"));
    let counter_before = b.core().state.records[&rid].counter;
    b.client.dismiss_alerts();
    b.client.sync_now().unwrap();
    assert!(b.core().state.records[&rid].counter > counter_before);
    assert_eq!(b.read("x.md").unwrap(), "x2");
    assert!(b.alert_codes().is_empty(), "{:?}", b.alert_codes());
}

#[test]
fn same_counter_different_content_is_a_fork() {
    let (server, a, b, _) = pair();
    let rid = record_id_of(&a, "x.md");
    let counter = a.core().state.records[&rid].counter;
    let ct = craft(&a, &rid, 1, |pt| {
        pt.counter = counter;
        pt.body = Body::Template { file_name: "x.md".into(), source: "forked".into() };
    });
    vault_mut(&server, |v| v.put(&rid, 1, ct, false));
    b.client.sync_now().unwrap();
    assert!(b.has_alert("fork"));
    assert_eq!(b.read("x.md").unwrap(), "x1");
}

#[test]
fn hidden_record_is_detected() {
    let (server, a, _b, _) = pair();
    a.write("y.md", "y1");
    a.client.sync_now().unwrap();
    let y = record_id_of(&a, "y.md");
    vault_mut(&server, |v| {
        v.records.remove(&y);
    });
    // 新设备首次同步：A 的状态记录说 y 应当存在
    let c = Dev::signed_in(&server);
    join(&a, &c);
    c.client.sync_now().unwrap();
    assert!(c.has_alert("hidden"), "{:?}", c.alert_codes());
    assert!(c.read("y.md").is_none());
    // A 全量对账时发现自己的记录缺失，重新推送
    a.core().state.needs_full = true;
    a.client.sync_now().unwrap();
    assert!(a.has_alert("hidden"));
    assert!(vault_mut(&server, |v| v.records.contains_key(&y)));
    c.client.sync_now().unwrap();
    assert_eq!(c.read("y.md").unwrap(), "y1");
}

#[test]
fn withheld_newer_version_is_detected_by_new_device() {
    let (server, a, _b, _) = pair();
    let rid = record_id_of(&a, "x.md");
    let old = vault_mut(&server, |v| v.records[&rid].clone());
    a.write("x.md", "x2");
    a.client.sync_now().unwrap();
    // 服务端对新设备扣留 x 的新版本，只给旧版本
    vault_mut(&server, |v| v.put(&rid, old.epoch, old.ct.clone(), false));
    let c = Dev::signed_in(&server);
    join(&a, &c);
    c.client.sync_now().unwrap();
    assert!(c.has_alert("withheld"), "{:?}", c.alert_codes());
}

#[test]
fn hidden_state_record_is_detected() {
    let (server, a, b, _) = pair();
    let vault = a.core().vault().unwrap();
    let a_id = a.id();
    let state_a = crypto::state_record_id(&vault, &a_id);
    vault_mut(&server, |v| {
        v.records.remove(&state_a);
    });
    let c = Dev::signed_in(&server);
    join(&a, &c);
    c.client.sync_now().unwrap();
    assert!(c.has_alert("state_missing"), "{:?}", c.alert_codes());
    let _ = b;
}

// MARK: 伪造、分叉、截断花名册 → 链校验失败或头部传播发现，停止写入并报警

#[test]
fn forged_roster_entry_stops_writes() {
    let (server, a, b, _) = pair();
    let vault = a.core().vault().unwrap();
    let raws = vault_mut(&server, |v| v.roster.clone());
    let roster = crate::sync::roster::Roster::from_entries(&vault, &raws).unwrap();
    let (idx, prev) = roster.head().unwrap();
    // 服务端用自己的密钥签一条 add_device，声称签名者是 A
    let attacker = DeviceKeys::from_seeds([1; 32], [2; 32], [3; 32]);
    let forged = Entry::signed(&vault, idx + 1, prev, Op::AddDevice { device: attacker.public() }, Signer::Device(a.id()), |m| attacker.roster_sign(m));
    vault_mut(&server, |v| v.roster.push(forged.encode()));
    b.write("z.md", "z");
    assert!(matches!(b.client.sync_now(), Err(SyncError::Security { .. })));
    assert!(b.has_alert("roster_invalid"));
    assert!(b.client.status().writes_blocked);
    assert!(!vault_mut(&server, |v| v.records.len() > 3), "没有写入");
    // 以 genesis 冒充第二条、或复用别处签名的条目同样被拒绝
    vault_mut(&server, |v| {
        v.roster.pop();
        let g = v.roster[0].clone();
        v.roster.push(g);
    });
    assert!(b.client.sync_now().is_err());
}

#[test]
fn truncated_roster_is_detected() {
    let (server, a, b, _) = pair();
    vault_mut(&server, |v| {
        v.roster.pop();
    });
    assert!(matches!(b.client.sync_now(), Err(SyncError::Security { .. })));
    assert!(b.has_alert("roster_truncated"));
    assert!(b.client.status().writes_blocked);
    let _ = a;
}

#[test]
fn record_referencing_forked_roster_stops_writes() {
    let (server, a, b, _) = pair();
    let rid = crypto::random_record_id();
    let ct = craft(&a, &rid, 1, |pt| {
        pt.counter = 1;
        pt.roster_head = [0x42; 32];
    });
    vault_mut(&server, |v| v.put(&rid, 1, ct, false));
    b.client.sync_now().unwrap();
    assert!(b.has_alert("roster_fork"));
    assert!(b.client.status().writes_blocked);
}

#[test]
fn withheld_roster_head_stops_writes_until_propagated() {
    let (server, a, b, _) = pair();
    let b_len = b.core().roster().unwrap().len();
    // A 批准第三台设备（花名册延长），并写入引用新头部的记录
    let c = Dev::signed_in(&server);
    join(&a, &c);
    a.write("x.md", "x2");
    a.client.sync_now().unwrap();
    // 服务端对 B 隐藏新的花名册条目
    server.world.lock().unwrap().hook = Some(Box::new(move |method, path, v: &mut Json| {
        if method == "GET" && path.ends_with("/roster") {
            v["entries"] = Json::Array(vec![]);
            v["length"] = Json::from(b_len);
        }
        if method == "GET" && path.ends_with("/head") {
            v["roster_index"] = Json::from(b_len);
        }
    }));
    b.write("b.md", "local edit");
    let r = b.client.sync_now().unwrap();
    assert!(r.writes_blocked);
    assert!(b.has_alert("roster_withheld"));
    assert_eq!(b.read("x.md").unwrap(), "x1");
    assert!(b.client.status().writes_blocked);
    assert!(!vault_mut(&server, |v| v.records.len() > 5), "B 没有写入");
    // 花名册恢复传播后自动解除
    server.world.lock().unwrap().hook = None;
    let r = b.client.sync_now().unwrap();
    assert!(!r.writes_blocked);
    assert_eq!(b.read("x.md").unwrap(), "x2");
    a.client.sync_now().unwrap();
    assert_eq!(a.read("b.md").unwrap(), "local edit");
}

// MARK: 冒充已授权设备、替换加入请求中的公钥 → 校验码无法在真实设备上输入成功

#[test]
fn fake_approver_not_in_roster_is_rejected() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    // 服务端以自己的设备回复，并附上一条以该设备为 genesis 的伪造花名册
    let vault = a.core().vault().unwrap();
    let fake = DeviceKeys::from_seeds([4; 32], [4; 32], [4; 32]);
    let g = Entry::signed(&vault, 0, ZERO_HASH, Op::Genesis { device: fake.public(), vk_commit: [0; 32] }, Signer::Device(fake.device_id()), |m| fake.roster_sign(m));
    let mut real = vault_mut(&server, |v| v.roster.clone());
    // 1) 真实花名册 + 不在册的 pk_E
    let reply = codec::encode(&codec::map(vec![
        ("pk", codec::bytes(&fake.public().concat())),
        ("nonce", codec::bytes(&[1; 32])),
        ("roster", ciborium::Value::Array(real.iter().map(|r| codec::bytes(r)).collect())),
    ]));
    vault_mut(&server, |v| v.joins[0].reply = Some(reply));
    assert!(matches!(n.client.poll_join(), Err(SyncError::Security { .. })));
    assert!(!n.client.status().enrolled);
    // 2) 伪造花名册（genesis 为服务端设备）：N 会显示校验码，但真实设备上输入必然失败
    let p2 = n.client.start_join().unwrap();
    assert_ne!(p.join_id, p2.join_id);
    real.clear();
    real.push(g.encode());
    let reply = codec::encode(&codec::map(vec![
        ("pk", codec::bytes(&fake.public().concat())),
        ("nonce", codec::bytes(&[1; 32])),
        ("roster", ciborium::Value::Array(real.iter().map(|r| codec::bytes(r)).collect())),
    ]));
    vault_mut(&server, |v| v.joins[0].reply = Some(reply));
    let shown = n.client.poll_join().unwrap();
    assert_eq!(shown.stage, JoinStage::EnterCodeOnOtherDevice);
    // 真实设备 A 没有回复过这个请求，无法批准
    assert_eq!(a.client.list_join_requests().unwrap()[0].status, JoinRequestStatus::HandledElsewhere);
    assert!(a.client.approve_join(p2.join_id.clone(), shown.sas.unwrap()).is_err());
    // 服务端也无法伪造结果：没有 A 的私钥，签不出 add_device，也无法发出认证信封
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::EnterCodeOnOtherDevice);
    assert!(!n.client.status().enrolled);
}

#[test]
fn replaced_approver_nonce_makes_sas_mismatch() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    // 服务端在转发给 N 时替换 n_E（保留真实 pk_E 与花名册）
    let reply = vault_mut(&server, |v| v.joins[0].reply.clone().unwrap());
    let mut f = codec::Fields::exact(codec::decode(&reply).unwrap(), &["pk", "nonce", "roster"]).unwrap();
    let swapped = codec::encode(&codec::map(vec![("pk", codec::bytes(&f.bytes("pk").unwrap())), ("nonce", codec::bytes(&[0xEE; 32])), ("roster", f.value("roster"))]));
    let swapped_b64 = b64u(&swapped);
    server.world.lock().unwrap().hook = Some(Box::new(move |method, path, v: &mut Json| {
        if method == "GET" && path.contains("/joins/") && !v["reply"].is_null() {
            v["reply"] = Json::from(swapped_b64.clone());
        }
    }));
    let shown = n.client.poll_join().unwrap().sas.unwrap();
    server.world.lock().unwrap().hook = None;
    // 用户在 A 上输入 N 显示的校验码：与 A 计算的不一致
    assert!(matches!(a.client.approve_join(p.join_id, shown), Err(SyncError::SasMismatch { .. })));
    assert!(!n.client.status().enrolled);
}

#[test]
fn replaced_new_device_key_fails_commitment() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let shown = n.client.poll_join().unwrap().sas.unwrap();
    // 服务端把 reveal 中的 pk_N 换成自己的公钥（承诺不符）
    let attacker = DeviceKeys::from_seeds([8; 32], [8; 32], [8; 32]);
    let reveal = codec::encode(&codec::map(vec![("pk", codec::bytes(&attacker.public().concat())), ("nonce", codec::bytes(&[2; 32]))]));
    vault_mut(&server, |v| v.joins[0].reveal = Some(reveal));
    assert!(matches!(a.client.approve_join(p.join_id.clone(), shown.clone()), Err(SyncError::Security { .. })));
    assert!(a.has_alert("join_failed"));
    assert_eq!(a.core().roster().unwrap().len(), 2, "没有追加 add_device");
}

/// 服务端在 E 回复之后整体替换承诺与公开值（自洽，承诺校验能通过）：E 发现承诺与回复时锁定的不同，拒绝（C1）。
#[test]
fn replaced_commit_and_reveal_are_rejected() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let shown = n.client.poll_join().unwrap().sas.unwrap();
    // 服务端整体替换为自己的承诺与公开值（自洽，承诺校验能通过）
    let vault = a.core().vault().unwrap();
    let attacker = DeviceKeys::from_seeds([8; 32], [8; 32], [8; 32]);
    let commit = crypto::join_commit(&vault, &attacker.public().concat(), &[2; 32]);
    let commit_msg = codec::encode(&codec::map(vec![("v", codec::uint(1)), ("commit", codec::bytes(&commit))]));
    let reveal = codec::encode(&codec::map(vec![("pk", codec::bytes(&attacker.public().concat())), ("nonce", codec::bytes(&[2; 32]))]));
    vault_mut(&server, |v| {
        v.joins[0].commit = commit_msg;
        v.joins[0].reveal = Some(reveal);
    });
    assert!(matches!(a.client.approve_join(p.join_id, shown), Err(SyncError::Security { .. })));
    assert!(a.has_alert("join_failed"));
    assert!(!a.core().roster().unwrap().members().any(|(id, _)| *id == attacker.device_id()));
}

#[test]
fn forged_join_result_is_rejected() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    n.client.poll_join().unwrap();
    // 服务端伪造结果：没有新花名册条目、信封来自攻击者
    let result = codec::encode(&codec::map(vec![
        ("roster", ciborium::Value::Array(vec![])),
        ("envelope", codec::bytes(&[1, 2, 3])),
        ("epoch", codec::uint(1)),
        ("roster_index", codec::uint(1)),
        ("anchor", ciborium::Value::Null),
    ]));
    vault_mut(&server, |v| v.joins[0].result = Some(result));
    assert!(matches!(n.client.poll_join(), Err(SyncError::Security { .. })));
    assert!(!n.client.status().enrolled);
}

// MARK: 扣留其他设备的最新写入 → 状态记录对账发现（见 withheld_newer_version_is_detected_by_new_device）

// MARK: 被撤销设备

#[test]
fn revoked_device_old_epoch_writes_are_rejected() {
    let (server, a, b, phrase) = pair();
    let rid = crypto::random_record_id();
    // B 在被撤销前保留了 epoch 1 的 VK
    let ct = craft(&b, &rid, 1, |pt| pt.counter = 1);
    a.client.revoke_device(b.id(), Some(phrase)).unwrap();
    // 服务端与被撤销设备勾结，接受旧 epoch 写入
    vault_mut(&server, |v| v.put(&rid, 1, ct, false));
    a.client.sync_now().unwrap();
    assert!(a.has_alert("stale_epoch"));
    assert!(a.read("evil.md").is_none());
    // 正常服务端会拒绝旧 epoch 的写入
    assert_eq!(vault_mut(&server, |v| v.min_epoch), 2);
}

// MARK: 恢复短语加入

#[test]
fn recovery_rejects_fabricated_or_truncated_roster() {
    let (server, a, b, phrase) = pair();
    a.client.revoke_device(b.id(), Some(phrase.clone())).unwrap();
    let real = vault_mut(&server, |v| v.roster.clone());
    let vault = a.core().vault().unwrap();

    // 1) 服务端伪造整条花名册：以自己的设备为 genesis，set_recovery 填入用户真实的恢复公钥
    let rk = crypto::RecoveryKeys::from_phrase(&phrase).unwrap();
    let fake = DeviceKeys::from_seeds([4; 32], [4; 32], [4; 32]);
    let g = Entry::signed(&vault, 0, ZERO_HASH, Op::Genesis { device: fake.public(), vk_commit: [0; 32] }, Signer::Device(fake.device_id()), |m| fake.roster_sign(m));
    // 服务端没有恢复私钥，只能复用真实花名册中的绑定签名
    let real_bind = match crate::sync::roster::Entry::decode(&real[1]).unwrap().op {
        Op::SetRecovery { bind, .. } => bind,
        _ => unreachable!(),
    };
    let s = Entry::signed(&vault, 1, g.hash(), Op::SetRecovery { x25519: rk.x_public(), ed25519: rk.s_public(), bind: real_bind }, Signer::Device(fake.device_id()), |m| fake.roster_sign(m));
    vault_mut(&server, |v| v.roster = vec![g.encode(), s.encode()]);
    let c = Dev::signed_in(&server);
    assert!(matches!(c.client.recover(phrase.clone()), Err(SyncError::Security { .. })));

    // 2) 截断花名册（早于恢复信封记录的头部）
    vault_mut(&server, |v| v.roster = real[..3].to_vec());
    assert!(matches!(c.client.recover(phrase.clone()), Err(SyncError::Security { .. })));
    assert!(!c.client.status().enrolled);

    // 3) 完整花名册：成功
    vault_mut(&server, |v| v.roster = real.clone());
    c.client.recover(phrase).unwrap();
    assert!(c.client.status().enrolled);
}
