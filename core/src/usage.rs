//! 本机使用记录（docs/decisions.md D14）：frecency 排序、最近施放、按前台应用区分。
//!
//! 只记录模板名、规范化的参数（不含附加文本）和应用标识，不记录渲染后的 Prompt。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// frecency 半衰期：7 天前的一次使用权重减半
const HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 3600.0;
const MAX_RECENT: usize = 30;
/// 当前应用内的使用习惯相对全局的权重
const APP_WEIGHT: f64 = 2.0;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct Stat {
    score: f64,
    last: i64,
    count: u32,
}

impl Stat {
    fn decayed(&self, now: i64) -> f64 {
        self.score * 0.5f64.powf((now - self.last).max(0) as f64 / HALF_LIFE_SECS)
    }

    fn bump(&mut self, now: i64) {
        self.score = self.decayed(now) + 1.0;
        self.last = now;
        self.count += 1;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    /// 规范形式，不含附加文本
    pub invocation: String,
    pub template: String,
    pub app: Option<String>,
    pub at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    templates: HashMap<String, Stat>,
    #[serde(default)]
    apps: HashMap<String, HashMap<String, Stat>>,
    #[serde(default)]
    recent: Vec<RecentEntry>,
}

pub struct Usage {
    path: Option<PathBuf>,
    data: Data,
    /// 面板打开时的排序快照：模板名 → 分数（输入过程中不变，避免候选跳动）
    snapshot: HashMap<String, f64>,
    snapshot_app: Option<String>,
}

pub fn now() -> i64 {
    // wasm32-unknown-unknown 没有系统时钟（SystemTime::now 会 panic）；浏览器演示不记录使用
    #[cfg(target_arch = "wasm32")]
    return 0;
    #[cfg(not(target_arch = "wasm32"))]
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Usage {
    pub fn load(path: Option<PathBuf>) -> Usage {
        let data = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut u = Usage { path, data, snapshot: HashMap::new(), snapshot_app: None };
        u.begin_session(None, now());
        u
    }

    /// 面板打开时调用：按当前前台应用计算排序快照。
    pub fn begin_session(&mut self, app: Option<String>, now: i64) {
        let mut scores: HashMap<String, f64> = self.data.templates.iter().map(|(k, s)| (k.clone(), s.decayed(now))).collect();
        if let Some(per_app) = app.as_ref().and_then(|a| self.data.apps.get(a)) {
            for (k, s) in per_app {
                *scores.entry(k.clone()).or_default() += APP_WEIGHT * s.decayed(now);
            }
        }
        self.snapshot = scores;
        self.snapshot_app = app;
    }

    pub fn score(&self, template: &str) -> f64 {
        self.snapshot.get(template).copied().unwrap_or(0.0)
    }

    /// 最近施放：去重后按时间倒序；当前应用里的记录优先。
    pub fn recent(&self, limit: usize) -> Vec<&RecentEntry> {
        let mut items: Vec<&RecentEntry> = self.data.recent.iter().collect();
        let app = self.snapshot_app.as_deref();
        items.sort_by_key(|e| (e.app.as_deref() != app || app.is_none(), -e.at));
        items.truncate(limit);
        items
    }

    pub fn record(&mut self, template: &str, invocation: &str, app: Option<&str>, now: i64) {
        self.data.templates.entry(template.to_string()).or_default().bump(now);
        if let Some(a) = app {
            self.data.apps.entry(a.to_string()).or_default().entry(template.to_string()).or_default().bump(now);
        }
        self.data.recent.retain(|e| e.invocation != invocation);
        self.data.recent.insert(
            0,
            RecentEntry { invocation: invocation.to_string(), template: template.to_string(), app: app.map(str::to_string), at: now },
        );
        self.data.recent.truncate(MAX_RECENT);
        self.save();
    }

    pub fn clear(&mut self) {
        self.data = Data::default();
        self.snapshot.clear();
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(&self.data) {
            // 先写临时文件再改名，避免写到一半时崩溃损坏数据
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frecency_decays_and_prefers_app() {
        let mut u = Usage::load(None);
        let day = 24 * 3600;
        u.record("old", "old", Some("term"), 0);
        u.record("old", "old", Some("term"), 0);
        u.record("new", "new", Some("browser"), 30 * day);
        u.begin_session(None, 30 * day);
        // 两次 30 天前的使用 < 一次刚刚的使用
        assert!(u.score("new") > u.score("old"));
        u.begin_session(Some("term".into()), 30 * day);
        assert!(u.recent(1)[0].template == "old", "当前应用的记录优先");
    }

    #[test]
    fn recent_dedupes() {
        let mut u = Usage::load(None);
        u.record("r", "review --strict", None, 1);
        u.record("z", "zh", None, 2);
        u.record("r", "review --strict", None, 3);
        let r: Vec<_> = u.recent(5).iter().map(|e| e.invocation.clone()).collect();
        assert_eq!(r, vec!["review --strict", "zh"]);
    }
}
