//! 桌面会话识别：显示协议（Wayland / X11）与桌面环境（GNOME / KDE / wlroots 系 / 其他）。

use gtk::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
    Wayland,
    X11,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Desktop {
    Gnome,
    Kde,
    /// Sway、Hyprland、river 等 wlroots 系合成器（尽力支持）
    Wlroots,
    Other,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub protocol: Protocol,
    pub desktop: Desktop,
    /// XDG_CURRENT_DESKTOP 原文（界面与日志显示用）
    pub desktop_name: String,
}

impl Session {
    pub fn detect() -> Session {
        let protocol = match gdk::Display::default() {
            Some(d) if d.is::<gdk4_wayland::WaylandDisplay>() => Protocol::Wayland,
            Some(d) if d.is::<gdk4_x11::X11Display>() => Protocol::X11,
            _ => {
                if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                    Protocol::Wayland
                } else {
                    Protocol::X11
                }
            }
        };
        let name = std::env::var("XDG_CURRENT_DESKTOP").or_else(|_| std::env::var("XDG_SESSION_DESKTOP")).unwrap_or_default();
        let lower = name.to_lowercase();
        let desktop = if lower.contains("gnome") || lower.contains("unity") {
            Desktop::Gnome
        } else if lower.contains("kde") || lower.contains("plasma") {
            Desktop::Kde
        } else if ["sway", "hyprland", "river", "wlroots", "labwc", "wayfire", "niri", "dwl"].iter().any(|w| lower.contains(w)) {
            Desktop::Wlroots
        } else {
            Desktop::Other
        };
        Session { protocol, desktop, desktop_name: if name.is_empty() { "unknown".into() } else { name } }
    }

    pub fn is_wayland(&self) -> bool {
        self.protocol == Protocol::Wayland
    }

    pub fn is_x11(&self) -> bool {
        self.protocol == Protocol::X11
    }

    /// 界面上显示的会话名称，如 "GNOME · Wayland"
    pub fn label(&self) -> String {
        let d = match self.desktop {
            Desktop::Gnome => "GNOME".to_string(),
            Desktop::Kde => "KDE Plasma".to_string(),
            _ => self.desktop_name.clone(),
        };
        format!("{d} · {}", if self.is_wayland() { "Wayland" } else { "X11" })
    }
}

use gtk::gdk;
