//! 同步客户端（核心 `SyncClient`）的 C ABI。
//!
//! 只有三个入口：`qt_sync_new` / `qt_sync_free` / `qt_sync_call`。`qt_sync_call(client, op, args)` 按操作名分派，
//! 参数与返回值都是 JSON：成功 `{"ok":true,"value":…}`，失败 `{"ok":false,"error":{"kind":"network",…}}`。
//! 所有操作都是阻塞的（可能发网络请求），外壳必须在后台线程调用；核心内部串行执行，可从任意线程调用。

use std::ffi::c_char;
use std::sync::Arc;

use qantrip_core::sync::{
    DeviceInfo, JoinProgress, JoinRequest, JoinRequestStatus, JoinStage, PhraseLanguage, SyncClient, SyncError, SyncReport, SyncStatus,
};
use serde_json::{Value, json};

use crate::secrets::DpapiStore;
use crate::{guard, opt_str, out, str_or_empty};

fn status_json(s: &SyncStatus) -> Value {
    json!({
        "signed_in": s.signed_in,
        "account": s.account,
        "device_id": s.device_id,
        "has_vault": s.has_vault,
        "enrolled": s.enrolled,
        "last_sync": s.last_sync,
        "writes_blocked": s.writes_blocked,
        "alerts": s.alerts.iter().map(|a| json!({ "code": a.code, "message": a.message, "at": a.at })).collect::<Vec<_>>(),
        "busy": s.busy,
    })
}

fn report_json(r: &SyncReport) -> Value {
    json!({
        "pulled": r.pulled,
        "pushed": r.pushed,
        "conflicts": r.conflicts,
        "conflict_files": r.conflict_files,
        "rejected": r.rejected,
        "alerts": r.alerts,
        "deferred": r.deferred,
        "writes_blocked": r.writes_blocked,
    })
}

fn stage_name(s: JoinStage) -> &'static str {
    match s {
        JoinStage::WaitingForApprover => "waiting_for_approver",
        JoinStage::EnterCodeOnOtherDevice => "enter_code_on_other_device",
        JoinStage::EnterCodeHere => "enter_code_here",
        JoinStage::Completed => "completed",
        JoinStage::Expired => "expired",
    }
}

fn progress_json(p: &JoinProgress) -> Value {
    json!({ "join_id": p.join_id, "stage": stage_name(p.stage), "sas": p.sas })
}

fn request_json(r: &JoinRequest) -> Value {
    let status = match r.status {
        JoinRequestStatus::WaitingForNewDevice => "waiting_for_new_device",
        JoinRequestStatus::ReadyForCode => "ready_for_code",
        JoinRequestStatus::HandledElsewhere => "handled_elsewhere",
    };
    json!({ "join_id": r.join_id, "expires_at": r.expires_at, "status": status })
}

fn device_json(d: &DeviceInfo) -> Value {
    json!({ "device_id": d.device_id, "is_self": d.is_self, "roster_index": d.roster_index })
}

/// 错误的机器可读形式；外壳按 `kind` 给出双语文字（对应 macOS SyncService.swift 的 `describe`）。
pub fn error_json(e: &SyncError) -> Value {
    let msg = e.to_string();
    match e {
        SyncError::NotSignedIn { .. } => json!({ "kind": "not_signed_in", "message": msg }),
        SyncError::Network { .. } => json!({ "kind": "network", "message": msg }),
        SyncError::Server { status, code, .. } => json!({ "kind": "server", "status": status, "code": code, "message": msg }),
        SyncError::NoVault { .. } => json!({ "kind": "no_vault", "message": msg }),
        SyncError::NotEnrolled { .. } => json!({ "kind": "not_enrolled", "message": msg }),
        SyncError::Security { .. } => json!({ "kind": "security", "message": msg }),
        SyncError::SasMismatch { remaining } => json!({ "kind": "sas_mismatch", "remaining": remaining, "message": msg }),
        SyncError::Cooldown { seconds } => json!({ "kind": "cooldown", "seconds": seconds, "message": msg }),
        SyncError::RecoveryRequired { .. } => json!({ "kind": "recovery_required", "message": msg }),
        SyncError::Conflict { .. } => json!({ "kind": "conflict", "message": msg }),
        SyncError::InvalidInput { .. } => json!({ "kind": "invalid_input", "message": msg }),
        SyncError::InvalidState { .. } => json!({ "kind": "invalid_state", "message": msg }),
        SyncError::Storage { .. } => json!({ "kind": "storage", "message": msg }),
    }
}

fn invalid(message: &str) -> SyncError {
    SyncError::InvalidInput { message: message.into() }
}

fn arg(args: &Value, key: &str) -> Result<String, SyncError> {
    args.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| invalid(&format!("缺少参数 {key}")))
}

/// 执行一个操作。未知操作返回 InvalidInput。
pub fn dispatch(c: &SyncClient, op: &str, args: &Value) -> Result<Value, SyncError> {
    Ok(match op {
        "status" => status_json(&c.status()),
        "remote_vault_exists" => json!(c.remote_vault_exists()?),
        "begin_sign_in" => json!(c.begin_sign_in(arg(args, "redirect_uri")?)?),
        "finish_sign_in" => json!(c.finish_sign_in(arg(args, "code")?, arg(args, "state")?)?),
        "sign_out" => {
            c.sign_out()?;
            Value::Null
        }
        "reset_device" => {
            c.reset_device()?;
            Value::Null
        }
        "create_vault" => {
            let lang = if arg(args, "language").unwrap_or_default().starts_with("zh") { PhraseLanguage::ChineseSimplified } else { PhraseLanguage::English };
            json!(c.create_vault(lang)?)
        }
        "start_join" => progress_json(&c.start_join()?),
        "poll_join" => progress_json(&c.poll_join()?),
        "confirm_join" => {
            c.confirm_join(arg(args, "code")?)?;
            Value::Null
        }
        "cancel_join" => {
            c.cancel_join();
            Value::Null
        }
        "list_join_requests" => Value::Array(c.list_join_requests()?.iter().map(request_json).collect()),
        "approve_join" => json!(c.approve_join(arg(args, "join_id")?, arg(args, "sas")?)?),
        "reject_join" => {
            c.reject_join(arg(args, "join_id")?)?;
            Value::Null
        }
        "recover" => {
            c.recover(arg(args, "phrase")?)?;
            Value::Null
        }
        "sync_now" => report_json(&c.sync_now()?),
        "devices" => Value::Array(c.devices()?.iter().map(device_json).collect()),
        "revoke_device" => {
            let phrase = args.get("recovery_phrase").and_then(Value::as_str).map(str::to_string);
            report_json(&c.revoke_device(arg(args, "device_id")?, phrase)?)
        }
        "rotate_keys" => report_json(&c.rotate_keys()?),
        "reverify" => status_json(&c.reverify()?),
        "dismiss_alerts" => {
            c.dismiss_alerts();
            Value::Null
        }
        _ => return Err(invalid(&format!("未知操作 {op}"))),
    })
}

fn client<'a>(p: *const SyncClient) -> Option<&'a SyncClient> {
    // SAFETY: p 来自 qt_sync_new，且尚未被 qt_sync_free 释放
    unsafe { p.as_ref() }
}

/// 创建同步客户端。`server_url`：后端地址；`data_dir`：同步状态目录；`template_dir`：用户模板目录；
/// `secrets_path`：DPAPI 加密的密钥文件。失败返回空指针。
#[unsafe(no_mangle)]
pub extern "C" fn qt_sync_new(
    server_url: *const c_char,
    data_dir: *const c_char,
    template_dir: *const c_char,
    secrets_path: *const c_char,
) -> *const SyncClient {
    guard(std::ptr::null(), || {
        let store = Arc::new(DpapiStore::new(str_or_empty(secrets_path)));
        Arc::into_raw(SyncClient::new(str_or_empty(server_url), str_or_empty(data_dir), str_or_empty(template_dir), store))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn qt_sync_free(c: *const SyncClient) {
    if !c.is_null() {
        // SAFETY: c 来自 Arc::into_raw
        drop(unsafe { Arc::from_raw(c) });
    }
}

/// 执行同步操作（阻塞）。`args`：JSON 对象，可为空指针。返回 `{"ok":true,"value":…}` 或 `{"ok":false,"error":{…}}`。
#[unsafe(no_mangle)]
pub extern "C" fn qt_sync_call(c: *const SyncClient, op: *const c_char, args: *const c_char) -> *mut c_char {
    let Some(c) = client(c) else { return std::ptr::null_mut() };
    let op = str_or_empty(op);
    let args: Value = opt_str(args).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let result = guard(Err(SyncError::InvalidState { message: "内部错误（panic）".into() }), || dispatch(c, &op, &args));
    let v = match result {
        Ok(value) => json!({ "ok": true, "value": value }),
        Err(e) => json!({ "ok": false, "error": error_json(&e) }),
    };
    out(Some(v.to_string()))
}
