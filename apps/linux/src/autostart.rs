//! 登录时启动（XDG Autostart）与 AppImage 的桌面集成。

use crate::paths;

/// 启动 Qantrip 的命令：AppImage 为镜像文件本身，安装包为 /usr/bin/qantrip
pub fn exec_path() -> String {
    if let Some(p) = std::env::var_os("APPIMAGE") {
        return p.to_string_lossy().into_owned();
    }
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "qantrip".into())
}

fn quote(s: &str) -> String {
    // Exec quoting and Desktop Entry string escaping are separate layers.
    // Literal '%' must not become an Exec field code after an AppImage is moved.
    let quoted = format!("\"{}\"", s.replace('%', "%%").replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`"));
    quoted.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

fn autostart_entry() -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Nekro Qantrip\nComment=Prompt launcher\nExec={} --background\nIcon={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\nX-KDE-autostart-after=panel\nNoDisplay=true\n",
        quote(&exec_path()),
        crate::APP_ID
    )
}

pub fn is_enabled() -> bool {
    let p = paths::autostart_file();
    match std::fs::read_to_string(&p) {
        Ok(s) => !s.lines().any(|l| l.trim() == "Hidden=true" || l.trim() == "X-GNOME-Autostart-enabled=false"),
        Err(_) => false,
    }
}

pub fn set_enabled(on: bool) -> std::io::Result<()> {
    let p = paths::autostart_file();
    if on {
        paths::write_atomic(&p, autostart_entry().as_bytes())
    } else {
        match std::fs::remove_file(&p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// AppImage 移动位置或更新后，自启动项中的路径可能失效：启动时按当前位置重写
pub fn refresh_if_enabled() {
    if is_enabled() {
        let current = std::fs::read_to_string(paths::autostart_file()).unwrap_or_default();
        if current != autostart_entry() {
            let _ = set_enabled(true);
        }
    }
}

/// AppImage：把 .desktop 与图标写入 ~/.local/share，使应用出现在应用菜单中，
/// 门户（全局快捷键、自动粘贴授权对话框）也能显示应用名称与图标。安装包已自带，不需要。
pub fn integrate_appimage() {
    let Some(image) = std::env::var_os("APPIMAGE") else { return };
    let image = image.to_string_lossy().into_owned();
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Nekro Qantrip\nGenericName=Prompt Launcher\nComment=Press Ctrl+Shift+Q to insert prompts anywhere\nComment[zh_CN]=按 Ctrl+Shift+Q 在任何应用中插入 Prompt\nExec={} %U\nIcon={}\nTerminal=false\nCategories=Utility;\nKeywords=prompt;ai;launcher;\nStartupNotify=true\nStartupWMClass={}\nX-GNOME-UsesNotifications=true\nActions=Toggle;Settings;\n\n[Desktop Action Toggle]\nName=Open Panel\nName[zh_CN]=打开面板\nExec={} toggle\n\n[Desktop Action Settings]\nName=Settings\nName[zh_CN]=设置\nExec={} settings\n",
        quote(&image),
        crate::APP_ID,
        crate::APP_ID,
        quote(&image),
        quote(&image)
    );
    let path = paths::user_applications_dir().join(format!("{}.desktop", crate::APP_ID));
    if std::fs::read_to_string(&path).ok().as_deref() != Some(desktop.as_str()) {
        let _ = paths::write_atomic(&path, desktop.as_bytes());
    }
    // 图标：AppImage 根目录的 .DirIcon / 应用图标
    if let Some(dir) = std::env::var_os("APPDIR") {
        let src = std::path::Path::new(&dir).join(format!("usr/share/icons/hicolor/256x256/apps/{}.png", crate::APP_ID));
        let dst = paths::user_icons_dir().join(format!("hicolor/256x256/apps/{}.png", crate::APP_ID));
        if src.exists() && !dst.exists() {
            if let Some(p) = dst.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_exec_roundtrips_spaces_and_metacharacters() {
        let executable = "/tmp/My \"Qantrip\" $dir\\100%.AppImage";
        let file = gtk::glib::KeyFile::new();
        file.load_from_data(&format!("[Desktop Entry]\nExec={} toggle\n", super::quote(executable)), gtk::glib::KeyFileFlags::NONE).unwrap();
        let exec = file.string("Desktop Entry", "Exec").unwrap().replace("%%", "%");
        let argv = gtk::glib::shell_parse_argv(exec).unwrap();
        assert_eq!(argv, [std::ffi::OsString::from(executable), std::ffi::OsString::from("toggle")]);
    }
}
