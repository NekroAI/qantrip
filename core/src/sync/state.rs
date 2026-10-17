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
    #[serde(default)]
    pub sas_failures: u32,
    #[serde(default)]
    pub sas_cooldown_until: Option<i64>,
    /// 他人发起、尚未完成的轮换：(epoch, 本机首次看到的时间)，用于 24 小时接管
    #[serde(default)]
    pub pending_rotation_seen: Option<(u32, i64)>,
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

    /// 清空保险库相关状态（保留后端地址与账号）。
    pub fn reset_vault(&mut self) {
        let server = std::mem::take(&mut self.server);
        let account = self.account.take();
        *self = LocalState { version: VERSION, server, account, ..Default::default() };
    }
}
