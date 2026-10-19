//! 无桌面依赖的后端契约与事件/生命周期规则。

use crate::i18n::l;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShortcutId {
    TogglePanel,
}

impl ShortcutId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TogglePanel => "toggle-panel",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    X11,
    Portal,
    Command,
    Tray,
    #[cfg(all(feature = "debug-hooks", debug_assertions))]
    Debug,
}

impl Source {
    pub fn physical(self) -> bool {
        matches!(self, Self::X11 | Self::Portal)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Pressed,
    Released,
    /// CLI / 托盘是一次动作，不能伪造物理按下与释放。
    Invoke,
}

// 不派生 Debug：激活令牌不得进入日志。
#[derive(Clone)]
pub struct Press {
    pub id: ShortcutId,
    pub source: Source,
    pub phase: Phase,
    pub timestamp: u32,
    pub token: Option<String>,
}

impl Press {
    pub fn invoke(source: Source) -> Self {
        Self { id: ShortcutId::TogglePanel, source, phase: Phase::Invoke, timestamp: 0, token: None }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Portal,
    X11,
    Manual,
}

/// 显示连接（包括 Xwayland 检测）决定候选后端；候选不等于注册成功。
pub fn select_backend(wayland_session: bool, native_x11_connected: bool) -> Mode {
    if wayland_session {
        Mode::Portal
    } else if native_x11_connected {
        Mode::X11
    } else {
        Mode::Manual
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capabilities {
    pub release_events: bool,
    pub configure: bool,
    pub passthrough_on_pause: bool,
}

impl Capabilities {
    pub const MANUAL: Self = Self { release_events: false, configure: false, passthrough_on_pause: false };
    pub const X11: Self = Self { release_events: true, configure: true, passthrough_on_pause: true };
    pub fn portal(version: u32) -> Self {
        Self { release_events: true, configure: version >= 2, passthrough_on_pause: false }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    Unknown,
    Unavailable,
    /// 仅探测到了接口版本，还未验证会话、绑定和信号订阅。
    PortalVersion(u32),
    Verified(Capabilities),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failure {
    Unsupported,
    Cancelled,
    Denied,
    Conflict,
    Disconnected,
    Temporary,
    InvalidShortcut,
    NotBound,
    Subscription,
    MappingChanged,
    Superseded,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Superseded => l("快捷键请求已被后续操作取消", "The shortcut request was superseded by a later operation"),
            Self::Unsupported => l("系统未提供可用的全局快捷键服务", "No global shortcut service is available"),
            Self::Cancelled => l("已取消系统授权；点击重试才会再次请求", "Authorization cancelled; only Retry will request it again"),
            Self::Denied => l("系统拒绝了快捷键请求", "The system denied the shortcut request"),
            Self::Conflict => l("快捷键已被其他程序占用", "The shortcut is in use by another app"),
            Self::Disconnected => l("快捷键服务或显示连接已断开", "The shortcut service or display connection was lost"),
            Self::Temporary => l("系统暂时无法完成快捷键请求", "The system could not complete the shortcut request"),
            Self::InvalidShortcut => l("无法识别快捷键或当前键盘映射", "Unrecognized shortcut or keyboard mapping"),
            Self::NotBound => l("系统没有绑定唤起快捷键", "The system did not bind the panel shortcut"),
            Self::Subscription => l("无法监听系统快捷键事件", "Could not subscribe to shortcut events"),
            Self::MappingChanged => {
                l("键盘映射变化后无法重新注册，请重试或更换组合", "Could not register after a keyboard mapping change; retry or change the shortcut")
            }
        };
        f.write_str(&message)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum State {
    Pending,
    AwaitingPermission,
    Active { trigger: String },
    Paused { trigger: String },
    Failed { reason: Failure },
    Manual,
    Closed,
}

impl State {
    pub fn registered(&self) -> bool {
        matches!(self, Self::Active { .. } | Self::Paused { .. })
    }
}

/// 每轮注册只有一个 generation；注销立即作废旧任务及其队列中的事件。
#[derive(Default)]
pub(super) struct Lifecycle {
    pub(super) generation: u64,
    started: bool,
    closed: bool,
}

impl Lifecycle {
    pub fn begin(&mut self, retry: bool) -> Option<u64> {
        if self.closed || (self.started && !retry) {
            return None;
        }
        self.started = true;
        self.generation += 1;
        Some(self.generation)
    }

    pub fn accepts(&self, generation: u64) -> bool {
        !self.closed && self.generation == generation
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.generation += 1;
    }
}

/// Portal：按住期间再次收到"按下"、且与上一次"按下"间隔超过这个时间，视为一次新的按下
/// （有的后端不发 Deactivated，或面板取得焦点时释放信号丢失；自动重复的间隔只有几十毫秒）
const PORTAL_REPRESS: std::time::Duration = std::time::Duration::from_millis(400);

#[derive(Default)]
pub(super) struct EventGate {
    held: bool,
    last_press: Option<std::time::Instant>,
}

impl EventGate {
    /// 更新物理状态后再应用暂停，避免暂停期间丢失释放导致永久抑制。
    pub fn accept(&mut self, event: &Press, suspended: bool) -> bool {
        self.accept_at(event, suspended, std::time::Instant::now())
    }

    pub fn accept_at(&mut self, event: &Press, suspended: bool, now: std::time::Instant) -> bool {
        if event.id != ShortcutId::TogglePanel {
            return false;
        }
        match (event.source.physical(), event.phase) {
            (true, Phase::Pressed) => {
                let last = self.last_press.replace(now);
                let mut repeated = std::mem::replace(&mut self.held, true);
                if repeated && event.source == Source::Portal && last.is_some_and(|t| now.duration_since(t) >= PORTAL_REPRESS) {
                    repeated = false;
                }
                !repeated && !suspended
            }
            (true, Phase::Released) => {
                let held = std::mem::replace(&mut self.held, false);
                held && !suspended
            }
            (false, Phase::Invoke) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_never_falls_back_to_xwayland_grabs() {
        assert_eq!(select_backend(true, true), Mode::Portal);
        assert_eq!(select_backend(true, false), Mode::Portal);
        assert_eq!(select_backend(false, true), Mode::X11);
        assert_eq!(select_backend(false, false), Mode::Manual);
    }

    #[test]
    fn duplicate_start_stale_completion_and_shutdown() {
        let mut life = Lifecycle::default();
        let first = life.begin(false).unwrap();
        assert_eq!(life.begin(false), None);
        let retry = life.begin(true).unwrap();
        assert!(!life.accepts(first));
        assert!(life.accepts(retry));
        life.close();
        assert!(!life.accepts(retry));
        assert_eq!(life.begin(true), None);
    }

    #[test]
    fn press_release_repeat_and_commands_are_distinct() {
        let mut gate = EventGate::default();
        let mut event = Press { phase: Phase::Pressed, ..Press::invoke(Source::Portal) };
        assert!(gate.accept(&event, false));
        assert!(!gate.accept(&event, false));
        event.phase = Phase::Released;
        assert!(gate.accept(&event, false));
        assert!(!gate.accept(&event, false));
        event.phase = Phase::Pressed;
        assert!(!gate.accept(&event, true));
        event.phase = Phase::Released;
        assert!(!gate.accept(&event, true));
        event.phase = Phase::Pressed;
        assert!(gate.accept(&event, false));
        // Portal 后端不发释放：间隔足够长的下一次按下照常触发，自动重复仍被过滤
        let mut gate = EventGate::default();
        let t0 = std::time::Instant::now();
        let press = Press { phase: Phase::Pressed, ..Press::invoke(Source::Portal) };
        assert!(gate.accept_at(&press, false, t0));
        assert!(!gate.accept_at(&press, false, t0 + std::time::Duration::from_millis(30)));
        assert!(gate.accept_at(&press, false, t0 + std::time::Duration::from_secs(2)));
        // X11 自己处理按住与重复（x11.rs），这里仍按释放判断
        let mut gate = EventGate::default();
        let x11 = Press { phase: Phase::Pressed, ..Press::invoke(Source::X11) };
        assert!(gate.accept_at(&x11, false, t0));
        assert!(!gate.accept_at(&x11, false, t0 + std::time::Duration::from_secs(2)));
        let command = Press::invoke(Source::Command);
        assert!(gate.accept(&command, true));
        assert!(gate.accept(&command, true)); // 两次显式命令就是两次动作
        const { assert!(!Capabilities::MANUAL.release_events) };
        assert!(!Capabilities::portal(1).configure);
        assert!(Capabilities::portal(2).configure);
    }
}
