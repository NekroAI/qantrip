//! 模板目录（D11：一个模板一个 `*.md` 文件）的读写。冲突副本（`*.conflict-*.md`）不参与同步。

use std::path::Path;

use super::codec;
use super::crypto;

const CONFLICT_MARK: &str = ".conflict-";

/// 远端记录中的文件名只能是模板目录下的普通 `.md` 文件名，防止路径穿越。
pub fn valid_file_name(name: &str) -> bool {
    name.ends_with(".md")
        && name.len() > 3
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
        && !name.contains(CONFLICT_MARK)
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

pub fn read(dir: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(dir.join(name)).ok()
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

/// 冲突副本名：`<名称>.conflict-<设备简称>-<时间>.md`（UTC，`YYYYMMDD-HHMMSS`）。已存在时追加序号。
pub fn conflict_name(dir: &Path, name: &str, device_short: &str, unix: i64) -> String {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    let base = format!("{stem}{CONFLICT_MARK}{device_short}-{}", timestamp(unix));
    let mut candidate = format!("{base}.md");
    let mut n = 2;
    while dir.join(&candidate).exists() {
        candidate = format!("{base}-{n}.md");
        n += 1;
    }
    candidate
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
}
