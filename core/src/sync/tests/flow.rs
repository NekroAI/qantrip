//! 完整流程：A 创建 → B 加入（选错/选对校验码）→ 双向同步 → 同一模板两边都改时以最后修改为准 → 删除墓碑 →
//! 撤销 B 并轮换 → B 不能再读新数据 → 恢复短语在新设备 C 上恢复。

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
            // 只检查足够长的明文片段：密文是随机字节，两三个字节的片段会偶然出现
            assert!(!s.contains("alpha") && !s.contains("name: alpha"));
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
    assert_eq!(p.stage, JoinStage::CompareCode);
    let sas = p.sas.clone().unwrap();
    assert_eq!(sas.len(), 9);
    let req = a.client.list_join_requests().unwrap().remove(0);
    assert_eq!(req.status, crate::sync::JoinRequestStatus::ReadyForCode);
    assert_eq!(req.choices.len(), 3);
    assert!(req.choices.contains(&sas));
    assert_eq!(a.client.list_join_requests().unwrap()[0].choices, req.choices, "每次显示相同的候选码");
    // B 上确认一致：等待 A 选择
    assert_eq!(b.client.confirm_join().unwrap().stage, JoinStage::WaitingForOtherDevice);
    assert!(!b.client.status().enrolled);
    // A 上选对（小写、无连字符也接受）：B 下一次轮询时加入
    a.client.approve_join(p.join_id, sas.to_lowercase().replace('-', "")).unwrap();
    assert_eq!(b.client.poll_join().unwrap().stage, JoinStage::Completed);
    assert!(b.client.status().enrolled);
    assert_eq!(b.client.devices().unwrap().len(), 2);

    // B 首次同步：合并本地模板；同名不同内容 → 以修改时间较晚的为准（同时修改时取远端），另一版备份
    let r = b.client.sync_now().unwrap();
    assert_eq!(r.replaced, 1);
    assert_eq!(r.replaced_files, vec!["alpha.md".to_string()]);
    assert_eq!(b.read("alpha.md").unwrap(), "---\nname: alpha\n---\nA1");
    assert_eq!(b.read("beta.md").unwrap(), "---\nname: beta\n---\nB1");
    assert_eq!(b.history(), vec!["---\nname: alpha\n---\nB-local".to_string()]);
    assert!(b.client.status().alerts.is_empty(), "{:?}", b.client.status().alerts);

    // A 收到 B 的模板；模板目录里没有多出任何文件
    a.client.sync_now().unwrap();
    assert_eq!(a.read("gamma.md").unwrap(), "---\nname: gamma\n---\nG1");
    assert_eq!(a.files(), vec!["alpha.md", "beta.md", "gamma.md"]);
    assert_eq!(b.files(), vec!["alpha.md", "beta.md", "gamma.md"]);

    // 双方同时修改同一模板：修改时间相同，以先推送者为准，后推送者的版本备份
    a.write("beta.md", "beta by A");
    b.write("beta.md", "beta by B");
    a.client.sync_now().unwrap();
    let r = b.client.sync_now().unwrap();
    assert_eq!(r.replaced, 1);
    assert_eq!(b.read("beta.md").unwrap(), "beta by A");
    assert!(b.history().contains(&"beta by B".to_string()));
    // 内容相同的并发修改直接采用
    a.write("gamma.md", "same");
    b.write("gamma.md", "same");
    a.client.sync_now().unwrap();
    assert_eq!(b.client.sync_now().unwrap().replaced, 0);

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

/// 在 A 上选错候选码：本次加入取消，新设备看到过期；重新发起后可以立即加入（没有冷却）。
#[test]
fn wrong_pick_cancels_join_without_cooldown() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let sas = n.client.poll_join().unwrap().sas.unwrap();
    let choices = a.client.list_join_requests().unwrap()[0].choices.clone();
    let wrong = choices.iter().find(|c| **c != sas).unwrap().clone();
    assert!(matches!(a.client.approve_join(p.join_id.clone(), wrong), Err(SyncError::CodeMismatch { .. })));
    assert_eq!(a.core().roster().unwrap().len(), 2, "没有追加 add_device");
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::Expired);
    assert!(a.client.approve_join(p.join_id, sas).is_err(), "已取消的请求不能再批准");
    join(&a, &n);
}

/// 用户在新设备上点"不一致"：取消本次加入，已登录设备上的请求随之消失。
#[test]
fn mismatch_on_new_device_cancels() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let sas = n.client.poll_join().unwrap().sas.unwrap();
    n.client.cancel_join();
    assert!(a.client.list_join_requests().unwrap().is_empty());
    assert!(a.client.approve_join(p.join_id, sas).is_err());
    assert!(!n.client.status().enrolled);
}

/// A 已选对（追加了 add_device），但新设备没有完成确认就取消：花名册中留下本机的条目。
/// 重新发起加入时不再被拒绝：A 不重复追加，只重新发送钥匙环。
#[test]
fn unfinished_join_can_be_retried() {
    let server = server();
    let (a, _) = first_device(&server);
    a.client.sync_now().unwrap();
    let n = Dev::signed_in(&server);
    n.write("mine.md", "local");
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let sas = n.client.poll_join().unwrap().sas.unwrap();
    a.client.approve_join(p.join_id, sas).unwrap();
    assert_eq!(a.core().roster().unwrap().len(), 3);
    n.client.cancel_join();
    assert!(!n.client.status().enrolled);
    join(&a, &n);
    assert_eq!(a.core().roster().unwrap().len(), 3, "没有重复追加");
    n.client.sync_now().unwrap();
    a.client.sync_now().unwrap();
    assert_eq!(a.read("mine.md").unwrap(), "local");
    assert!(n.client.status().alerts.is_empty(), "{:?}", n.client.status().alerts);
    assert!(a.client.status().alerts.is_empty(), "{:?}", a.client.status().alerts);
}

/// 被移出保险库的设备不能用同一套设备密钥再次加入：两边都给出明确的处理方法。
#[test]
fn removed_device_cannot_rejoin_with_same_keys() {
    let server = server();
    let (a, phrase) = first_device(&server);
    let n = Dev::signed_in(&server);
    join(&a, &n);
    a.client.revoke_device(n.id(), Some(phrase)).unwrap();
    // 本机状态里还有保险库：重置后用新的设备密钥才能加入
    n.core().state.reset_vault();
    n.core().roster = None;
    n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let err = n.client.poll_join().unwrap_err();
    assert!(err.to_string().contains("重置"), "{err}");
    assert!(!n.client.status().enrolled);
    n.client.reset_device().unwrap();
    let url = n.client.begin_sign_in(REDIRECT.into()).unwrap();
    let (code, state) = server.world.lock().unwrap().authorize(&url);
    n.client.finish_sign_in(code, state).unwrap();
    join(&a, &n);
}

/// 新设备先确认一致、已登录设备后选择：结果到达后自动完成；两种顺序都可以。
#[test]
fn confirm_before_or_after_approval() {
    let server = server();
    let (a, _) = first_device(&server);
    let n = Dev::signed_in(&server);
    let p = n.client.start_join().unwrap();
    a.client.list_join_requests().unwrap();
    let sas = n.client.poll_join().unwrap().sas.unwrap();
    assert_eq!(n.client.confirm_join().unwrap().stage, JoinStage::WaitingForOtherDevice);
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::WaitingForOtherDevice);
    assert_eq!(n.client.poll_join().unwrap().sas, Some(sas.clone()), "等待时继续显示校验码");
    a.client.approve_join(p.join_id, sas).unwrap();
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::Completed);
    assert!(n.client.status().enrolled);
    // 另一种顺序（先选择后确认）见 tests::join
    let m = Dev::signed_in(&server);
    join(&a, &m);
}

#[test]
fn join_survives_restart_on_both_sides() {
    let server = server();
    let (mut a, _) = first_device(&server);
    a.client.sync_now().unwrap();
    let mut n = Dev::signed_in(&server);
    n.write("mine.md", "local");
    let p = n.client.start_join().unwrap();
    n.restart();
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::WaitingForApprover);
    a.client.list_join_requests().unwrap();
    // E 回复之后重启：仍可批准（n_E 与锁定的 commit 保存在安全存储中）
    a.restart();
    let shown = n.client.poll_join().unwrap();
    assert_eq!(shown.stage, JoinStage::CompareCode);
    n.restart();
    let again = n.client.poll_join().unwrap();
    assert_eq!(again.stage, JoinStage::CompareCode);
    assert_eq!(again.sas, shown.sas, "重启后显示同一个校验码");
    let choices = a.client.list_join_requests().unwrap()[0].choices.clone();
    a.restart();
    assert_eq!(a.client.list_join_requests().unwrap()[0].choices, choices, "重启后候选码不变");
    a.client.approve_join(p.join_id, shown.sas.unwrap()).unwrap();
    assert!(a.store.0.lock().unwrap().get(crate::sync::join::SECRET_JOIN_E).is_none(), "批准完成后删除 E 的加入状态");
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::CompareCode);
    n.restart();
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::CompareCode);
    assert_eq!(n.client.confirm_join().unwrap().stage, JoinStage::Completed);
    assert!(n.store.0.lock().unwrap().get(crate::sync::join::SECRET_JOIN_N).is_none(), "加入完成后删除 N 的加入状态");
    n.restart();
    assert!(n.client.status().enrolled);
    n.client.sync_now().unwrap();
    a.client.sync_now().unwrap();
    assert_eq!(a.read("mine.md").unwrap(), "local");
    assert!(n.client.status().alerts.is_empty(), "{:?}", n.client.status().alerts);
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

/// 两台设备 A、B 已同步 t.md；两边的时钟相同。
fn synced_pair() -> (Arc<fake::FakeServer>, Dev, Dev) {
    let server = server();
    let (a, _) = first_device(&server);
    let b = Dev::signed_in(&server);
    join(&a, &b);
    a.write("t.md", "base");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    assert_eq!(b.read("t.md").unwrap(), "base");
    (server, a, b)
}

/// 同一模板两边都改：以修改时间较晚的为准，与谁先同步无关；被覆盖的版本进入历史目录。
#[test]
fn later_edit_wins_regardless_of_sync_order() {
    let (_server, a, b) = synced_pair();
    // A 较早修改并先同步；B 较晚修改、后同步：B 的版本保留并推送
    a.advance(10);
    a.write("t.md", "older by A");
    a.client.sync_now().unwrap();
    b.advance(100);
    b.write("t.md", "newer by B");
    let r = b.client.sync_now().unwrap();
    assert_eq!(r.replaced, 1);
    assert_eq!(b.read("t.md").unwrap(), "newer by B");
    assert_eq!(b.history(), vec!["older by A".to_string()]);
    let r = a.client.sync_now().unwrap();
    assert_eq!(r.replaced, 0, "A 上的版本已经同步过，直接覆盖");
    assert_eq!(a.read("t.md").unwrap(), "newer by B");

    // 离线设备带着较早的修改回来：远端较新的版本胜出，离线修改进入历史目录
    a.advance(200);
    b.write("t.md", "offline edit by B");
    a.write("t.md", "latest by A");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    assert_eq!(b.read("t.md").unwrap(), "latest by A");
    assert!(b.history().contains(&"offline edit by B".to_string()));
    a.client.sync_now().unwrap();
    assert_eq!(a.read("t.md").unwrap(), "latest by A");
    assert!(a.client.status().alerts.is_empty(), "{:?}", a.client.status().alerts);
    assert!(b.client.status().alerts.is_empty(), "{:?}", b.client.status().alerts);
}

/// 删除与修改也按先后：较晚的删除生效（被删的修改进入历史目录），较晚的修改让模板"复活"。
#[test]
fn delete_and_edit_follow_last_modified() {
    let (_server, a, b) = synced_pair();
    // B 较早修改并同步；A 较晚删除、后同步：删除生效
    b.advance(10);
    b.write("t.md", "edit by B");
    b.client.sync_now().unwrap();
    a.advance(100);
    a.remove("t.md");
    a.client.sync_now().unwrap();
    assert!(a.read("t.md").is_none());
    assert_eq!(a.history(), vec!["edit by B".to_string()]);
    b.client.sync_now().unwrap();
    assert!(b.read("t.md").is_none());

    // 重新建立 u.md；A 较早删除并同步，B 较晚修改：修改生效，A 上恢复
    a.write("u.md", "u1");
    a.client.sync_now().unwrap();
    b.client.sync_now().unwrap();
    a.remove("u.md");
    a.client.sync_now().unwrap();
    b.advance(200);
    b.write("u.md", "u2 by B");
    b.client.sync_now().unwrap();
    assert_eq!(b.read("u.md").unwrap(), "u2 by B");
    a.client.sync_now().unwrap();
    assert_eq!(a.read("u.md").unwrap(), "u2 by B");
    assert!(a.client.status().alerts.is_empty(), "{:?}", a.client.status().alerts);
    assert!(b.client.status().alerts.is_empty(), "{:?}", b.client.status().alerts);
}

/// 新设备加入时本机已有同名模板：本机的版本较新时保留并上传，云端旧版本进入历史目录。
#[test]
fn newer_local_template_survives_first_sync() {
    let server = server();
    let (a, _) = first_device(&server);
    a.write("p.md", "cloud");
    a.client.sync_now().unwrap();
    let b = Dev::signed_in(&server);
    b.advance(60);
    b.write("p.md", "mine, newer");
    join(&a, &b);
    let r = b.client.sync_now().unwrap();
    assert_eq!(r.replaced, 1);
    assert_eq!(b.read("p.md").unwrap(), "mine, newer");
    assert_eq!(b.history(), vec!["cloud".to_string()]);
    a.client.sync_now().unwrap();
    assert_eq!(a.read("p.md").unwrap(), "mine, newer");
}
