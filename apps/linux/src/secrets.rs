//! 同步密钥的安全存储（核心的 `SecretStore`）。
//!
//! 与 macOS 把全部同步密钥合成一个钥匙串条目相同：所有键合成一个映射，作为一个条目保存在
//! Secret Service（GNOME 密钥环、KWallet 等）中，属性为 `application=ai.nekro.qantrip`、`server=<主机>`。
//! 系统没有可用的 Secret Service 时安全回退：保存为仅本用户可读（0600）的文件，界面中说明。
//! 一旦选定后端就固定使用（已有文件时继续用文件），避免密钥分散在两处。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use qantrip_core::sync::{SecretStore, SecretStoreError};
use secret_service::EncryptionType;
use secret_service::blocking::SecretService;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Keyring,
    File,
}

pub struct LinuxSecretStore {
    host: String,
    backend: Backend,
    file: PathBuf,
    cache: Mutex<Option<BTreeMap<String, Vec<u8>>>>,
}

fn err(e: impl std::fmt::Display) -> SecretStoreError {
    SecretStoreError::Failed { message: e.to_string() }
}

fn encode(map: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let m: BTreeMap<&str, String> = map.iter().map(|(k, v)| (k.as_str(), data_encoding::BASE64.encode(v))).collect();
    serde_json::to_vec(&m).unwrap_or_default()
}

fn decode(data: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let m: BTreeMap<String, String> = serde_json::from_slice(data).unwrap_or_default();
    m.into_iter().filter_map(|(k, v)| data_encoding::BASE64.decode(v.as_bytes()).ok().map(|b| (k, b))).collect()
}

/// 持久保存的集合：默认集合（通常是"登录"密钥环），其次任意非临时集合。
/// GNOME 密钥环的 `session` 集合只保存在内存中，注销后丢失，不能用来保存设备密钥。
fn persistent_collection<'a>(ss: &'a SecretService<'a>) -> Option<secret_service::blocking::Collection<'a>> {
    let is_session = |c: &secret_service::blocking::Collection| c.collection_path.as_str().ends_with("/session");
    if let Some(c) = ss.get_default_collection().ok().filter(|c| !is_session(c)) {
        return Some(c);
    }
    ss.get_all_collections().ok()?.into_iter().find(|c| !is_session(c))
}

/// 系统是否有可用的 Secret Service 与持久保存的集合
fn keyring_available() -> bool {
    match SecretService::connect(EncryptionType::Dh) {
        Ok(ss) => persistent_collection(&ss).is_some(),
        Err(_) => false,
    }
}

impl LinuxSecretStore {
    /// `dir`：该服务端的同步数据目录
    pub fn open(host: &str, dir: &std::path::Path) -> Arc<LinuxSecretStore> {
        let file = dir.join("secrets.json");
        let backend = if file.exists() {
            Backend::File
        } else if keyring_available() {
            Backend::Keyring
        } else {
            Backend::File
        };
        Arc::new(LinuxSecretStore { host: host.to_string(), backend, file, cache: Mutex::new(None) })
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    fn attributes(&self) -> HashMap<&str, &str> {
        HashMap::from([("application", crate::APP_ID), ("server", self.host.as_str())])
    }

    fn load(&self) -> Result<BTreeMap<String, Vec<u8>>, SecretStoreError> {
        match self.backend {
            Backend::File => match std::fs::read(&self.file) {
                Ok(b) => Ok(decode(&b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
                Err(e) => Err(err(e)),
            },
            Backend::Keyring => {
                let ss = SecretService::connect(EncryptionType::Dh).map_err(err)?;
                let collection = persistent_collection(&ss).ok_or_else(|| err("no persistent keyring"))?;
                collection.ensure_unlocked().map_err(err)?;
                let items = collection.search_items(self.attributes()).map_err(err)?;
                let Some(item) = items.into_iter().next() else { return Ok(BTreeMap::new()) };
                item.ensure_unlocked().map_err(err)?;
                Ok(decode(&item.get_secret().map_err(err)?))
            }
        }
    }

    fn save(&self, map: &BTreeMap<String, Vec<u8>>) -> Result<(), SecretStoreError> {
        let data = encode(map);
        match self.backend {
            Backend::File => crate::paths::write_private(&self.file, &data).map_err(err),
            Backend::Keyring => {
                let ss = SecretService::connect(EncryptionType::Dh).map_err(err)?;
                let collection = persistent_collection(&ss).ok_or_else(|| err("no persistent keyring"))?;
                collection.ensure_unlocked().map_err(err)?;
                let label = format!("Qantrip sync ({})", self.host);
                collection.create_item(&label, self.attributes(), &data, true, "application/json").map_err(err)?;
                Ok(())
            }
        }
    }

    fn with_map<T>(&self, f: impl FnOnce(&mut BTreeMap<String, Vec<u8>>) -> (T, bool)) -> Result<T, SecretStoreError> {
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(self.load()?);
        }
        let map = guard.as_mut().unwrap();
        let (out, changed) = f(map);
        if changed && let Err(e) = self.save(map) {
            // 写入失败：丢弃缓存，下次重新读取
            *guard = None;
            return Err(e);
        }
        Ok(out)
    }
}

impl SecretStore for LinuxSecretStore {
    fn get(&self, key: String) -> Result<Option<Vec<u8>>, SecretStoreError> {
        self.with_map(|m| (m.get(&key).cloned(), false))
    }

    fn set(&self, key: String, value: Vec<u8>) -> Result<(), SecretStoreError> {
        self.with_map(|m| {
            let changed = m.get(&key) != Some(&value);
            m.insert(key, value);
            ((), changed)
        })
    }

    fn delete(&self, key: String) -> Result<(), SecretStoreError> {
        self.with_map(|m| {
            let changed = m.remove(&key).is_some();
            ((), changed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_backend_roundtrip_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("qantrip-secrets-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("secrets.json"), b"{}").unwrap();
        let s = LinuxSecretStore::open("example.test", &dir);
        assert_eq!(s.backend(), Backend::File);
        s.set("device_keys".into(), vec![1, 2, 3]).unwrap();
        assert_eq!(s.get("device_keys".into()).unwrap(), Some(vec![1, 2, 3]));
        let mode = std::fs::metadata(dir.join("secrets.json")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // 重新打开后仍能读到
        let s2 = LinuxSecretStore::open("example.test", &dir);
        assert_eq!(s2.get("device_keys".into()).unwrap(), Some(vec![1, 2, 3]));
        s2.delete("device_keys".into()).unwrap();
        assert_eq!(s2.get("device_keys".into()).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
