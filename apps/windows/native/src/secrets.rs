//! 同步密钥的安全存储（核心 `SecretStore` 的 Windows 实现）。
//!
//! 所有键（设备私钥、VK 钥匙环、令牌、加入过程中的临时密钥）合成一个映射，序列化后用 DPAPI
//! （`CryptProtectData`，当前用户范围）加密，写入一个文件（默认 `%APPDATA%\Qantrip\sync\<host>\secrets.bin`）。
//! 与 macOS 把所有密钥合成一个钥匙串条目的做法对应。
//!
//! 选择 DPAPI 文件而不是凭据管理器：凭据管理器的单个凭据上限为 2560 字节（`CRED_MAX_CREDENTIAL_BLOB_SIZE`），
//! 钥匙环随轮换增长，拆成多个凭据又失去"一次读写全部密钥"的原子性；而凭据管理器本身也是用 DPAPI 保护的，
//! 安全性相同（同一用户身份运行的程序都能解密，与 docs/decisions.md D30 中"仅本用户可读的文件"相当）。
//!
//! 非 Windows 平台（仅用于在开发机上编译与测试本 crate）不加密，直接写入映射。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use qantrip_core::sync::{SecretStore, SecretStoreError};

fn fail(message: impl Into<String>) -> SecretStoreError {
    SecretStoreError::Failed { message: message.into() }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// 附加熵：把密文绑定到本应用的用途（其他使用 DPAPI 的程序拿到文件也不能直接当作自己的数据解密）。
#[cfg(windows)]
const ENTROPY: &[u8] = b"ai.nekro.qantrip/sync-secrets/v1";

pub struct DpapiStore {
    path: PathBuf,
    cache: Mutex<Option<BTreeMap<String, Vec<u8>>>>,
}

impl DpapiStore {
    pub fn new(path: impl Into<PathBuf>) -> DpapiStore {
        DpapiStore { path: path.into(), cache: Mutex::new(None) }
    }

    fn load(&self, cache: &mut Option<BTreeMap<String, Vec<u8>>>) -> Result<BTreeMap<String, Vec<u8>>, SecretStoreError> {
        if let Some(map) = cache {
            return Ok(map.clone());
        }
        let map = match std::fs::read(&self.path) {
            Ok(blob) => {
                let plain = unprotect(&blob)?;
                let raw: BTreeMap<String, String> = serde_json::from_slice(&plain).map_err(|e| fail(format!("secrets 格式错误：{e}")))?;
                raw.into_iter().map(|(k, v)| unhex(&v).map(|b| (k, b)).ok_or_else(|| fail("secrets 格式错误"))).collect::<Result<_, _>>()?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(fail(format!("读取 {} 失败：{e}", self.path.display()))),
        };
        *cache = Some(map.clone());
        Ok(map)
    }

    fn save(&self, cache: &mut Option<BTreeMap<String, Vec<u8>>>, map: BTreeMap<String, Vec<u8>>) -> Result<(), SecretStoreError> {
        let raw: BTreeMap<&str, String> = map.iter().map(|(k, v)| (k.as_str(), hex(v))).collect();
        let plain = serde_json::to_vec(&raw).map_err(|e| fail(e.to_string()))?;
        let blob = protect(&plain)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| fail(format!("创建目录失败：{e}")))?;
        }
        // 先写临时文件再替换，避免写到一半时崩溃丢失全部密钥
        let tmp = self.path.with_extension("bin.tmp");
        std::fs::write(&tmp, &blob).map_err(|e| fail(format!("写入失败：{e}")))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| fail(format!("写入失败：{e}")))?;
        *cache = Some(map);
        Ok(())
    }
}

impl SecretStore for DpapiStore {
    fn get(&self, key: String) -> Result<Option<Vec<u8>>, SecretStoreError> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        Ok(self.load(&mut cache)?.get(&key).cloned())
    }

    fn set(&self, key: String, value: Vec<u8>) -> Result<(), SecretStoreError> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.load(&mut cache)?;
        map.insert(key, value);
        self.save(&mut cache, map)
    }

    fn delete(&self, key: String) -> Result<(), SecretStoreError> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.load(&mut cache)?;
        if map.remove(&key).is_none() {
            return Ok(());
        }
        if map.is_empty() {
            // 全部删除（重置设备）后不留下空文件
            let _ = std::fs::remove_file(&self.path);
            *cache = Some(map);
            return Ok(());
        }
        self.save(&mut cache, map)
    }
}

#[cfg(windows)]
mod dpapi {
    use super::*;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData};

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: out 由 CryptProtectData / CryptUnprotectData 以 LocalAlloc 分配
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        unsafe { LocalFree(out.pbData as _) };
        v
    }

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
        let input = blob(plain);
        let entropy = blob(ENTROPY);
        let desc: Vec<u16> = "Qantrip sync".encode_utf16().chain([0]).collect();
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: 所有指针在调用期间有效
        let ok = unsafe { CryptProtectData(&input, desc.as_ptr(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
        if ok == 0 {
            return Err(fail(format!("CryptProtectData 失败（{}）", std::io::Error::last_os_error())));
        }
        Ok(take(out))
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: 同上
        let ok = unsafe {
            CryptUnprotectData(&input, std::ptr::null_mut(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        if ok == 0 {
            return Err(fail(format!("CryptUnprotectData 失败（{}）", std::io::Error::last_os_error())));
        }
        Ok(take(out))
    }
}

#[cfg(windows)]
use dpapi::{protect, unprotect};

#[cfg(not(windows))]
fn protect(plain: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
    Ok(data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("qantrip-secrets-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("secrets.bin");
        let s = DpapiStore::new(&path);
        assert_eq!(s.get("device_keys".into()).unwrap(), None);
        s.set("device_keys".into(), vec![1, 2, 3, 255]).unwrap();
        s.set("access_token".into(), b"token-value".to_vec()).unwrap();
        // 文件中不含明文
        let raw = std::fs::read(&path).unwrap();
        if cfg!(windows) {
            assert!(!raw.windows(11).any(|w| w == b"token-value"), "plaintext on disk");
            assert!(!raw.windows(12).any(|w| w == b"access_token"));
        }
        // 新实例（无缓存）从文件解密读回
        let s2 = DpapiStore::new(&path);
        assert_eq!(s2.get("device_keys".into()).unwrap(), Some(vec![1, 2, 3, 255]));
        assert_eq!(s2.get("access_token".into()).unwrap(), Some(b"token-value".to_vec()));
        s2.delete("device_keys".into()).unwrap();
        s2.delete("missing".into()).unwrap();
        assert_eq!(DpapiStore::new(&path).get("device_keys".into()).unwrap(), None);
        s2.delete("access_token".into()).unwrap();
        assert!(!path.exists(), "empty store leaves no file");
        // 篡改后解密失败，而不是返回错误的数据
        s2.set("k".into(), vec![9; 40]).unwrap();
        let mut raw = std::fs::read(&path).unwrap();
        let n = raw.len();
        raw[n - 5] ^= 0x55;
        std::fs::write(&path, &raw).unwrap();
        if cfg!(windows) {
            assert!(DpapiStore::new(&path).get("k".into()).is_err());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hex_roundtrip() {
        let v: Vec<u8> = (0..=255).collect();
        assert_eq!(unhex(&hex(&v)).unwrap(), v);
        assert!(unhex("abc").is_none());
    }
}
