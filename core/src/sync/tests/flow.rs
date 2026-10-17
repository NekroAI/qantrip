//! 完整流程：A 创建 → B 加入（校验码错误/正确）→ 双向同步 → 冲突保留双方 → 删除墓碑 → 撤销 B 并轮换 →
//! B 不能再读新数据 → 恢复短语在新设备 C 上恢复。

use super::*;
use crate::sync::SyncError;
use crate::sync::crypto;

#[test]
fn two_devices_full_lifecycle() {
    let server = server();

    // A：登录、写模板、创建保险库并同步
    let a = Dev::signed_in(&server);
    a.write("alpha.md", "---\nname: alpha\n---\nA1");
    a.write("beta.md", "---\nname: beta\n---\nB1");
    let phrase = a.client.create_vault(PhraseLanguage::English).unwrap();
    assert_eq!(phrase.split(' ').count(), 24);
    let r = a.client.sync_now().unwrap();
    assert_eq!(r.pushed, 3, "两个模板 + 本机状态记录");
    let st = a.client.status();
    assert!(st.signed_in && st.has_vault && st.enrolled && st.alerts.is_empty());

    // 服务端只看到密文：模板内容与文件名都不出现
    {
        let w = server.world.lock().unwrap();
        let v = w.vault.as_ref().unwrap();
        for r in v.records.values() {
            let s = String::from_utf8_lossy(&r.ct);
            assert!(!s.contains("alpha") && !s.contains("A1"));
            assert_eq!(r.ct.len() % 256, 1 + 24 + 16, "填充到 256 字节档位");
        }
    }

    // B：登录，本地已有一个同名但内容不同的模板和一个独有模板
    let b = Dev::signed_in(&server);
    b.write("alpha.md", "---\nname: alpha\n---\nB-local");
    b.write("gamma.md", "---\nname: gamma\n---\nG1");
    let p = b.client.start_join().unwrap();
    assert_eq!(p.stage, JoinStage::WaitingForApprover);
    assert!(b.client.sync_now().is_err(), "加入完成前不能同步");
    let reqs = a.client.list_join_requests().unwrap();
    assert_eq!(reqs[0].status, crate::sync::JoinRequestStatus::WaitingForNewDevice);
    let p = b.client.poll_join().unwrap();
    assert_eq!(p.stage, JoinStage::EnterCodeOnOtherDevice);
    let sas = p.sas.clone().unwrap();
    assert_eq!(sas.len(), 9);
    assert_eq!(a.client.list_join_requests().unwrap()[0].status, crate::sync::JoinRequestStatus::ReadyForCode);
    // 输错一次：计数，不完成加入
    assert_eq!(a.client.approve_join(p.join_id.clone(), "AAAA-AAAA".into()), Err(SyncError::SasMismatch { remaining: 2 }));
    assert_eq!(b.client.poll_join().unwrap().stage, JoinStage::EnterCodeOnOtherDevice);
    // 输入正确（小写、无连字符也接受）
    a.client.approve_join(p.join_id, sas.to_lowercase().replace('-', "")).unwrap();
    assert_eq!(b.client.poll_join().unwrap().stage, JoinStage::Completed);
    assert!(b.client.status().enrolled);
    assert_eq!(b.client.devices().unwrap().len(), 2);

    // B 首次同步：合并本地模板；同名不同内容 → 远端版本写入原文件，本地版本另存为冲突副本
    let r = b.client.sync_now().unwrap();
    assert_eq!(r.conflicts, 1);
    assert_eq!(b.read("alpha.md").unwrap(), "---\nname: alpha\n---\nA1");
    assert_eq!(b.read("beta.md").unwrap(), "---\nname: beta\n---\nB1");
    let conflict = &r.conflict_files[0];
    assert!(conflict.starts_with("alpha.conflict-") && conflict.ends_with(".md"));
    assert_eq!(b.read(conflict).unwrap(), "---\nname: alpha\n---\nB-local");
    assert!(b.client.status().alerts.is_empty(), "{:?}", b.client.status().alerts);

    // A 收到 B 的模板；冲突副本不同步
    a.client.sync_now().unwrap();
    assert_eq!(a.read("gamma.md").unwrap(), "---\nname: gamma\n---\nG1");
    assert!(a.files().iter().all(|f| !f.contains(".conflict-")));

    // 双方并发修改同一模板：先推送者胜出，后推送者的版本另存为冲突副本
    a.write("beta.md", "beta by A");
    b.write("beta.md", "beta by B");
    a.client.sync_now().unwrap();
    let r = b.client.sync_now().unwrap();
    assert_eq!(r.conflicts, 1);
    assert_eq!(b.read("beta.md").unwrap(), "beta by A");
    assert_eq!(b.read(&r.conflict_files[0]).unwrap(), "beta by B");
    // 内容相同的并发修改直接采用
    a.write("gamma.md", "same");
    b.write("gamma.md", "same");
    a.client.sync_now().unwrap();
    assert_eq!(b.client.sync_now().unwrap().conflicts, 0);

    // 删除：墓碑同步到另一台设备
    a.remove("gamma.md");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    assert!(b.read("gamma.md").is_none());
    {
        let w = server.world.lock().unwrap();
        assert!(w.vault.as_ref().unwrap().records.values().any(|r| r.deleted));
    }
    // 远端改名
    a.write("delta.md", "D1");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    assert_eq!(b.read("delta.md").unwrap(), "D1");
    a.client.sync_now().unwrap();
    assert!(a.client.status().alerts.is_empty(), "{:?}", a.client.status().alerts);
    assert!(b.client.status().alerts.is_empty(), "{:?}", b.client.status().alerts);

    // 撤销 B：移除最后一台其他设备需要恢复短语（§6.4）
    let b_id = b.id();
    assert!(matches!(a.client.revoke_device(b_id.clone(), None), Err(SyncError::RecoveryRequired { .. })));
    let wrong = crypto::phrase_from_entropy(&[1; 32], PhraseLanguage::English).to_string();
    assert!(matches!(a.client.revoke_device(b_id.clone(), Some(wrong)), Err(SyncError::InvalidInput { .. })));
    a.client.revoke_device(b_id.clone(), Some(phrase.clone())).unwrap();
    assert_eq!(a.client.devices().unwrap().len(), 1);
    {
        let core = a.core();
        let roster = core.roster().unwrap();
        assert_eq!(roster.max_epoch(), 2);
        assert_eq!(roster.min_epoch(), 2, "rotate_done 已追加");
        let w = server.world.lock().unwrap();
        let v = w.vault.as_ref().unwrap();
        assert_eq!(v.min_epoch, 2);
        assert!(v.records.iter().all(|(id, r)| r.epoch == 2 || *id == crypto::state_record_id(&v.id, &b_id)), "除 B 的状态记录外全部重新加密");
        assert!(!v.envelopes.contains_key(&(b_id.clone(), 2)), "不为被撤销设备写新信封");
        assert!(v.envelopes.contains_key(&("recovery".to_string(), 2)));
        assert_eq!(core.keyring().unwrap().epochs(), vec![2], "旧 VK 已丢弃");
    }

    // A 写入新数据；B 不在册，无法同步，也拿不到新 VK
    a.write("secret.md", "after revoke");
    a.client.sync_now().unwrap();
    assert!(matches!(b.client.sync_now(), Err(SyncError::NotEnrolled { .. })));
    assert!(b.has_alert("revoked"));
    assert!(b.core().keyring().unwrap().get(2).is_none());
    assert!(b.read("secret.md").is_none());

    // 恢复短语：新设备 C 恢复并取回全部数据
    let c = Dev::signed_in(&server);
    assert!(matches!(c.client.recover("not a phrase".into()), Err(SyncError::InvalidInput { .. })));
    c.client.recover(phrase.to_uppercase()).unwrap();
    assert!(c.client.status().enrolled);
    c.client.sync_now().unwrap();
    assert_eq!(c.read("secret.md").unwrap(), "after revoke");
    assert_eq!(c.read("alpha.md").unwrap(), "---\nname: alpha\n---\nA1");
    assert_eq!(c.read("beta.md").unwrap(), "beta by A");
    assert!(c.read("gamma.md").is_none());
    assert!(c.client.status().alerts.is_empty(), "{:?}", c.client.status().alerts);
    // A 与 C 继续双向同步
    c.write("from-c.md", "C!");
    c.client.sync_now().unwrap();
    a.client.sync_now().unwrap();
    assert_eq!(a.read("from-c.md").unwrap(), "C!");
    assert!(a.client.status().alerts.is_empty(), "{:?}", a.client.status().alerts);
}

#[test]
fn sas_failures_cool_down() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    n.client.poll_join().unwrap();
    assert_eq!(a.client.approve_join(p.join_id.clone(), "AAAAAAAA".into()), Err(SyncError::SasMismatch { remaining: 2 }));
    assert_eq!(a.client.approve_join(p.join_id.clone(), "AAAAAAAA".into()), Err(SyncError::SasMismatch { remaining: 1 }));
    assert_eq!(a.client.approve_join(p.join_id.clone(), "AAAAAAAA".into()), Err(SyncError::Cooldown { seconds: 600 }));
    // 请求被取消，新设备看到过期
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::Expired);
    // 冷却期间不能批准新请求
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let sas = n.client.poll_join().unwrap().sas.unwrap();
    assert!(matches!(a.client.approve_join(p.join_id.clone(), sas.clone()), Err(SyncError::Cooldown { .. })));
    a.advance(601);
    a.client.approve_join(p.join_id, sas).unwrap();
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::Completed);
}

#[test]
fn rejected_join_expires_on_new_device() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.reject_join(p.join_id).unwrap();
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::Expired);
    assert!(!n.client.status().enrolled);
}

#[test]
fn token_refresh_and_sign_out() {
    let server = server();
    let (a, _) = first_device(&server);
    a.client.sync_now().unwrap();
    // 访问令牌过期：自动用刷新令牌换新令牌后重试
    server.world.lock().unwrap().grants.iter_mut().for_each(|g| g.expired = true);
    a.client.sync_now().unwrap();
    assert!(server.world.lock().unwrap().log.iter().any(|l| l == "POST /api/v1/oauth/token"));
    // 刷新令牌被吊销：转为未登录
    server.world.lock().unwrap().grants.iter_mut().for_each(|g| {
        g.expired = true;
        g.revoked = false;
        g.refresh = "qtr_revoked".into();
    });
    assert!(matches!(a.client.sync_now(), Err(SyncError::NotSignedIn { .. })));
    assert!(!a.client.status().signed_in);
    // 本地数据与身份保留：重新登录后继续同步
    let url = a.client.begin_sign_in(REDIRECT.into()).unwrap();
    let (code, state) = server.world.lock().unwrap().authorize(&url);
    assert!(matches!(a.client.finish_sign_in(code.clone(), "forged".into()), Err(SyncError::Security { .. })));
    let url = a.client.begin_sign_in(REDIRECT.into()).unwrap();
    let (code, state2) = server.world.lock().unwrap().authorize(&url);
    let _ = state;
    a.client.finish_sign_in(code, state2).unwrap();
    a.client.sync_now().unwrap();
    a.client.sign_out().unwrap();
    assert!(!a.client.status().signed_in);
    assert!(a.client.status().enrolled);
}

#[test]
fn manual_rotation_and_resume() {
    let server = server();
    let (a, _) = first_device(&server);
    let b = Dev::signed_in(&server);
    join(&a, &b);
    a.write("x.md", "x");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    // B 手动轮换；A 拿到新 VK 后继续同步
    b.client.rotate_keys().unwrap();
    a.write("y.md", "y");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    assert_eq!(b.read("y.md").unwrap(), "y");
    assert_eq!(a.core().roster().unwrap().min_epoch(), 2);
    assert!(a.client.status().alerts.is_empty(), "{:?}", a.client.status().alerts);
    assert!(b.client.status().alerts.is_empty(), "{:?}", b.client.status().alerts);
}

#[test]
fn stalled_rotation_is_taken_over_after_24h() {
    let server = server();
    let (a, _) = first_device(&server);
    let b = Dev::signed_in(&server);
    join(&a, &b);
    a.write("x.md", "x");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    // A 追加 rotate 后"崩溃"：不写信封、不完成
    {
        let mut core = a.core();
        let vault = core.vault().unwrap();
        let keys = core.keys().unwrap();
        let me = keys.device_id();
        core.append_roster(|r| {
            let (idx, prev) = r.head().unwrap();
            let op = crate::sync::roster::Op::Rotate { epoch: 2, vk_commit: crypto::vk_commit(&[7; 32], &vault, 2) };
            Ok(Some(crate::sync::roster::Entry::signed(&vault, idx + 1, prev, op, crate::sync::roster::Signer::Device(me.clone()), |m| keys.roster_sign(m)).encode()))
        })
        .unwrap();
    }
    // B 拿不到 epoch 2 的 VK：不写入，等待
    b.write("z.md", "z");
    b.client.sync_now().unwrap();
    assert!(server.world.lock().unwrap().vault.as_ref().unwrap().records.len() == 3, "x + 两个状态记录，z 未写入");
    assert!(matches!(b.client.rotate_keys(), Err(SyncError::InvalidState { .. })));
    // 24 小时后接管：发起 epoch 3 并完成
    b.advance(24 * 3600 + 1);
    b.client.sync_now().unwrap();
    assert!(b.has_alert("rotation_takeover"));
    let roster = b.core().roster().unwrap().clone();
    assert_eq!((roster.max_epoch(), roster.min_epoch()), (3, 3));
    b.client.sync_now().unwrap();
    a.client.sync_now().unwrap();
    assert_eq!(a.read("z.md").unwrap(), "z");
}
