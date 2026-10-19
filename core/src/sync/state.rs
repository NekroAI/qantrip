//! 本地同步状态（JSON，存放在数据目录）。不保存任何私钥、VK 或令牌——它们只在平台安全存储中。
//!
//! 保存的内容：已验证的花名册（公开数据）、各记录的 max counter 与内容哈希、模板文件 ↔ record_id 映射、
//! 上次同步的 seq、其他设备状态记录的内容（用于对账）、告警与加入校验码的失败计数。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::record::Kind;

const FILE: &str = "sync-state.json";
const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordMeta {
    pub kind: Kind,
    /// 已接受的最大 counter
    pub counter: u64,
    /// 该版本的内容哈希（base64url）
    pub hash: String,
    pub deleted: bool,
    /// 服务端 seq；None 表示服务端尚无此记录（需要以 base_seq = null 创建）
    pub seq: Option<u64>,
    pub epoch: u32,
    pub writer: String,
    /// 模板记录对应的文件名
    #[serde(default)]
    pub file: Option<String>,
    /// 上次同步时文件内容的哈希，用于发现本地修改
    #[serde(default)]
    pub file_hash: Option<String>,
    /// 需要重新推送（被回滚、或服务端丢失）
    #[serde(default)]
    pub dirty: bool,
    /// 接受该版本时本地花名册的长度：轮换只重新加密在轮换开始（rotate 条目）之前接受的版本（§7）
    #[serde(default)]
    pub accepted_len: u32,
    /// 本机发现模板文件被删除的时间（删除尚未上传时），用于与其他设备的修改比较先后（§5.5）
    #[serde(default)]
    pub deleted_at: Option<i64>,
}

/// 加入时批准设备在认证结果中给出的本机状态记录锚点（§6.2，v0.3）：新设备首次同步必须看到
/// 该设备的状态记录，且不早于此版本，否则视为服务端隐藏。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JoinAnchor {
    pub device: String,
    pub seq: u64,
    pub counter: u64,
    /// 内容哈希（base64url）
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerEntry {
    pub counter: u64,
    pub hash: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredAlert {
    pub code: String,
    pub message: String,
    pub at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalState {
    pub version: u32,
    pub server: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub vault_id: Option<String>,
    /// 已验证的花名册条目（base64url），本地已验证头部即最后一条
    #[serde(default)]
    pub roster: Vec<String>,
    #[serde(default)]
    pub last_seq: u64,
    #[serde(default)]
    pub last_sync: Option<i64>,
    /// 下次同步需要全量对账
    #[serde(default)]
    pub needs_full: bool,
    #[serde(default)]
    pub records: BTreeMap<String, RecordMeta>,
    /// 其他设备的状态记录：device_id → record_id → 条目
    #[serde(default)]
    pub peers: BTreeMap<String, BTreeMap<String, PeerEntry>>,
    #[serde(default)]
    pub alerts: Vec<StoredAlert>,
    /// 停止写入（§4.3 第 2 步）：Some(n) 表示要等到本地花名册长度超过 n 才恢复；
    /// 花名册分叉、截断等无法自动恢复的情况为 `u32::MAX`
    #[serde(default)]
    pub writes_blocked: Option<u32>,
    /// 上次写入的本机状态记录内容的哈希，未变化时不重复写入
    #[serde(default)]
    pub own_state_digest: Option<String>,
    /// 他人发起、尚未完成的轮换：(epoch, 本机首次看到的时间)，用于 24 小时接管
    #[serde(default)]
    pub pending_rotation_seen: Option<(u32, i64)>,
    /// 加入时得到的批准设备状态记录锚点，对账满足后清除
    #[serde(default)]
    pub join_anchor: Option<JoinAnchor>,
    /// 服务端下发的同步策略（最近一次版本检查的值）
    #[serde(default)]
    pub policy: StoredPolicy,
    /// 上次成功的版本检查时间（unix 秒）
    #[serde(default)]
    pub last_check: Option<i64>,
    /// 上次上传本地变化的时间（unix 秒），用于推送节流
    #[serde(default)]
    pub last_auto_push: Option<i64>,
    /// 最近一次版本检查或账号接口返回的用量
    #[serde(default)]
    pub usage: Option<StoredUsage>,
    /// 其他设备发起的待处理加入请求数（最近一次版本检查）
    #[serde(default)]
    pub pending_joins: u32,
    /// 服务端报告本机超出设备数量限制时的上限；之后任一保险库请求成功即清除
    #[serde(default)]
    pub device_limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredPolicy {
    pub check_interval_secs: u32,
    pub panel_check_secs: u32,
    pub min_push_interval_secs: u32,
}

impl Default for StoredPolicy {
    fn default() -> Self {
        StoredPolicy { check_interval_secs: 3600, panel_check_secs: 900, min_push_interval_secs: 900 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredUsage {
    pub bytes: u64,
    pub quota_bytes: u64,
    pub records: u32,
}

impl LocalState {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FILE)
    }

    /// 读取本地状态；后端地址变化时视为新保险库（§9 后端切换），丢弃旧状态。
    pub fn load(dir: &Path, server: &str) -> LocalState {
        let fresh = || LocalState { version: VERSION, server: server.to_string(), ..Default::default() };
        match std::fs::read(Self::path(dir)).ok().and_then(|b| serde_json::from_slice::<LocalState>(&b).ok()) {
            Some(s) if s.server == server && s.version == VERSION => s,
            _ => fresh(),
        }
    }

    /// 原子写入（先写临时文件再改名）。
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("无法创建数据目录：{e}"))?;
        let tmp = dir.join(format!("{FILE}.tmp"));
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, data).map_err(|e| format!("无法写入同步状态：{e}"))?;
        std::fs::rename(&tmp, Self::path(dir)).map_err(|e| format!("无法写入同步状态：{e}"))
    }

    /// 清空保险库相关状态（保留后端地址、账号、同步策略与设备数量限制状态）。
    pub fn reset_vault(&mut self) {
        let server = std::mem::take(&mut self.server);
        let account = self.account.take();
        *self = LocalState { version: VERSION, server, account, policy: self.policy, device_limit: self.device_limit, ..Default::default() };
    }
}
