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
    /// 校验码（E 上输入的 SAS，或 N 上输入的确认码）不符；`remaining` 为冷却前剩余的尝试次数
    SasMismatch { remaining: u32 },
    /// 校验码错误次数过多，冷却中
    Cooldown { seconds: u64 },
    /// 该操作需要恢复短语（§6.4）
    RecoveryRequired { message: String },
    /// 与其他设备的并发操作多次冲突
    Conflict { message: String },
    InvalidInput { message: String },
    InvalidState { message: String },
    /// 本地文件或安全存储读写失败
    Storage { message: String },
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
            | SyncError::Storage { message } => f.write_str(message),
            SyncError::SasMismatch { remaining } => write!(f, "校验码不正确，还可以再试 {remaining} 次"),
            SyncError::Cooldown { seconds } => write!(f, "校验码错误次数过多，请 {} 分钟后再试", seconds.div_ceil(60)),
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
}

#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct SyncReport {
    /// 接受的远端新版本数
    pub pulled: u32,
    /// 推送成功的写入数
    pub pushed: u32,
    /// 产生的冲突副本数
    pub conflicts: u32,
    pub conflict_files: Vec<String>,
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
    /// 显示校验码，请用户在已登录的设备上输入（新设备上没有任何确认按钮）
    EnterCodeOnOtherDevice,
    /// 已登录的设备已批准：请用户在本机输入该设备显示的确认码（调用 `confirm_join`）
    EnterCodeHere,
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
    /// 可以输入校验码
    ReadyForCode,
    /// 由另一台已登录的设备处理
    HandledElsewhere,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct JoinRequest {
    pub join_id: String,
    pub expires_at: i64,
    pub status: JoinRequestStatus,
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
    ) -> SyncClient {
        let api = Api::new(transport, secrets.clone());
        let core = Core::new(api, secrets, server_url, data_dir, template_dir, clock);
        let client = SyncClient {
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
        }
    }

    /// 串行执行一个操作，结束后刷新状态缓存。
    fn run<T>(&self, f: impl FnOnce(&mut Core) -> Result<T, SyncError>) -> Result<T, SyncError> {
        let mut core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        let result = f(&mut core);
        let status = self.compute_status(&core);
        *self.cached.lock().unwrap() = status;
        result
    }
}

#[uniffi::export]
impl SyncClient {
    /// `server_url`：后端地址（如 `https://qantrip.nekro.ai`）；`data_dir`：存放同步状态；`template_dir`：用户模板目录。
    #[uniffi::constructor]
    pub fn new(server_url: String, data_dir: String, template_dir: String, secrets: Arc<dyn SecretStore>) -> Arc<SyncClient> {
        let transport = Arc::new(UreqTransport::new(&server_url));
        Arc::new(SyncClient::with_transport(&server_url, data_dir.into(), template_dir.into(), secrets, transport, Arc::new(now_secs)))
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

    /// 新设备：轮询加入进度。阶段为 `EnterCodeOnOtherDevice` 时显示 `sas`；`EnterCodeHere` 时请用户输入
    /// 已登录设备显示的确认码并调用 `confirm_join`。
    pub fn poll_join(&self) -> Result<JoinProgress, SyncError> {
        self.run(join::poll_join)
    }

    /// 新设备：输入已登录设备批准后显示的确认码。成功后阶段为 `Completed`，随后调用 `sync_now`。
    /// 不符时返回 `SasMismatch`，连续 3 次后返回 `Cooldown` 并取消本次加入。
    pub fn confirm_join(&self, code: String) -> Result<(), SyncError> {
        self.run(|c| join::confirm_join(c, &code))
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

    /// 已登录设备：输入新设备上显示的校验码以批准加入。返回确认码：在本机显示，由用户在新设备上输入。
    pub fn approve_join(&self, join_id: String, sas: String) -> Result<String, SyncError> {
        self.run(|c| join::approve_join(c, &join_id, &sas))
    }

    pub fn reject_join(&self, join_id: String) -> Result<(), SyncError> {
        self.run(|c| join::reject_join(c, &join_id))
    }

    /// 用恢复短语加入（所有设备丢失时）。
    pub fn recover(&self, phrase: String) -> Result<(), SyncError> {
        self.run(|c| join::recover(c, &phrase))
    }

    /// 立即同步：拉取、应用到模板目录、推送本地变化、更新本机状态记录。
    pub fn sync_now(&self) -> Result<SyncReport, SyncError> {
        self.run(|c| c.sync())
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
