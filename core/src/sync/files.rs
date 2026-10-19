//! 模板目录（D11：一个模板一个 `*.md` 文件）的读写，以及被覆盖版本的历史目录。
//! 0.4 及更早版本生成的冲突副本（`*.conflict-*.md`）不参与同步。
//!
//! - 文件名比较按 [`name_key`]（NFC 规范化后转小写），与 APFS / NTFS 默认的大小写不敏感、规范化不敏感一致，
//!   避免两条记录落到同一个文件上互相覆盖。
//! - 读取不跟随符号链接（符号链接与非普通文件一律跳过），防止把模板目录外的文件同步出去。

use std::io::Read;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use unicode_normalization::UnicodeNormalization;

use super::codec;
use super::crypto;

const CONFLICT_MARK: &str = ".conflict-";
/// 历史版本的保留时间
pub const HISTORY_KEEP_SECS: i64 = 30 * 86_400;

/// Unicode 一般类别 Cf（格式字符：双向控制、零宽字符等，Unicode 16）。文件名中出现它们可以伪装扩展名或名称。
pub fn is_format_char(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2 | 0x180E
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB
        | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

/// 远端记录中的文件名只能是模板目录下的普通 `.md` 文件名，防止路径穿越与名称伪装。
pub fn valid_file_name(name: &str) -> bool {
    name.ends_with(".md")
        && name.len() > 3
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(|c| c.is_control() || is_format_char(c))
        && !name.contains(CONFLICT_MARK)
}

/// 文件名的比较键：NFC 规范化后转小写。
pub fn name_key(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}

/// 参与同步的模板文件名（排序）。
pub fn list(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| valid_file_name(n))
        .collect();
    out.sort();
    out
}

/// 读取普通文件；符号链接、目录等返回 None（不跟随符号链接）。
pub fn read(dir: &Path, name: &str) -> Option<Vec<u8>> {
    let path = dir.join(name);
    let link = std::fs::symlink_metadata(&path).ok()?;
    if !link.file_type().is_file() {
        return None;
    }
    let mut f = std::fs::File::open(&path).ok()?;
    let opened = f.metadata().ok()?;
    // 检查与打开之间被换成符号链接或其他文件时放弃
    if !opened.is_file() || !same_file(&link, &opened) {
        return None;
    }
    let mut out = Vec::new();
    f.read_to_end(&mut out).ok()?;
    Some(out)
}

#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.len() == b.len()
}

pub fn write(dir: &Path, name: &str, data: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建模板目录：{e}"))?;
    let tmp = dir.join(format!(".{name}.sync-tmp"));
    std::fs::write(&tmp, data).map_err(|e| format!("无法写入 {name}：{e}"))?;
    std::fs::rename(&tmp, dir.join(name)).map_err(|e| format!("无法写入 {name}：{e}"))
}

pub fn remove(dir: &Path, name: &str) -> Result<(), String> {
    match std::fs::remove_file(dir.join(name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("无法删除 {name}：{e}")),
    }
}

pub fn hash(data: &[u8]) -> String {
    codec::b64u(&crypto::sha256(&[data]))
}

/// 文件的修改时间（unix 秒）；读不到时返回 None。不跟随符号链接。
pub fn modified(dir: &Path, name: &str) -> Option<i64> {
    let t = std::fs::symlink_metadata(dir.join(name)).ok()?.modified().ok()?;
    Some(match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    })
}

/// 把被覆盖的版本写入历史目录：`<名称>.<时间>.md`（UTC，`YYYYMMDD-HHMMSS`，已存在时追加序号），
/// 文件的修改时间设为 `unix`（清理按它计算）。返回写入的文件名。
pub fn backup(history: &Path, name: &str, data: &[u8], unix: i64) -> Result<String, String> {
    std::fs::create_dir_all(history).map_err(|e| format!("无法创建历史版本目录：{e}"))?;
    let stem = name.strip_suffix(".md").unwrap_or(name);
    let base = format!("{stem}.{}", timestamp(unix));
    let mut candidate = format!("{base}.md");
    let mut n = 2;
    while history.join(&candidate).exists() {
        candidate = format!("{base}-{n}.md");
        n += 1;
    }
    let path = history.join(&candidate);
    std::fs::write(&path, data).map_err(|e| format!("无法备份 {name}：{e}"))?;
    let at = UNIX_EPOCH + Duration::from_secs(unix.max(0) as u64);
    let _ = std::fs::File::options().write(true).open(&path).and_then(|f| f.set_modified(at));
    Ok(candidate)
}

/// 删除历史目录中早于 `now - keep_secs` 的版本。
pub fn prune_history(history: &Path, now: i64, keep_secs: i64) {
    let Ok(entries) = std::fs::read_dir(history) else { return };
    for e in entries.flatten() {
        if e.file_type().is_ok_and(|t| t.is_file())
            && let Some(name) = e.file_name().to_str()
            && modified(history, name).is_some_and(|t| t < now - keep_secs)
        {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn timestamp(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // 公历换算（Howard Hinnant 的 civil_from_days）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_keys_and_format_chars() {
        assert_eq!(name_key("Zh.MD"), name_key("zh.md"));
        // "é" 的 NFD 与 NFC 形式视为同一文件名
        assert_eq!(name_key("cafe\u{301}.md"), name_key("caf\u{e9}.md"));
        assert!(!valid_file_name("a\u{202E}dm.exe.md"));
        assert!(!valid_file_name("a\u{200B}b.md"));
        assert!(!valid_file_name("\u{FEFF}x.md"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let dir = std::env::temp_dir().join(format!("qantrip-files-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let outside = dir.join("outside.txt");
        std::fs::write(&outside, "secret").unwrap();
        let tpl = dir.join("tpl");
        std::fs::create_dir_all(&tpl).unwrap();
        std::os::unix::fs::symlink(&outside, tpl.join("link.md")).unwrap();
        std::fs::write(tpl.join("real.md"), "ok").unwrap();
        assert_eq!(list(&tpl), vec!["real.md".to_string()]);
        assert!(read(&tpl, "link.md").is_none());
        assert_eq!(read(&tpl, "real.md").unwrap(), b"ok");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names() {
        assert!(valid_file_name("zh.md"));
        assert!(valid_file_name("代码审查.md"));
        assert!(!valid_file_name("../x.md"));
        assert!(!valid_file_name("a/b.md"));
        assert!(!valid_file_name(".hidden.md"));
        assert!(!valid_file_name("x.txt"));
        assert!(!valid_file_name("zh.conflict-abc-20260101-000000.md"));
        assert_eq!(timestamp(0), "19700101-000000");
        assert_eq!(timestamp(1_792_281_600 + 3_723), "20261018-010203");
    }

    #[test]
    fn history_backup_and_prune() {
        let dir = std::env::temp_dir().join(format!("qantrip-history-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let t = 1_792_281_600;
        assert_eq!(backup(&dir, "zh.md", b"v1", t).unwrap(), "zh.20261018-000000.md");
        assert_eq!(backup(&dir, "zh.md", b"v2", t).unwrap(), "zh.20261018-000000-2.md");
        assert_eq!(backup(&dir, "zh.md", b"v3", t + 10 * 86_400).unwrap(), "zh.20261028-000000.md");
        assert_eq!(modified(&dir, "zh.20261018-000000.md"), Some(t));
        prune_history(&dir, t + 31 * 86_400, HISTORY_KEEP_SECS);
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left, vec!["zh.20261028-000000.md".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
