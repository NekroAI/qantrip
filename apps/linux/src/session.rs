//! 桌面会话识别：显示协议（Wayland / X11）与桌面环境（GNOME / KDE / wlroots 系 / 其他）。

use gtk::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
    Wayland,
    X11,
    Unknown,
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
            _ => Protocol::Unknown,
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

    pub fn is_niri(&self) -> bool {
        self.is_wayland() && self.desktop_name.split(':').any(|name| name.eq_ignore_ascii_case("niri"))
    }

    /// 界面上显示的会话名称，如 "GNOME · Wayland"
    pub fn label(&self) -> String {
        let d = match self.desktop {
            Desktop::Gnome => "GNOME".to_string(),
            Desktop::Kde => "KDE Plasma".to_string(),
            _ => self.desktop_name.clone(),
        };
        format!(
            "{d} · {}",
            match self.protocol {
                Protocol::Wayland => "Wayland",
                Protocol::X11 => "X11",
                Protocol::Unknown => "Unknown",
            }
        )
    }
}

use gtk::gdk;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niri_requires_a_wayland_session_and_exact_desktop_entry() {
        for (protocol, name, expected) in [
            (Protocol::Wayland, "niri", true),
            (Protocol::Wayland, "custom:NIRI", true),
            (Protocol::Wayland, "not-niri", false),
            (Protocol::Wayland, "Hyprland", false),
            (Protocol::X11, "niri", false),
        ] {
            let session = Session { protocol, desktop: Desktop::Wlroots, desktop_name: name.into() };
            assert_eq!(session.is_niri(), expected, "{protocol:?}: {name}");
        }
    }
}
