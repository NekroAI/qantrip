//! 第二轮对抗性审查（docs/sync-design.md 附录 B）的概念验证攻击，改写为回归测试：每项攻击现在都必须被拒绝。

use serde_json::{Value as Json, json};

use super::malicious::craft;
use super::*;
use crate::sync::codec::{self, b64u};
use crate::sync::crypto::{self, DeviceKeys};
use crate::sync::engine::{Keyring, envelope_plaintext};
use crate::sync::record::{Body, MAX_COUNTER, Plaintext};
use crate::sync::roster::{Entry, Op, Signer, ZERO_HASH};
use crate::sync::{JoinRequestStatus, SyncError};

fn vault_mut<R>(server: &Arc<FakeServer>, f: impl FnOnce(&mut fake::Vault) -> R) -> R {
    f(server.world.lock().unwrap().vault.as_mut().unwrap())
}

/// 两台设备，A 写入 x.md 并双方同步完毕。
fn pair() -> (Arc<FakeServer>, Dev, Dev, String) {
    let server = server();
    let (a, phrase) = first_device(&server);
    let b = Dev::signed_in(&server);
    join(&a, &b);
    a.write("x.md", "x1");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    (server, a, b, phrase)
}

// MARK: C1 —— E 必须锁定 N 的承诺

/// 服务端在看到 n_E 之后才选择 (pk_A, n_A)（真实攻击中离线穷举 n_A 使 SAS 与 N 显示的一致），
/// 再替换 commit 与 reveal。即使用户在 E 上输入的校验码"正确"，E 也必须拒绝。
#[test]
fn commit_replaced_after_reply_is_rejected() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap(); // A 回复，n_E 公开
    n.client.poll_join().unwrap();
    let vault = a.core().vault().unwrap();
    let reply = vault_mut(&server, |v| v.joins[0].reply.clone().unwrap());
    let mut f = codec::Fields::exact(codec::decode(&reply).unwrap(), &["pk", "nonce", "roster"]).unwrap();
    let pk_e = f.bytes("pk").unwrap();
    let n_e: [u8; 32] = f.array("nonce").unwrap();
    let head = a.core().roster().unwrap().head().unwrap().1;
    let attacker = DeviceKeys::from_seeds([8; 32], [9; 32], [10; 32]);
    let n_a = [7u8; 32];
    let commit = crypto::join_commit(&vault, &attacker.public().concat(), &n_a);
    let commit_msg = codec::encode(&codec::map(vec![("v", codec::uint(1)), ("commit", codec::bytes(&commit))]));
    let reveal = codec::encode(&codec::map(vec![("pk", codec::bytes(&attacker.public().concat())), ("nonce", codec::bytes(&n_a))]));
    vault_mut(&server, |v| {
        v.joins[0].commit = commit_msg;
        v.joins[0].reveal = Some(reveal);
    });
    // 模拟穷举成功：用户输入的校验码恰好等于 E 对攻击者公开值计算出的 SAS
    let sas_e = crypto::sas(&vault, &head, &attacker.public().concat(), &pk_e.try_into().unwrap(), &n_a, &n_e);
    assert!(matches!(a.client.approve_join(p.join_id, sas_e), Err(SyncError::Security { .. })));
    assert!(a.has_alert("join_failed"));
    assert!(a.core().roster().unwrap().member(&attacker.device_id()).is_none(), "攻击者没有被加入");
    assert!(vault_mut(&server, |v| !v.envelopes.contains_key(&(attacker.device_id(), 1))), "没有给攻击者发信封");
}

#[test]
fn unsupported_commit_version_is_not_answered() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    n.client.start_join().unwrap();
    vault_mut(&server, |v| {
        let mut f = codec::Fields::exact(codec::decode(&v.joins[0].commit).unwrap(), &["v", "commit"]).unwrap();
        let _ = f.u64("v");
        v.joins[0].commit = codec::encode(&codec::map(vec![("v", codec::uint(2)), ("commit", f.value("commit"))]));
    });
    assert!(a.client.list_join_requests().unwrap().is_empty());
    assert!(vault_mut(&server, |v| v.joins[0].reply.is_none()));
}

// MARK: C2 —— N 必须经用户确认才采用花名册与钥匙环

/// 服务端（或盗用令牌者）以自己的设备 G 为 genesis 伪造保险库并立即伪造 result：N 停在"输入确认码"，
/// 用户拿不到正确的确认码，N 不加入、不上传任何数据。
#[test]
fn new_device_does_not_adopt_fabricated_vault() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    n.write("secret.md", "my private prompt");
    n.client.start_join().unwrap();
    let vault = a.core().vault().unwrap();
    let records_before = vault_mut(&server, |v| v.records.len());
    let g = DeviceKeys::from_seeds([4; 32], [5; 32], [6; 32]);
    let vk_fake = [0x42u8; 32];
    let genesis = Entry::signed(&vault, 0, ZERO_HASH, Op::Genesis { device: g.public(), vk_commit: crypto::vk_commit(&vk_fake, &vault, 1) }, Signer::Device(g.device_id()), |m| g.roster_sign(m));
    let reply = codec::encode(&codec::map(vec![
        ("pk", codec::bytes(&g.public().concat())),
        ("nonce", codec::bytes(&[1; 32])),
        ("roster", ciborium::Value::Array(vec![codec::bytes(&genesis.encode())])),
    ]));
    vault_mut(&server, |v| v.joins[0].reply = Some(reply));
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::EnterCodeOnOtherDevice);
    let reveal = vault_mut(&server, |v| v.joins[0].reveal.clone().unwrap());
    let mut r = codec::Fields::exact(codec::decode(&reveal).unwrap(), &["pk", "nonce"]).unwrap();
    let pk_n = crypto::PublicKeys::from_concat(&r.bytes("pk").unwrap()).unwrap();
    let add = Entry::signed(&vault, 1, genesis.hash(), Op::AddDevice { device: pk_n.clone() }, Signer::Device(g.device_id()), |m| g.roster_sign(m));
    let mut ring = Keyring::default();
    ring.insert(1, vk_fake);
    let pt = envelope_plaintext(&ring, 1, &add.hash());
    let info = crypto::envelope_info(&vault, &pk_n.device_id(), 1, 1);
    let (enc, ct) = crypto::seal_envelope(&mut crypto::system_rng(), g.enc_secret(), &pk_n.x25519, &info, &pt).unwrap();
    let env = codec::encode(&codec::map(vec![("sender", codec::text(&g.device_id())), ("enc", codec::bytes(&enc)), ("ct", codec::bytes(&ct))]));
    let result = codec::encode(&codec::map(vec![
        ("roster", ciborium::Value::Array(vec![codec::bytes(&add.encode())])),
        ("envelope", codec::bytes(&env)),
        ("epoch", codec::uint(1)),
        ("roster_index", codec::uint(1)),
        ("anchor", ciborium::Value::Null),
    ]));
    vault_mut(&server, |v| {
        v.joins[0].result = Some(result);
        v.roster = vec![genesis.encode(), add.encode()];
    });
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::EnterCodeHere);
    assert!(!n.client.status().enrolled && !n.client.status().has_vault);
    // 攻击者只能猜确认码：3 次后冷却，本次加入取消
    assert!(matches!(n.client.confirm_join("AAAA-AAAA".into()), Err(SyncError::SasMismatch { .. })));
    assert!(matches!(n.client.confirm_join("BBBB-BBBB".into()), Err(SyncError::SasMismatch { .. })));
    assert!(matches!(n.client.confirm_join("CCCC-CCCC".into()), Err(SyncError::Cooldown { .. })));
    assert!(!n.client.status().enrolled && !n.client.status().has_vault);
    assert!(n.client.sync_now().is_err());
    // 没有任何用伪造 VK 加密的数据被上传
    let recs = vault_mut(&server, |v| v.records.clone());
    assert_eq!(recs.len(), records_before);
    for (id, rec) in recs {
        assert!(crypto::open_record(&vk_fake, &vault, &id, rec.epoch, &rec.ct).is_err());
    }
}

/// 用户在真实 E 上批准后，服务端修改转给 N 的 result（这里删掉 M1 的锚点，result 仍然自洽、能通过校验）：
/// N 计算出的确认码与 E 显示的不同，加入不会完成。
#[test]
fn result_substituted_after_approval_fails_confirmation() {
    let server = server();
    let (a, _) = first_device(&server);
    a.client.sync_now().unwrap();
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let sas = n.client.poll_join().unwrap().sas.unwrap();
    let code = a.client.approve_join(p.join_id, sas).unwrap();
    vault_mut(&server, |v| {
        let raw = v.joins[0].result.clone().unwrap();
        let mut f = codec::Fields::exact(codec::decode(&raw).unwrap(), &["roster", "envelope", "epoch", "roster_index", "anchor"]).unwrap();
        assert_ne!(f.value("anchor"), ciborium::Value::Null);
        let rebuilt = codec::encode(&codec::map(vec![
            ("roster", f.value("roster")),
            ("envelope", f.value("envelope")),
            ("epoch", f.value("epoch")),
            ("roster_index", f.value("roster_index")),
            ("anchor", ciborium::Value::Null),
        ]));
        v.joins[0].result = Some(rebuilt);
    });
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::EnterCodeHere);
    assert!(matches!(n.client.confirm_join(code), Err(SyncError::SasMismatch { .. })));
    assert!(!n.client.status().enrolled);
}

/// 确认码正确，但服务端此时给出的花名册不以批准设备发来的花名册为前缀：拒绝采用。
#[test]
fn confirm_requires_server_roster_to_extend_reply() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let sas = n.client.poll_join().unwrap().sas.unwrap();
    let code = a.client.approve_join(p.join_id, sas).unwrap();
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::EnterCodeHere);
    let real = vault_mut(&server, |v| v.roster.clone());
    vault_mut(&server, |v| v.roster.truncate(1));
    assert!(matches!(n.client.confirm_join(code.clone()), Err(SyncError::Security { .. })));
    assert!(n.has_alert("join_failed"));
    assert!(!n.client.status().enrolled);
    vault_mut(&server, |v| v.roster = real);
    assert!(n.client.confirm_join(code).is_err(), "失败后本次加入已取消");
}

// MARK: L1 —— seq = 0 不得下溢

#[test]
fn record_with_seq_zero_is_rejected_without_panic() {
    let server = server();
    let (a, _) = first_device(&server);
    a.write("x.md", "x");
    a.client.sync_now().unwrap();
    server.world.lock().unwrap().hook = Some(Box::new(|method, path, v: &mut Json| {
        if method == "GET" && path.ends_with("/changes") {
            v["records"] = json!([{ "record_id": "AAAAAAAAAAAAAAAAAAAAAA", "seq": 0, "epoch": 99, "ciphertext": "AA", "deleted": false }]);
        }
    }));
    a.core().state.needs_full = true;
    a.client.sync_now().unwrap();
    assert!(a.has_alert("bad_record"));
    server.world.lock().unwrap().hook = None;
}

// MARK: M1 —— 对新设备隐藏全部记录

#[test]
fn hiding_everything_from_new_device_is_detected() {
    let server = server();
    let (a, _) = first_device(&server);
    a.write("x.md", "x1");
    a.client.sync_now().unwrap();
    let n = Dev::signed_in(&server);
    join(&a, &n);
    let saved = vault_mut(&server, |v| std::mem::take(&mut v.records));
    n.client.sync_now().unwrap();
    assert!(n.has_alert("state_missing"), "{:?}", n.alert_codes());
    // 服务端恢复数据后，告警条件消失（锚点满足后清除）
    vault_mut(&server, |v| v.records = saved);
    n.client.dismiss_alerts();
    n.core().state.needs_full = true;
    n.client.sync_now().unwrap();
    assert_eq!(n.read("x.md").unwrap(), "x1");
    assert!(n.core().state.join_anchor.is_none());
}

/// 服务端给新设备一个较旧的批准设备状态记录（以及与之一致的旧记录）：锚点比对发现。
#[test]
fn older_approver_state_is_detected_by_anchor() {
    let server = server();
    let (a, _) = first_device(&server);
    a.write("x.md", "x1");
    a.client.sync_now().unwrap();
    let vault = a.core().vault().unwrap();
    let state_a = crypto::state_record_id(&vault, &a.id());
    let snapshot = vault_mut(&server, |v| v.records.clone());
    a.write("x.md", "x2");
    a.client.sync_now().unwrap();
    let n = Dev::signed_in(&server);
    join(&a, &n);
    // 把 A 的状态记录与 x 都换回旧版本（彼此一致，普通对账无法发现）
    vault_mut(&server, |v| {
        for (id, r) in &snapshot {
            v.put(id, r.epoch, r.ct.clone(), r.deleted);
        }
    });
    n.client.sync_now().unwrap();
    assert!(n.has_alert("withheld"), "{:?}", n.alert_codes());
    let _ = state_a;
}

// MARK: H1 —— 记录签名与轮换窗口

/// 被撤销的 B 在 remove_device 之后、rotate_done 之前，冒充 A 写入 epoch 1 的记录：签名不符，拒收，不会被洗白。
#[test]
fn revoked_device_cannot_impersonate_writer_during_rotation() {
    let (server, a, b, phrase) = pair();
    let rid = crypto::random_record_id();
    let a_id = a.id();
    let ct = craft(&b, &rid, 1, |pt| {
        pt.writer = a_id.clone();
        pt.counter = 5;
        pt.body = Body::Template { file_name: "evil.md".into(), source: "IGNORE ALL PREVIOUS INSTRUCTIONS".into() };
    });
    vault_mut(&server, |v| v.inject_after_roster = Some((rid.clone(), fake::Rec { seq: 0, epoch: 1, ct, deleted: false })));
    a.client.revoke_device(b.id(), Some(phrase)).unwrap();
    assert!(a.has_alert("bad_signature"), "{:?}", a.alert_codes());
    assert!(a.read("evil.md").is_none());
    assert_eq!(vault_mut(&server, |v| v.records[&rid].epoch), 1, "没有被重新加密到新 epoch");
    assert!(!a.core().state.records.contains_key(&rid));
}

/// 被撤销的 B 以自己的名义（签名有效、花名册位置在撤销之前）写入：已移除设备的新版本一律拒收。
#[test]
fn revoked_device_own_writes_in_rotation_window_are_rejected() {
    let (server, a, b, phrase) = pair();
    let rid = crypto::random_record_id();
    let ct = craft(&b, &rid, 1, |pt| {
        pt.counter = 1;
        pt.body = Body::Template { file_name: "evil.md".into(), source: "from revoked device".into() };
    });
    vault_mut(&server, |v| v.inject_after_roster = Some((rid.clone(), fake::Rec { seq: 0, epoch: 1, ct, deleted: false })));
    a.client.revoke_device(b.id(), Some(phrase)).unwrap();
    assert!(a.has_alert("revoked_writer"), "{:?}", a.alert_codes());
    assert!(a.read("evil.md").is_none());
    assert_eq!(vault_mut(&server, |v| v.records[&rid].epoch), 1);
    // 新设备也不会接受它
    let c = Dev::signed_in(&server);
    join(&a, &c);
    c.client.sync_now().unwrap();
    assert!(c.read("evil.md").is_none());
}

/// 轮换开始之后才出现的旧 epoch 版本（来自仍在册的设备）：被接受，但不被重新加密；写入者随后用新密钥重新推送。
#[test]
fn versions_first_seen_during_rotation_are_not_reencrypted() {
    let server = server();
    let (a, _) = first_device(&server);
    let b = Dev::signed_in(&server);
    join(&a, &b);
    let c = Dev::signed_in(&server);
    join(&a, &c);
    for d in [&a, &b, &c] {
        d.client.sync_now().unwrap();
    }
    // C 在不知道轮换开始的情况下写入 epoch 1 的新模板；服务端恰好在 A 追加花名册条目后才交付
    let rid = crypto::random_record_id();
    let ct = craft(&c, &rid, 1, |pt| {
        pt.counter = 1;
        pt.body = Body::Template { file_name: "late.md".into(), source: "late".into() };
    });
    vault_mut(&server, |v| v.inject_after_roster = Some((rid.clone(), fake::Rec { seq: 0, epoch: 1, ct, deleted: false })));
    a.client.revoke_device(b.id(), None).unwrap();
    assert_eq!(a.read("late.md").unwrap(), "late", "来自在册设备、签名有效：接受");
    assert_eq!(vault_mut(&server, |v| v.records[&rid].epoch), 1, "但不重新加密");
    let _ = (b, c);
}

// MARK: M2 —— counter 溢出

#[test]
fn oversized_counter_is_rejected_and_max_counter_does_not_block_rotation() {
    let (server, a, b, phrase) = pair();
    // u64::MAX：超过 2^53，解码时拒绝
    let rid = crypto::random_record_id();
    let ct = craft(&b, &rid, 1, |pt| pt.counter = u64::MAX);
    vault_mut(&server, |v| v.put(&rid, 1, ct, false));
    a.client.sync_now().unwrap();
    assert!(a.has_alert("bad_plaintext"));
    // 恰好 2^53：接受，但之后无法再修改；不会让同步或轮换失败
    let rid2 = crypto::random_record_id();
    let ct = craft(&b, &rid2, 1, |pt| {
        pt.counter = MAX_COUNTER;
        pt.body = Body::Template { file_name: "max.md".into(), source: "m".into() };
    });
    vault_mut(&server, |v| v.put(&rid2, 1, ct, false));
    a.client.sync_now().unwrap();
    assert_eq!(a.read("max.md").unwrap(), "m");
    a.write("max.md", "edited");
    a.client.sync_now().unwrap();
    assert!(a.has_alert("counter_exhausted"));
    a.client.revoke_device(b.id(), Some(phrase)).unwrap();
    assert_eq!(a.core().roster().unwrap().min_epoch(), 2, "轮换完成");
}

// MARK: 解码健壮性

#[test]
fn cbor_huge_length_headers_and_deep_nesting_are_rejected() {
    for bytes in [
        vec![0x5b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00],
        vec![0x9b, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00],
        vec![0x7b, 0x00, 0x00, 0x00, 0x10, 0, 0, 0, 0],
    ] {
        assert!(codec::decode(&bytes).is_err());
    }
    let mut deep = vec![0x81u8; 100_000];
    deep.push(0x00);
    assert!(codec::decode(&deep).is_err());
}

// MARK: L2 —— 重新校验

#[test]
fn reverify_clears_block_only_when_roster_checks_out() {
    let (server, a, b, _) = pair();
    let real = vault_mut(&server, |v| v.roster.clone());
    vault_mut(&server, |v| {
        v.roster.pop();
    });
    assert!(b.client.sync_now().is_err());
    assert!(b.client.status().writes_blocked);
    // 仍被截断：重新校验失败，继续停止写入
    assert!(matches!(b.client.reverify(), Err(SyncError::Security { .. })));
    assert!(b.client.status().writes_blocked);
    // 服务端恢复：重新校验通过，解除停止写入
    vault_mut(&server, |v| v.roster = real);
    let status = b.client.reverify().unwrap();
    assert!(!status.writes_blocked);
    b.write("after.md", "ok");
    b.client.sync_now().unwrap();
    a.client.sync_now().unwrap();
    assert_eq!(a.read("after.md").unwrap(), "ok");
}

// MARK: L3 —— 响应大小上限

#[test]
fn oversized_roster_and_join_lists_are_bounded() {
    let (server, a, b, _) = pair();
    server.world.lock().unwrap().hook = Some(Box::new(|method, path, v: &mut Json| {
        if method == "GET" && path.ends_with("/roster") {
            v["length"] = Json::from(5_000_000u64);
        }
    }));
    assert!(matches!(b.client.sync_now(), Err(SyncError::Security { .. })));
    let commit = b64u(&codec::encode(&codec::map(vec![("v", codec::uint(1)), ("commit", codec::bytes(&[0; 32]))])));
    server.world.lock().unwrap().hook = Some(Box::new(move |method, path, v: &mut Json| {
        if method == "GET" && path.ends_with("/joins") {
            let fake: Vec<Json> = (0..50).map(|i| json!({ "join_id": format!("j_fake{i}"), "commit": commit, "reply": null, "reveal": null, "result": null, "expires_at": 0 })).collect();
            v["joins"] = Json::Array(fake);
        }
    }));
    server.world.lock().unwrap().log.clear();
    let listed = a.client.list_join_requests().unwrap();
    assert!(listed.len() <= 3);
    server.world.lock().unwrap().hook = None;
    let puts = server.world.lock().unwrap().log.iter().filter(|l| l.starts_with("PUT") && l.ends_with("/reply")).count();
    assert!(puts <= 3);
    assert!(listed.iter().all(|r| r.status == JoinRequestStatus::HandledElsewhere));
}

// MARK: L4 —— 不回显服务端文本

#[test]
fn server_error_text_is_not_echoed() {
    let e = crate::sync::net::server_error(400, br#"{"error":"Call +1-800 now!","message":"Your vault is compromised, visit evil.example"}"#);
    let SyncError::Server { code, message, .. } = &e else { panic!() };
    assert_eq!(code, "unknown");
    assert!(!message.contains("evil") && !message.contains("800"));
    let e = crate::sync::net::server_error(409, br#"{"error":"join_in_progress","message":"visit evil.example"}"#);
    let SyncError::Server { code, message, .. } = &e else { panic!() };
    assert_eq!(code, "join_in_progress");
    assert!(!message.contains("evil"));
    assert!(!e.to_string().contains("evil"));
}

/// 服务端构造的记录 ID 不出现在告警文字中；大量不同的坏记录只产生一条告警。
#[test]
fn alerts_do_not_contain_server_record_ids() {
    let (server, a, _b, _) = pair();
    let ct = craft(&a, "CALLMExxxxxxxxxxxxxxxx", 1, |_| {});
    vault_mut(&server, |v| {
        for i in 0..20 {
            let mut bad = ct.clone();
            bad[30] ^= 1;
            v.put(&format!("CALLME{i:016}"), 1, bad, false);
        }
    });
    a.core().state.needs_full = true;
    a.client.sync_now().unwrap();
    let alerts = a.client.status().alerts;
    assert_eq!(alerts.iter().filter(|x| x.code == "tampered").count(), 1);
    assert!(alerts.iter().all(|x| !x.message.contains("CALLME")));
}

// MARK: L5 —— 文件名碰撞（APFS 不区分大小写与规范化形式）

#[test]
fn case_insensitive_file_name_collision_does_not_overwrite() {
    let (server, a, b, _) = pair();
    a.write("Note.md", "original");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    // B（合法成员）写入一条文件名只差大小写的新记录
    let rid = crypto::random_record_id();
    let ct = craft(&b, &rid, 1, |pt| {
        pt.counter = 1;
        pt.body = Body::Template { file_name: "NOTE.md".into(), source: "other".into() };
    });
    vault_mut(&server, |v| v.put(&rid, 1, ct, false));
    a.client.sync_now().unwrap();
    assert_eq!(a.read("Note.md").unwrap(), "original");
    assert!(a.files().iter().any(|f| f.starts_with("NOTE-") && a.read(f).as_deref() == Some("other")), "{:?}", a.files());
    let _ = Plaintext::decode_padded;
}
