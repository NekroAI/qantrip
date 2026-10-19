//! 端到端加密同步客户端（docs/sync-design.md v0.3，项目内称 C2）。
//!
//! - `crypto`：密码学原语与域分离字符串；`codec`：确定性 CBOR 等编码
//! - `roster`：签名花名册；`record`：记录明文；`state`：本地同步状态（JSON，不含任何私钥）
//! - `engine`：记录校验、拉取、推送、冲突与对账；`join`：登录、创建、加入与恢复；`rotate`：撤销与轮换
//! - `net`：服务端接口（apps/cloud/server/api.ts）
//!
//! 对外只暴露 [`SyncClient`]。私钥、VK 钥匙环与令牌通过平台实现的 [`SecretStore`] 保存（macOS 用钥匙串）。

pub mod codec;
pub mod crypto;
pub mod engine;
pub mod files;
pub mod join;
pub mod net;
pub mod record;
pub mod roster;
pub mod rotate;
pub mod state;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use crypto::PhraseLanguage;
use engine::{Clock, Core};
use net::{Api, Transport, UreqTransport};

// MARK: - 错误

#[derive(Debug, Clone, PartialEq, uniffi::Error)]
pub enum SyncError {
    /// 未登录或登录已失效
    NotSignedIn { message: String },
    Network { message: String },
    /// 服务端返回的错误（`code` 为机器可读的错误码；`message` 为本地固定文本，不含服务端返回的文字）
    Server { status: u16, code: String, message: String },
    NoVault { message: String },
    NotEnrolled { message: String },
    /// 安全校验失败（签名、承诺、密文、花名册等）；同时会记录告警
    Security { message: String },
    /// 加入时在已登录设备上选择的校验码与新设备上显示的不一致（本次加入已取消）
    CodeMismatch { message: String },
    /// 该操作需要恢复短语（§6.4）
    RecoveryRequired { message: String },
    /// 与其他设备的并发操作多次冲突
    Conflict { message: String },
    InvalidInput { message: String },
    InvalidState { message: String },
    /// 本地文件或安全存储读写失败
    Storage { message: String },
    /// 此账号同时同步的设备数量已达上限：本机在移除其他设备（`release_device`）之前不能访问保险库
    DeviceLimit { max_devices: u32 },
}

impl SyncError {
    pub fn not_signed_in() -> SyncError {
        SyncError::NotSignedIn { message: "需要登录".into() }
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::NotSignedIn { message }
            | SyncError::Network { message }
            | SyncError::Server { message, .. }
            | SyncError::NoVault { message }
            | SyncError::NotEnrolled { message }
            | SyncError::Security { message }
            | SyncError::RecoveryRequired { message }
            | SyncError::Conflict { message }
            | SyncError::InvalidInput { message }
            | SyncError::InvalidState { message }
            | SyncError::Storage { message }
            | SyncError::CodeMismatch { message } => f.write_str(message),
            SyncError::DeviceLimit { max_devices } => write!(f, "此账号最多同时在 {max_devices} 台设备上同步，请先移除不用的设备"),
        }
    }
}

impl std::error::Error for SyncError {}

#[derive(Debug, Clone, PartialEq, uniffi::Error)]
pub enum SecretStoreError {
    Failed { message: String },
}

impl std::fmt::Display for SecretStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretStoreError::Failed { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for SecretStoreError {}

impl From<uniffi::UnexpectedUniFFICallbackError> for SecretStoreError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        SecretStoreError::Failed { message: e.reason }
    }
}

impl From<SecretStoreError> for SyncError {
    fn from(e: SecretStoreError) -> Self {
        SyncError::Storage { message: format!("安全存储读写失败：{e}") }
    }
}

// MARK: - 平台安全存储

/// 由平台实现的安全存储（macOS：钥匙串）。按 key 存取字节；核心用到的 key：
/// `device_keys`（设备私钥）、`vault_keyring`（VK 钥匙环）、`access_token`、`refresh_token`。
#[uniffi::export(with_foreign)]
pub trait SecretStore: Send + Sync {
    fn get(&self, key: String) -> Result<Option<Vec<u8>>, SecretStoreError>;
    fn set(&self, key: String, value: Vec<u8>) -> Result<(), SecretStoreError>;
    fn delete(&self, key: String) -> Result<(), SecretStoreError>;
}

// MARK: - 对外数据

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SyncAlert {
    /// 机器可读的类别，如 `tampered`、`rollback`、`roster_withheld`
    pub code: String,
    pub message: String,
    pub at: i64,
}

/// 本机的设备信息：登录与刷新令牌时上报给服务端，显示在账号的设备列表中（服务端可见的明文元数据）。
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeviceLabel {
    /// 设备名称（如"MacBook Pro"），最多 64 个字符
    pub name: String,
    /// 平台与系统版本（如"macOS 26.1"），最多 32 个字符
    pub platform: String,
    /// 应用版本，最多 32 个字符
    pub app_version: String,
}

/// 去掉控制字符与格式字符（双向覆盖、零宽字符等），首尾去空白，按字符数截断。
pub fn clean_label(s: &str, max: usize) -> String {
    let kept: String = s.chars().filter(|c| !c.is_control() && !files::is_format_char(*c) && *c != '\u{2028}' && *c != '\u{2029}').collect();
    kept.trim().chars().take(max).collect::<String>().trim().to_string()
}

impl DeviceLabel {
    pub fn sanitized(&self) -> DeviceLabel {
        DeviceLabel { name: clean_label(&self.name, 64), platform: clean_label(&self.platform, 32), app_version: clean_label(&self.app_version, 32) }
    }
}

/// 保险库用量（服务端统计的密文大小与记录数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct SyncUsage {
    pub bytes: u64,
    pub quota_bytes: u64,
    pub records: u32,
}

/// 服务端下发的同步策略（未收到前为默认值 3600 / 900 / 900，之后保存最近一次的值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct SyncPolicy {
    /// 定时检查的最小间隔
    pub check_interval_secs: u32,
    /// 唤起面板时顺带检查的最小间隔
    pub panel_check_secs: u32,
    /// 自动上传本地变化的最小间隔
    pub min_push_interval_secs: u32,
}

/// 触发同步的原因，由 [`SyncClient::maybe_sync`] 结合策略决定是否发请求。
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SyncTrigger {
    /// 用户点击"立即同步"：完整同步，不受节流限制
    Manual,
    /// 应用内的定时器
    Timer,
    /// 唤起面板
    PanelOpened,
    /// 模板目录发生变化
    LocalChange,
    /// 应用启动
    Launch,
    /// 系统从睡眠中唤醒
    Wake,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AccountDevice {
    pub grant_id: String,
    pub device_id: String,
    /// 设备上报的名称（未上报时为空字符串）
    pub name: String,
    pub platform: String,
    pub app_version: String,
    /// unix 秒
    pub created_at: i64,
    pub last_seen_at: i64,
    pub is_self: bool,
    /// 超出设备数量限制，不能访问保险库
    pub limited: bool,
    /// 在本机已验证的花名册中在册（本机没有保险库状态时为 false）
    pub in_vault: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AccountDevices {
    pub max_devices: u32,
    /// 已登录的设备，最近活跃的在前
    pub devices: Vec<AccountDevice>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SyncStatus {
    pub signed_in: bool,
    pub account: Option<String>,
    pub device_id: Option<String>,
    /// 本机已加入保险库（本地有保险库状态）
    pub has_vault: bool,
    /// 本机在花名册中在册
    pub enrolled: bool,
    pub last_sync: Option<i64>,
    /// 因安全原因停止写入
    pub writes_blocked: bool,
    pub alerts: Vec<SyncAlert>,
    /// 正在执行其他操作（返回的是上一次的状态）
    pub busy: bool,
    /// 保险库用量（最近一次版本检查或 `account_usage` 的结果）
    pub usage: Option<SyncUsage>,
    /// 其他设备发起的待处理加入请求数（最近一次版本检查）
    pub pending_joins: u32,
    pub policy: SyncPolicy,
    /// 被节流的本地变化将自动上传的时间（unix 秒）；没有待上传的变化时为 None
    pub next_push_at: Option<i64>,
    /// 服务端报告本机超出设备数量限制时为 Some(上限)；之后任一保险库请求成功即清除
    pub device_limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct SyncReport {
    /// 接受的远端新版本数
    pub pulled: u32,
    /// 推送成功的写入数
    pub pushed: u32,
    /// 同一模板在两台设备上都有修改时，以修改时间较晚的为准（docs/sync-design.md 5.5）：
    /// 被覆盖的那一版备份到历史版本目录（`history_dir`）。这是本次备份的版本数
    pub replaced: u32,
    /// 备份的版本对应的模板文件名
    pub replaced_files: Vec<String>,
    /// 未通过校验而被丢弃的记录数
    pub rejected: u32,
    /// 本次新增的告警数
    pub alerts: u32,
    /// 暂缓的写入数（密钥版本过期，下次同步重试）
    pub deferred: u32,
    pub writes_blocked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JoinStage {
    /// 等待已登录的设备响应
    WaitingForApprover,
    /// 显示校验码 `sas`：请用户在已登录的设备上选出同一个码；选对后下一次 `poll_join` 即完成加入。
    /// 用户放弃时调用 `cancel_join`
    ShowCode,
    Completed,
    Expired,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct JoinProgress {
    pub join_id: String,
    pub stage: JoinStage,
    pub sas: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JoinRequestStatus {
    /// 已回复，等待新设备显示校验码
    WaitingForNewDevice,
    /// 可以选择校验码（见 `JoinRequest::choices`）
    ReadyForCode,
    /// 由另一台已登录的设备处理
    HandledElsewhere,
    /// 新设备的 Qantrip 版本与本机不兼容（加入协议版本不同），请把两台设备升级到同一版本
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct JoinRequest {
    pub join_id: String,
    pub expires_at: i64,
    pub status: JoinRequestStatus,
    /// `ReadyForCode` 时的三个候选校验码（其中一个与新设备上显示的相同），其余状态为空
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeviceInfo {
    pub device_id: String,
    pub is_self: bool,
    /// 加入时的花名册序号（服务端接口不提供加入时间；设备名称待加密设置记录接入后提供）
    pub roster_index: u32,
}

// MARK: - SyncClient

#[derive(uniffi::Object)]
pub struct SyncClient {
    core: Mutex<Core>,
    cached: Mutex<SyncStatus>,
    /// 历史版本目录（不随操作变化，不需要等待其他操作）
    history: PathBuf,
}

fn policy_of(p: &state::StoredPolicy) -> SyncPolicy {
    SyncPolicy { check_interval_secs: p.check_interval_secs, panel_check_secs: p.panel_check_secs, min_push_interval_secs: p.min_push_interval_secs }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl SyncClient {
    /// 使用自定义传输层与时钟（测试用）。
    pub fn with_transport(
        server_url: &str,
        data_dir: PathBuf,
        template_dir: PathBuf,
        secrets: Arc<dyn SecretStore>,
        transport: Arc<dyn Transport>,
        clock: Clock,
        device: DeviceLabel,
    ) -> SyncClient {
        let api = Api::new(transport, secrets.clone(), device);
        let core = Core::new(api, secrets, server_url, data_dir, template_dir, clock);
        let history = core.history_dir();
        let client = SyncClient {
            history,
            core: Mutex::new(core),
            cached: Mutex::new(SyncStatus {
                signed_in: false,
                account: None,
                device_id: None,
                has_vault: false,
                enrolled: false,
                last_sync: None,
                writes_blocked: false,
                alerts: vec![],
                busy: false,
                usage: None,
                pending_joins: 0,
                policy: policy_of(&state::StoredPolicy::default()),
                next_push_at: None,
                device_limit: None,
            }),
        };
        let status = client.compute_status(&client.core.lock().unwrap());
        *client.cached.lock().unwrap() = status;
        client
    }

    fn compute_status(&self, core: &Core) -> SyncStatus {
        SyncStatus {
            signed_in: core.api.signed_in().unwrap_or(false),
            account: core.state.account.clone(),
            device_id: core.device_keys().ok().flatten().map(|k| k.device_id()),
            has_vault: core.state.vault_id.is_some(),
            enrolled: core.enrolled(),
            last_sync: core.state.last_sync,
            writes_blocked: core.state.writes_blocked.is_some(),
            alerts: core.state.alerts.iter().map(|a| SyncAlert { code: a.code.clone(), message: a.message.clone(), at: a.at }).collect(),
            busy: false,
            usage: core.state.usage.map(|u| SyncUsage { bytes: u.bytes, quota_bytes: u.quota_bytes, records: u.records }),
            pending_joins: core.state.pending_joins,
            policy: policy_of(&core.state.policy),
            next_push_at: core.next_push_at(),
            device_limit: core.state.device_limit,
        }
    }

    /// 串行执行一个操作，结束后刷新状态缓存。
    fn run<T>(&self, f: impl FnOnce(&mut Core) -> Result<T, SyncError>) -> Result<T, SyncError> {
        let mut core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        let result = f(&mut core);
        // 设备数量限制：被拒绝时记下上限，之后任一保险库请求成功即清除
        if let Some(event) = core.api.take_limit_event()
            && core.state.device_limit != event
        {
            core.state.device_limit = event;
            let _ = core.save();
        }
        let status = self.compute_status(&core);
        *self.cached.lock().unwrap() = status;
        result
    }
}

#[uniffi::export]
impl SyncClient {
    /// `server_url`：后端地址（如 `https://qantrip.nekro.ai`）；`data_dir`：存放同步状态；`template_dir`：用户模板目录；
    /// `device`：登录与刷新令牌时上报的设备名称、平台与应用版本。
    #[uniffi::constructor]
    pub fn new(server_url: String, data_dir: String, template_dir: String, secrets: Arc<dyn SecretStore>, device: DeviceLabel) -> Arc<SyncClient> {
        let transport = Arc::new(UreqTransport::new(&server_url));
        Arc::new(SyncClient::with_transport(&server_url, data_dir.into(), template_dir.into(), secrets, transport, Arc::new(now_secs), device))
    }

    /// 当前状态（不发网络请求）。其他操作进行中时返回缓存并标记 `busy`。
    pub fn status(&self) -> SyncStatus {
        match self.core.try_lock() {
            Ok(core) => {
                let s = self.compute_status(&core);
                *self.cached.lock().unwrap() = s.clone();
                s
            }
            Err(_) => SyncStatus { busy: true, ..self.cached.lock().unwrap().clone() },
        }
    }

    /// 查询此账号在服务端是否已有保险库（用于决定"创建"还是"加入"）。
    pub fn remote_vault_exists(&self) -> Result<bool, SyncError> {
        self.run(|c| Ok(c.api.vault()?.is_some()))
    }

    /// 开始桌面端登录（PKCE）：返回在浏览器中打开的授权地址。`redirect_uri` 为平台监听的本机回环地址。
    pub fn begin_sign_in(&self, redirect_uri: String) -> Result<String, SyncError> {
        self.run(|c| join::begin_sign_in(c, &redirect_uri))
    }

    /// 用回调收到的授权码与 state 换取令牌。返回 GitHub 用户名。
    pub fn finish_sign_in(&self, code: String, state: String) -> Result<String, SyncError> {
        self.run(|c| join::finish_sign_in(c, &code, &state))
    }

    /// 退出登录：吊销并删除令牌。设备密钥与本地同步状态保留，重新登录后继续使用。
    pub fn sign_out(&self) -> Result<(), SyncError> {
        self.run(|c| {
            c.api.revoke()?;
            c.state.account = None;
            c.save()
        })
    }

    /// 重置本机的同步身份：删除设备密钥、钥匙环、令牌与本地同步状态（不影响模板文件）。
    pub fn reset_device(&self) -> Result<(), SyncError> {
        self.run(|c| {
            let _ = c.api.revoke();
            for key in [engine::SECRET_DEVICE_KEYS, engine::SECRET_KEYRING] {
                c.secrets.delete(key.into())?;
            }
            for key in [join::SECRET_JOIN_N, join::SECRET_JOIN_E] {
                c.secrets.delete(key.into())?;
            }
            c.state.reset_vault();
            c.state.account = None;
            c.roster = None;
            c.join_n = None;
            c.join_e.clear();
            c.save()
        })
    }

    /// 首台设备创建保险库，返回 24 个词的恢复短语（只显示一次）。
    pub fn create_vault(&self, language: PhraseLanguage) -> Result<String, SyncError> {
        self.run(|c| join::create_vault(c, language).map(|p| p.to_string()))
    }

    /// 新设备：发起加入请求。
    pub fn start_join(&self) -> Result<JoinProgress, SyncError> {
        self.run(join::start_join)
    }

    /// 新设备：轮询加入进度。阶段为 `ShowCode` 时显示 `sas`；已登录设备上选对码后，阶段为 `Completed`，
    /// 随后调用 `sync_now`。
    pub fn poll_join(&self) -> Result<JoinProgress, SyncError> {
        self.run(join::poll_join)
    }

    pub fn cancel_join(&self) {
        let _ = self.run(|c| {
            join::cancel_join(c);
            Ok(())
        });
    }

    /// 已登录设备：列出加入请求（会对新请求发送回复）。
    pub fn list_join_requests(&self) -> Result<Vec<JoinRequest>, SyncError> {
        self.run(join::list_join_requests)
    }

    /// 已登录设备：用户从 `choices` 中选出新设备上显示的校验码以批准加入。选错时返回 `CodeMismatch`
    /// 并取消本次加入。
    pub fn approve_join(&self, join_id: String, code: String) -> Result<(), SyncError> {
        self.run(|c| join::approve_join(c, &join_id, &code))
    }

    pub fn reject_join(&self, join_id: String) -> Result<(), SyncError> {
        self.run(|c| join::reject_join(c, &join_id))
    }

    /// 用恢复短语加入（所有设备丢失时）。
    pub fn recover(&self, phrase: String) -> Result<(), SyncError> {
        self.run(|c| join::recover(c, &phrase))
    }

    /// 立即同步（等同于 `maybe_sync(Manual)`）：版本检查、有变化时拉取并应用到模板目录、推送本地变化、更新本机状态记录。
    pub fn sync_now(&self) -> Result<SyncReport, SyncError> {
        self.run(|c| c.sync())
    }

    /// 历史版本目录：同步时被较新版本覆盖的模板备份在这里（不参与同步，保留 30 天）。目录可能还不存在。
    /// 不等待进行中的操作，可以在界面线程调用。
    pub fn history_dir(&self) -> String {
        self.history.to_string_lossy().into_owned()
    }

    /// 按触发原因与服务端下发的策略决定是否同步；返回 None 表示没有发出任何请求。
    /// 平台在定时器、唤起面板、模板目录变化、启动与唤醒时调用；规则见 `SyncTrigger` 与 docs/sync-design.md 5.1。
    pub fn maybe_sync(&self, trigger: SyncTrigger) -> Result<Option<SyncReport>, SyncError> {
        self.run(|c| c.maybe_sync(trigger))
    }

    /// 此账号已登录的设备（含超出设备数量限制的设备）。超出限制的设备也可以调用。
    pub fn account_devices(&self) -> Result<AccountDevices, SyncError> {
        self.run(|c| {
            let d = c.api.account_devices()?;
            let roster = c.roster.clone();
            let devices = d
                .devices
                .into_iter()
                .map(|x| AccountDevice {
                    in_vault: roster.as_ref().is_some_and(|r| r.is_enrolled(&x.device_id)),
                    grant_id: x.grant_id,
                    device_id: clean_label(&x.device_id, 64),
                    name: clean_label(x.device_name.as_deref().unwrap_or(""), 64),
                    platform: clean_label(x.platform.as_deref().unwrap_or(""), 32),
                    app_version: clean_label(x.app_version.as_deref().unwrap_or(""), 32),
                    created_at: x.created_at,
                    last_seen_at: x.last_seen_at,
                    is_self: x.is_self,
                    limited: x.limited,
                })
                .collect();
            Ok(AccountDevices { max_devices: d.max_devices, devices })
        })
    }

    /// 让一台设备退出此账号（吊销其登录授权，腾出名额）。是本机时等同于 `sign_out`。
    /// 这不会把设备移出保险库的花名册；需要时另用 `revoke_device`。
    pub fn release_device(&self, grant_id: String) -> Result<(), SyncError> {
        self.run(|c| {
            if c.api.release_device(&grant_id)? {
                c.api.clear_tokens()?;
                c.state.account = None;
                c.save()?;
            }
            Ok(())
        })
    }

    /// 账号的保险库用量（没有保险库时为 None），用于还没有加入保险库或超出设备数量限制的设备。
    pub fn account_usage(&self) -> Result<Option<SyncUsage>, SyncError> {
        self.run(|c| {
            let usage = c.api.account()?.usage.map(|u| engine::usage_of(&u));
            if usage.is_some() && usage != c.state.usage {
                c.state.usage = usage;
                c.save()?;
            }
            Ok(usage.map(|u| SyncUsage { bytes: u.bytes, quota_bytes: u.quota_bytes, records: u.records }))
        })
    }

    /// 重新校验：重新拉取完整花名册与全部记录并逐条校验。全部通过时才解除"停止写入"，返回最新状态。
    pub fn reverify(&self) -> Result<SyncStatus, SyncError> {
        self.run(|c| {
            c.reverify()?;
            Ok(self.compute_status(c))
        })
    }

    /// 在册设备。
    pub fn devices(&self) -> Result<Vec<DeviceInfo>, SyncError> {
        self.run(|c| {
            let me = c.me()?;
            Ok(c.roster()?
                .current_devices()
                .into_iter()
                .map(|(id, m)| DeviceInfo { is_self: id == me, device_id: id, roster_index: m.added })
                .collect())
        })
    }

    /// 撤销设备并轮换密钥。移除除本机外的最后一台设备时需要恢复短语（§6.4）。
    pub fn revoke_device(&self, device_id: String, recovery_phrase: Option<String>) -> Result<SyncReport, SyncError> {
        self.run(|c| rotate::revoke_device(c, &device_id, recovery_phrase.as_deref()))
    }

    /// 手动轮换密钥。
    pub fn rotate_keys(&self) -> Result<SyncReport, SyncError> {
        self.run(rotate::rotate_keys)
    }

    pub fn dismiss_alerts(&self) {
        let _ = self.run(|c| {
            c.state.alerts.clear();
            c.save()
        });
    }
}
