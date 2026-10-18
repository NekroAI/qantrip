//! 节省请求的同步触发（docs/sync-design.md 5.1，v0.4）：一次请求的空闲检查、按需拉取花名册、按触发原因节流、
//! 服务端下发的策略、设备数量限制与账号设备管理。

use serde_json::{Value as Json, json};

use super::*;
use crate::sync::{SyncError, SyncTrigger};

/// 两台设备，A 写入 x.md，反复同步直到双方都没有待处理的变化。
fn settled_pair() -> (Arc<FakeServer>, Dev, Dev) {
    let server = server();
    let (a, _) = first_device(&server);
    let b = Dev::signed_in(&server);
    join(&a, &b);
    a.write("x.md", "x1");
    for _ in 0..3 {
        a.client.sync_now().unwrap();
        b.client.sync_now().unwrap();
    }
    assert_eq!(b.read("x.md").unwrap(), "x1");
    (server, a, b)
}

/// 清空请求日志后执行 `f`，返回期间发出的请求（"METHOD path"）。
fn requests(server: &Arc<FakeServer>, f: impl FnOnce()) -> Vec<String> {
    server.world.lock().unwrap().log.clear();
    f();
    server.world.lock().unwrap().log.clone()
}

fn vault_mut<R>(server: &Arc<FakeServer>, f: impl FnOnce(&mut fake::Vault) -> R) -> R {
    f(server.world.lock().unwrap().vault.as_mut().unwrap())
}

fn is_push(r: &str) -> bool {
    r.starts_with("POST") && r.ends_with("/records")
}

#[test]
fn idle_check_is_one_request() {
    let (server, a, b) = settled_pair();
    for dev in [&a, &b] {
        let log = requests(&server, || {
            dev.client.sync_now().unwrap();
        });
        assert_eq!(log.len(), 1, "{log:?}");
        assert!(log[0].starts_with("GET ") && log[0].ends_with("/head"));
    }
    // 自己的写入不需要再拉取回来：推送后的下一次检查仍然只有一个请求
    a.write("y.md", "y1");
    let log = requests(&server, || {
        a.client.sync_now().unwrap();
    });
    assert!(log.iter().any(|r| is_push(r)));
    let log = requests(&server, || {
        a.client.sync_now().unwrap();
    });
    assert_eq!(log.len(), 1, "{log:?}");
}

#[test]
fn nothing_changed_means_no_push_and_no_state_rewrite() {
    let (server, a, b) = settled_pair();
    let seq = vault_mut(&server, |v| v.seq);
    for _ in 0..3 {
        let log = requests(&server, || {
            a.client.sync_now().unwrap();
            b.client.maybe_sync(SyncTrigger::Launch).unwrap();
        });
        assert!(!log.iter().any(|r| is_push(r)), "{log:?}");
    }
    assert_eq!(vault_mut(&server, |v| v.seq), seq);
    assert_eq!(a.client.status().next_push_at, None);
}

#[test]
fn roster_is_fetched_only_when_its_length_changes() {
    let (server, a, b) = settled_pair();
    let log = requests(&server, || {
        b.client.sync_now().unwrap();
    });
    assert!(!log.iter().any(|r| r.contains("/roster")));
    // 第三台设备加入：花名册变长，B 的下一次检查拉取花名册
    let c = Dev::signed_in(&server);
    join(&a, &c);
    let log = requests(&server, || {
        b.client.sync_now().unwrap();
    });
    assert!(log.iter().any(|r| r.contains("/roster?since=")), "{log:?}");
    assert_eq!(b.client.devices().unwrap().len(), 3);
    let log = requests(&server, || {
        b.client.sync_now().unwrap();
    });
    assert!(!log.iter().any(|r| r.contains("/roster")), "{log:?}");
}

#[test]
fn truncation_is_detected_from_the_head() {
    // 服务端截断花名册（头部与花名册接口一致）
    let (server, _a, b) = settled_pair();
    vault_mut(&server, |v| {
        v.roster.pop();
    });
    assert!(matches!(b.client.sync_now(), Err(SyncError::Security { .. })));
    assert!(b.has_alert("roster_truncated"));
    assert!(b.client.status().writes_blocked);

    // 只在头部报告较短的长度（花名册接口不变）：同样视为截断
    let (server, _a, b) = settled_pair();
    server.world.lock().unwrap().hook = Some(Box::new(|method, path, v: &mut Json| {
        if method == "GET" && path.ends_with("/head") {
            v["roster_index"] = Json::from(v["roster_index"].as_u64().unwrap() - 1);
        }
    }));
    assert!(matches!(b.client.sync_now(), Err(SyncError::Security { .. })));
    assert!(b.has_alert("roster_truncated"));
    assert!(b.client.status().writes_blocked);
}

#[test]
fn hidden_roster_growth_is_still_caught_by_records() {
    // 服务端在头部隐瞒花名册的增长：记录引用的花名册条目拿不到时，仍然停止写入（头部传播，§4.3 第 2 步）
    let (server, a, b) = settled_pair();
    let c = Dev::signed_in(&server);
    join(&a, &c);
    a.write("z.md", "z");
    a.client.sync_now().unwrap();
    let real = vault_mut(&server, |v| v.roster.len());
    server.world.lock().unwrap().hook = Some(Box::new(move |method, path, v: &mut Json| {
        if method == "GET" && path.ends_with("/head") {
            v["roster_index"] = Json::from(real - 1);
        }
        if method == "GET" && path.ends_with("/roster") {
            v["entries"] = json!([]);
            v["length"] = Json::from(real - 1);
        }
    }));
    let _ = b.client.sync_now();
    assert!(b.has_alert("roster_withheld"));
    assert!(b.client.status().writes_blocked);
}

#[test]
fn triggers_are_throttled_by_policy() {
    let (server, a, _b) = settled_pair();
    // 刚完成上传：本地变化在 15 分钟内不上传，也不发任何请求
    a.write("y.md", "y1");
    let pushed_at = a.core().state.last_auto_push.unwrap();
    let log = requests(&server, || {
        assert_eq!(a.client.maybe_sync(SyncTrigger::LocalChange).unwrap(), None);
    });
    assert!(log.is_empty(), "{log:?}");
    assert_eq!(a.client.status().next_push_at, Some(pushed_at + 900));
    // 定时器与面板：未到检查间隔时不发请求
    let log = requests(&server, || {
        assert_eq!(a.client.maybe_sync(SyncTrigger::Timer).unwrap(), None);
        assert_eq!(a.client.maybe_sync(SyncTrigger::PanelOpened).unwrap(), None);
    });
    assert!(log.is_empty(), "{log:?}");
    // 启动：立即检查，但不上传（节流中）
    let log = requests(&server, || {
        assert!(a.client.maybe_sync(SyncTrigger::Launch).unwrap().is_some());
    });
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(a.client.status().next_push_at.is_some());
    // 到达最小上传间隔：本地变化触发上传
    a.advance(900);
    let report = a.client.maybe_sync(SyncTrigger::LocalChange).unwrap().unwrap();
    assert_eq!(report.pushed, 2, "模板与本机状态记录");
    assert_eq!(a.client.status().next_push_at, None);
    // 手动同步不受节流限制
    a.write("y.md", "y2");
    assert_eq!(a.client.maybe_sync(SyncTrigger::LocalChange).unwrap(), None);
    let report = a.client.maybe_sync(SyncTrigger::Manual).unwrap().unwrap();
    assert!(report.pushed >= 1);
    // 面板：距上次检查达到 900 秒才检查；定时器：3600 秒
    a.advance(899);
    assert_eq!(a.client.maybe_sync(SyncTrigger::PanelOpened).unwrap(), None);
    a.advance(1);
    let log = requests(&server, || {
        assert!(a.client.maybe_sync(SyncTrigger::PanelOpened).unwrap().is_some());
    });
    assert_eq!(log.len(), 1, "{log:?}");
    a.advance(3599);
    assert_eq!(a.client.maybe_sync(SyncTrigger::Timer).unwrap(), None);
    a.advance(1);
    assert!(a.client.maybe_sync(SyncTrigger::Timer).unwrap().is_some());
    // 唤醒总是检查
    assert!(a.client.maybe_sync(SyncTrigger::Wake).unwrap().is_some());
}

#[test]
fn remote_changes_are_pulled_but_upload_waits_for_throttle() {
    let (_server, a, b) = settled_pair();
    a.write("x.md", "x2");
    a.client.sync_now().unwrap();
    b.write("own.md", "mine");
    b.advance(3600);
    // 定时检查拉取远端变化；本机刚上传过，本地变化等到间隔之后
    let now = b.core().now();
    b.core().state.last_auto_push = Some(now - 10);
    let report = b.client.maybe_sync(SyncTrigger::Timer).unwrap().unwrap();
    assert_eq!(report.pulled, 2, "模板与 A 的状态记录");
    assert_eq!(report.pushed, 0);
    assert_eq!(b.read("x.md").unwrap(), "x2");
    assert_eq!(b.client.status().next_push_at, Some(now + 890));
    b.advance(900);
    let report = b.client.maybe_sync(SyncTrigger::LocalChange).unwrap().unwrap();
    assert!(report.pushed >= 1);
    a.client.sync_now().unwrap();
    assert_eq!(a.read("own.md").unwrap(), "mine");
}

#[test]
fn policy_comes_from_the_server_and_is_clamped() {
    let (server, a, _b) = settled_pair();
    let s = a.client.status();
    assert_eq!((s.policy.check_interval_secs, s.policy.panel_check_secs, s.policy.min_push_interval_secs), (3600, 900, 900));
    server.world.lock().unwrap().policy = json!({ "check_interval_secs": 600, "panel_check_secs": 1, "min_push_interval_secs": 0 });
    a.client.sync_now().unwrap();
    let p = a.client.status().policy;
    assert_eq!((p.check_interval_secs, p.panel_check_secs, p.min_push_interval_secs), (600, 60, 0));
    // 不节流时本地变化立即上传
    a.write("now.md", "n");
    assert!(a.client.maybe_sync(SyncTrigger::LocalChange).unwrap().unwrap().pushed >= 1);
    // 重启后保留最近一次的策略
    let mut a = a;
    a.restart();
    assert_eq!(a.client.status().policy.check_interval_secs, 600);
}

#[test]
fn head_reports_usage_and_pending_joins() {
    let (server, a, _b) = settled_pair();
    let s = a.client.status();
    let usage = s.usage.unwrap();
    assert_eq!(usage.quota_bytes, 204_800);
    assert!(usage.records >= 3 && usage.bytes > 0);
    assert_eq!(s.pending_joins, 0);
    let c = Dev::signed_in(&server);
    c.client.start_join().unwrap();
    a.client.maybe_sync(SyncTrigger::Launch).unwrap();
    assert_eq!(a.client.status().pending_joins, 1);
    // 账号接口：还没有保险库的设备也能看到用量
    assert_eq!(c.client.account_usage().unwrap().unwrap().quota_bytes, 204_800);
    assert_eq!(c.client.status().usage.unwrap().quota_bytes, 204_800);
}

#[test]
fn no_vault_or_signed_out_means_no_request() {
    let server = server();
    let d = Dev::signed_in(&server);
    let log = requests(&server, || {
        for t in [SyncTrigger::Launch, SyncTrigger::Timer, SyncTrigger::Wake, SyncTrigger::LocalChange] {
            assert_eq!(d.client.maybe_sync(t).unwrap(), None);
        }
    });
    assert!(log.is_empty());
}

#[test]
fn device_limit_flow() {
    let (server, a, b) = settled_pair();
    server.world.lock().unwrap().max_devices = 2;
    let c = Dev::signed_in(&server);
    // 第三台设备受限：保险库请求返回 DeviceLimit，状态中记下上限
    assert_eq!(c.client.remote_vault_exists(), Err(SyncError::DeviceLimit { max_devices: 2 }));
    assert_eq!(c.client.status().device_limit, Some(2));
    // 设备列表：名称已清理控制与格式字符；只有在本机已验证花名册中的设备 in_vault
    let list = c.client.account_devices().unwrap();
    assert_eq!(list.max_devices, 2);
    assert_eq!(list.devices.len(), 3);
    let me = list.devices.iter().find(|d| d.is_self).unwrap();
    assert!(me.limited && !me.in_vault);
    assert_eq!(me.name, "Test Mac");
    assert_eq!((me.platform.as_str(), me.app_version.as_str()), ("macOS 26.1", "0.4.0"));
    assert!(list.devices.iter().all(|d| !d.in_vault));
    let from_a = a.client.account_devices().unwrap();
    let in_vault: Vec<bool> = from_a.devices.iter().map(|d| d.in_vault).collect();
    assert_eq!(in_vault.iter().filter(|x| **x).count(), 2);
    // C 让 B 退出：B 失去登录，C 的下一次保险库请求成功并清除限制
    let b_grant = list.devices.iter().find(|d| d.device_id == b.id()).unwrap().grant_id.clone();
    c.client.release_device(b_grant).unwrap();
    assert!(c.client.status().signed_in);
    assert!(matches!(b.client.sync_now(), Err(SyncError::NotSignedIn { .. })));
    assert!(c.client.remote_vault_exists().unwrap());
    assert_eq!(c.client.status().device_limit, None);
}

#[test]
fn device_limit_during_sync_and_releasing_self() {
    let (server, a, _b) = settled_pair();
    server.world.lock().unwrap().max_devices = 1;
    // 运营者调低上限不会让已占名额的设备受限；把 A 改为受限来模拟（B 占着唯一的名额）
    let me = a.id();
    server.world.lock().unwrap().grants.iter_mut().filter(|g| g.device_id == me).for_each(|g| g.limited = true);
    assert_eq!(a.client.sync_now(), Err(SyncError::DeviceLimit { max_devices: 1 }));
    assert_eq!(a.client.status().device_limit, Some(1));
    // 受限期间本地变化不单独发请求
    a.write("q.md", "q");
    let log = requests(&server, || {
        assert_eq!(a.client.maybe_sync(SyncTrigger::LocalChange).unwrap(), None);
    });
    assert!(log.is_empty());
    assert_eq!(a.client.status().next_push_at, None);
    // 让本机退出：等同于退出登录
    let me = a.client.account_devices().unwrap().devices.into_iter().find(|d| d.is_self).unwrap();
    a.client.release_device(me.grant_id).unwrap();
    let s = a.client.status();
    assert!(!s.signed_in);
    assert_eq!(s.account, None);
    let log = requests(&server, || {
        assert_eq!(a.client.maybe_sync(SyncTrigger::Launch).unwrap(), None);
    });
    assert!(log.is_empty());
}

#[test]
fn device_label_is_sent_on_sign_in_and_refresh() {
    let server = server();
    let d = Dev::signed_in(&server);
    let label = server.world.lock().unwrap().grants.last().unwrap().label.clone().unwrap();
    assert_eq!(label, ("Test Mac".to_string(), "macOS 26.1".to_string(), "0.4.0".to_string()));
    server.world.lock().unwrap().grants.last_mut().unwrap().label = None;
    server.world.lock().unwrap().grants.last_mut().unwrap().expired = true;
    d.client.remote_vault_exists().unwrap();
    assert!(server.world.lock().unwrap().grants.last().unwrap().label.is_some());
}
