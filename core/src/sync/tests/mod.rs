//! 同步模块测试：原语测试向量、花名册规则、两台设备完整流程、恶意服务端（设计 §8 逐项）。

mod fake;
mod flow;
mod malicious;
mod review;
mod roster_rules;
mod vectors;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::engine::Core;
use super::{JoinStage, PhraseLanguage, SecretStore, SecretStoreError, SyncClient};
use fake::FakeServer;

pub const SERVER: &str = "https://sync.test";
pub const REDIRECT: &str = "http://127.0.0.1:53682/callback";

#[derive(Default)]
pub struct MemStore(Mutex<HashMap<String, Vec<u8>>>);

impl SecretStore for MemStore {
    fn get(&self, key: String) -> Result<Option<Vec<u8>>, SecretStoreError> {
        Ok(self.0.lock().unwrap().get(&key).cloned())
    }
    fn set(&self, key: String, value: Vec<u8>) -> Result<(), SecretStoreError> {
        self.0.lock().unwrap().insert(key, value);
        Ok(())
    }
    fn delete(&self, key: String) -> Result<(), SecretStoreError> {
        self.0.lock().unwrap().remove(&key);
        Ok(())
    }
}

static DIRS: AtomicUsize = AtomicUsize::new(0);

pub struct Dev {
    pub client: SyncClient,
    pub root: PathBuf,
    pub templates: PathBuf,
    pub clock: Arc<AtomicI64>,
    pub store: Arc<MemStore>,
    pub server: Arc<FakeServer>,
}

impl Dev {
    pub fn new(server: &Arc<FakeServer>) -> Dev {
        let n = DIRS.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("qantrip-sync-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let templates = root.join("templates");
        std::fs::create_dir_all(&templates).unwrap();
        let store = Arc::new(MemStore::default());
        let clock = Arc::new(AtomicI64::new(1_792_281_600));
        let client = Dev::make_client(server, &root, &templates, &store, &clock);
        Dev { client, root, templates, clock, store, server: server.clone() }
    }

    fn make_client(server: &Arc<FakeServer>, root: &std::path::Path, templates: &std::path::Path, store: &Arc<MemStore>, clock: &Arc<AtomicI64>) -> SyncClient {
        let c = clock.clone();
        SyncClient::with_transport(SERVER, root.join("data"), templates.to_path_buf(), store.clone(), server.clone(), Arc::new(move || c.load(Ordering::SeqCst)))
    }

    /// 模拟应用重启：用同一份数据目录与安全存储重新创建 SyncClient（内存状态全部丢弃）。
    pub fn restart(&mut self) {
        self.client = Dev::make_client(&self.server, &self.root, &self.templates, &self.store, &self.clock);
    }

    /// 完整的 PKCE 登录：拼授权地址 → 假官网批准 → 换令牌。
    pub fn signed_in(server: &Arc<FakeServer>) -> Dev {
        let d = Dev::new(server);
        let url = d.client.begin_sign_in(REDIRECT.into()).unwrap();
        let (code, state) = server.world.lock().unwrap().authorize(&url);
        assert_eq!(d.client.finish_sign_in(code, state).unwrap(), "tester");
        d
    }

    pub fn core(&self) -> MutexGuard<'_, Core> {
        self.client.core.lock().unwrap()
    }

    pub fn id(&self) -> String {
        self.core().me().unwrap()
    }

    pub fn write(&self, name: &str, content: &str) {
        std::fs::write(self.templates.join(name), content).unwrap();
    }

    pub fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.templates.join(name)).ok()
    }

    pub fn remove(&self, name: &str) {
        std::fs::remove_file(self.templates.join(name)).unwrap();
    }

    pub fn files(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.templates).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        v.sort();
        v
    }

    pub fn alert_codes(&self) -> Vec<String> {
        self.client.status().alerts.into_iter().map(|a| a.code).collect()
    }

    pub fn has_alert(&self, code: &str) -> bool {
        self.alert_codes().iter().any(|c| c == code)
    }

    pub fn advance(&self, secs: i64) {
        self.clock.fetch_add(secs, Ordering::SeqCst);
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn server() -> Arc<FakeServer> {
    Arc::new(FakeServer::default())
}

/// 创建保险库的第一台设备，返回设备与恢复短语。
pub fn first_device(server: &Arc<FakeServer>) -> (Dev, String) {
    let a = Dev::signed_in(server);
    let phrase = a.client.create_vault(PhraseLanguage::English).unwrap();
    (a, phrase)
}

/// 按 §6.2 走完加入流程（用户在 E 上输入 N 显示的校验码，再在 N 上输入 E 显示的确认码）。
pub fn join(e: &Dev, n: &Dev) {
    let p = n.client.start_join().unwrap();
    assert_eq!(p.stage, JoinStage::WaitingForApprover);
    let reqs = e.client.list_join_requests().unwrap();
    assert_eq!(reqs.len(), 1);
    let p = n.client.poll_join().unwrap();
    assert_eq!(p.stage, JoinStage::EnterCodeOnOtherDevice);
    let sas = p.sas.unwrap();
    e.client.list_join_requests().unwrap();
    let code = e.client.approve_join(p.join_id, sas).unwrap();
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::EnterCodeHere);
    assert!(!n.client.status().enrolled, "确认码输入之前不加入");
    n.client.confirm_join(code).unwrap();
    assert_eq!(n.client.poll_join().unwrap().stage, JoinStage::Completed);
    assert!(n.client.status().enrolled);
}
