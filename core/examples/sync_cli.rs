//! 同步联调工具：在命令行里扮演一台设备，对真实服务端（如 staging）测试登录、创建、加入、同步。
//! 密钥以明文 JSON 存在 `<dir>/secrets.json`，仅供测试，不要用于真实数据。
//!
//! cargo run --example sync_cli -- <server> <dir> <command> [args]
//! 命令：signin | status | exists | create | join [codefile] | requests | approve <join_id> <code> | approve-auto <codefile>
//!       | sync | maybe <manual|timer|panel|local|launch|wake> | devices | account-devices
//!       | release <grant_id> | usage | revoke <device_id> [phrase] | reverify | recover <phrase…>
//! 环境变量 QANTRIP_DEVICE_NAME 可以指定上报的设备名称（默认 "sync_cli"）。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use qantrip_core::sync::{DeviceLabel, JoinStage, PhraseLanguage, SecretStore, SecretStoreError, SyncClient, SyncTrigger};

struct FileStore {
    path: PathBuf,
    data: Mutex<BTreeMap<String, String>>,
}

impl FileStore {
    fn open(path: PathBuf) -> FileStore {
        let data = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        FileStore { path, data: Mutex::new(data) }
    }
    fn save(&self, data: &BTreeMap<String, String>) {
        std::fs::write(&self.path, serde_json::to_string_pretty(data).unwrap()).unwrap();
    }
}

impl SecretStore for FileStore {
    fn get(&self, key: String) -> Result<Option<Vec<u8>>, SecretStoreError> {
        Ok(self.data.lock().unwrap().get(&key).map(|v| hex_decode(v)))
    }
    fn set(&self, key: String, value: Vec<u8>) -> Result<(), SecretStoreError> {
        let mut d = self.data.lock().unwrap();
        d.insert(key, value.iter().map(|b| format!("{b:02x}")).collect());
        self.save(&d);
        Ok(())
    }
    fn delete(&self, key: String) -> Result<(), SecretStoreError> {
        let mut d = self.data.lock().unwrap();
        d.remove(&key);
        self.save(&d);
        Ok(())
    }
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [server, dir, cmd, rest @ ..] = args.as_slice() else {
        eprintln!("usage: sync_cli <server> <dir> <command> [args]");
        std::process::exit(2);
    };
    let dir = PathBuf::from(dir);
    let templates = dir.join("templates");
    std::fs::create_dir_all(&templates).unwrap();
    let store = Arc::new(FileStore::open(dir.join("secrets.json")));
    let device = DeviceLabel {
        name: std::env::var("QANTRIP_DEVICE_NAME").unwrap_or_else(|_| "sync_cli".into()),
        platform: std::env::consts::OS.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
    };
    let client = SyncClient::new(server.clone(), dir.join("data").to_string_lossy().into(), templates.to_string_lossy().into(), store, device);

    let result: Result<String, qantrip_core::sync::SyncError> = (|| match cmd.as_str() {
        "signin" => {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let redirect = format!("http://127.0.0.1:{}/callback", listener.local_addr().unwrap().port());
            let url = client.begin_sign_in(redirect)?;
            println!("open: {url}");
            if std::env::var_os("QANTRIP_NO_OPEN").is_none() {
                let _ = std::process::Command::new("open").arg(&url).status();
            }
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = conn.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let target = req.split_whitespace().nth(1).unwrap_or("").to_string();
            let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nok, close this tab");
            let query = target.split_once('?').map(|x| x.1).unwrap_or("");
            let get = |k: &str| query.split('&').find_map(|p| p.strip_prefix(&format!("{k}="))).unwrap_or("").to_string();
            client.finish_sign_in(get("code"), get("state"))
        }
        "status" => Ok(format!("{:#?}", client.status())),
        "exists" => client.remote_vault_exists().map(|b| b.to_string()),
        "create" => client.create_vault(PhraseLanguage::English),
        "join" => {
            let p = client.start_join()?;
            println!("join_id {}", p.join_id);
            let mut shown = false;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let p = client.poll_join()?;
                match p.stage {
                    JoinStage::ShowCode if !shown => {
                        let sas = p.sas.unwrap_or_default();
                        println!("SAS {sas}");
                        shown = true;
                        // 校验码写入文件，供另一台设备上的 approve-auto 选出同一个码
                        if let Some(f) = rest.first() {
                            std::fs::write(f, &sas).unwrap();
                        }
                    }
                    JoinStage::Completed => return client.sync_now().map(|r| format!("joined; {r:?}")),
                    JoinStage::Expired => return Ok("expired".into()),
                    _ => {}
                }
            }
        }
        "requests" => client.list_join_requests().map(|r| format!("{r:#?}")),
        "approve" => client.approve_join(rest[0].clone(), rest[1].clone()).map(|_| "approved".into()),
        // 在同一进程里回复并等待校验码：反复列出请求，校验码文件出现后批准
        "approve-auto" => loop {
            let reqs = client.list_join_requests()?;
            if let Some(r) = reqs.iter().find(|r| r.status == qantrip_core::sync::JoinRequestStatus::ReadyForCode)
                && let Ok(code) = std::fs::read_to_string(&rest[0])
            {
                // QANTRIP_WRONG_PICK=1：故意选一个干扰码
                let code = code.trim().to_string();
                let pick = if std::env::var("QANTRIP_WRONG_PICK").is_ok() { r.choices.iter().find(|c| **c != code).cloned().unwrap_or_default() } else { code };
                client.approve_join(r.join_id.clone(), pick)?;
                break Ok(format!("approved; choices were {:?}", r.choices));
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
        "sync" => client.sync_now().map(|r| format!("{r:#?}")),
        "maybe" => {
            let trigger = match rest.first().map(String::as_str) {
                Some("manual") => SyncTrigger::Manual,
                Some("timer") => SyncTrigger::Timer,
                Some("panel") => SyncTrigger::PanelOpened,
                Some("local") => SyncTrigger::LocalChange,
                Some("launch") => SyncTrigger::Launch,
                Some("wake") => SyncTrigger::Wake,
                _ => return Ok("usage: maybe <manual|timer|panel|local|launch|wake>".into()),
            };
            client.maybe_sync(trigger).map(|r| match r {
                Some(r) => format!("{r:#?}"),
                None => format!("no request; next_push_at {:?}", client.status().next_push_at),
            })
        }
        "devices" => client.devices().map(|d| format!("{d:#?}")),
        "account-devices" => client.account_devices().map(|d| format!("{d:#?}")),
        "release" => client.release_device(rest[0].clone()).map(|_| "released".into()),
        "usage" => client.account_usage().map(|u| format!("{u:?}")),
        "revoke" => client.revoke_device(rest[0].clone(), rest.get(1).cloned()).map(|r| format!("{r:?}")),
        "reverify" => client.reverify().map(|s| format!("{s:?}")),
        "recover" => client.recover(rest.join(" ")).map(|_| "recovered".into()),
        other => Ok(format!("unknown command {other}")),
    })();
    match result {
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("error: {e:?}");
            std::process::exit(1);
        }
    }
}
