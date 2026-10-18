//! 文件位置（XDG Base Directory）。
//!
//! - 设置：`$XDG_CONFIG_HOME/qantrip/settings.json`
//! - 用户模板与使用记录：`$XDG_DATA_HOME/qantrip/templates/`、`usage.json`
//! - 同步状态：`$XDG_DATA_HOME/qantrip/sync/<服务端主机>/`
//! - 日志与本机状态：`$XDG_STATE_HOME/qantrip/`

use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if Path::new(&v).is_absolute() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("qantrip")
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("qantrip")
}

pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("qantrip")
}

pub fn templates_dir() -> PathBuf {
    let dir = data_dir().join("templates");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn usage_file() -> PathBuf {
    data_dir().join("usage.json")
}

pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn autostart_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("autostart").join(format!("{}.desktop", crate::APP_ID))
}

pub fn user_applications_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("applications")
}

pub fn user_icons_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("icons")
}

/// 应用自带的资源目录（字体、图标）：AppImage 中由 AppRun 设置 `QANTRIP_DATA_DIR`；
/// 安装包为 `/usr/share/qantrip`；开发时回退到仓库中的位置。
pub fn resource_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = std::env::var_os("QANTRIP_DATA_DIR") {
        out.push(PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(prefix) = exe.parent().and_then(Path::parent)
    {
        out.push(prefix.join("share/qantrip"));
    }
    out.push(PathBuf::from("/usr/share/qantrip"));
    out.push(PathBuf::from("/usr/local/share/qantrip"));
    out
}

/// 查找资源子目录（如 `fonts`、`icons`）；开发构建额外查找仓库中的源文件位置。
pub fn resource(sub: &str) -> Option<PathBuf> {
    for d in resource_dirs() {
        let p = d.join(sub);
        if p.exists() {
            return Some(p);
        }
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dev = match sub {
        "fonts" => manifest.join("../macos/Resources/Fonts"),
        _ => manifest.join("resources").join(sub),
    };
    if cfg!(debug_assertions) && dev.exists() { Some(dev) } else { None }
}

/// 以 0600 权限原子地写入文件（先写临时文件再改名）。
pub fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&tmp, path)
}

/// 普通文件的原子写入。
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

/// 把家目录替换为 `~`（界面显示用）。
pub fn display(path: &Path) -> String {
    let h = home();
    match path.strip_prefix(&h) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}
