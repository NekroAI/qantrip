//! 小工具：核心使用 UTF-16 偏移，GTK 文本缓冲区使用字符（Unicode 标量）偏移，Pango 使用字节偏移。

/// UTF-16 偏移 → 字符偏移（超出末尾时取末尾）。
pub fn u16_to_char(s: &str, u16: u32) -> usize {
    let mut n = 0u32;
    for (i, c) in s.chars().enumerate() {
        if n >= u16 {
            return i;
        }
        n += c.len_utf16() as u32;
    }
    s.chars().count()
}

/// 字符偏移 → UTF-16 偏移。
#[cfg(test)]
pub fn char_to_u16(s: &str, chars: usize) -> u32 {
    s.chars().take(chars).map(|c| c.len_utf16() as u32).sum()
}

/// UTF-16 偏移 → 字节偏移。
pub fn u16_to_byte(s: &str, u16: u32) -> usize {
    let mut n = 0u32;
    for (i, c) in s.char_indices() {
        if n >= u16 {
            return i;
        }
        n += c.len_utf16() as u32;
    }
    s.len()
}

/// 把 UTF-16 区间 `[start, end)` 的替换应用到字符串上。
pub fn replace_u16(s: &str, start: u32, end: u32, text: &str) -> Option<(String, u32)> {
    let total: u32 = s.encode_utf16().count() as u32;
    if start > end || end > total {
        return None;
    }
    let a = u16_to_byte(s, start);
    let b = u16_to_byte(s, end);
    let mut out = String::with_capacity(s.len() + text.len());
    out.push_str(&s[..a]);
    out.push_str(text);
    out.push_str(&s[b..]);
    Some((out, start + text.encode_utf16().count() as u32))
}

/// 去掉首尾空白后的第一个词（命令词）。
pub fn first_word(s: &str) -> String {
    s.split_whitespace().next().unwrap_or("").to_string()
}

/// 是否为指令模式输入（以 `/` 或全角 `／` 开头）。
pub fn is_command_mode(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with('/') || t.starts_with('／')
}

/// 人类可读的字节数（与系统文件管理器一致，按 1000 进位）。
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else if v < 10.0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.0} {}", UNITS[i])
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// 单调时钟（秒），用于计时日志。
pub fn uptime() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// 启动子进程：去掉本进程为自己设置的 GSK_RENDERER，避免影响被启动的应用
pub fn launcher() -> gtk::gio::SubprocessLauncher {
    let l = gtk::gio::SubprocessLauncher::new(gtk::gio::SubprocessFlags::NONE);
    if std::env::var_os("QANTRIP_SET_GSK_RENDERER").is_some() {
        l.unsetenv("GSK_RENDERER");
        l.unsetenv("QANTRIP_SET_GSK_RENDERER");
    }
    l
}

pub fn ms(seconds: f64) -> f64 {
    (seconds * 10_000.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets() {
        let s = "r 安全😀x";
        assert_eq!(u16_to_char(s, 2), 2);
        assert_eq!(u16_to_char(s, 6), 5); // 😀 占两个 UTF-16 单元
        assert_eq!(char_to_u16(s, 5), 6);
        assert_eq!(u16_to_byte(s, 3), 5);
        assert_eq!(replace_u16(s, 0, 1, "review"), Some(("review 安全😀x".to_string(), 6)));
        assert_eq!(replace_u16(s, 0, 99, ""), None);
    }

    #[test]
    fn bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
    }
}
