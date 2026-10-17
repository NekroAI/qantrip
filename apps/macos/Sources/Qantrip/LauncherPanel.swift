import AppKit
import SwiftUI

/// 不激活本应用的浮动面板：可以成为 key window 接收键盘输入，
/// 但原应用保持激活状态（PRD 3.2）。`.nonactivatingPanel` 必须在初始化时设置。
final class LauncherPanel: NSPanel {
    init(contentRect: NSRect) {
        super.init(contentRect: contentRect,
                   styleMask: [.borderless, .nonactivatingPanel, .fullSizeContentView],
                   backing: .buffered, defer: false)
        isFloatingPanel = true
        level = .floating
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        hidesOnDeactivate = false
        becomesKeyOnlyIfNeeded = false
        isMovableByWindowBackground = false
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        animationBehavior = .none
    }

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}

/// 管理面板的预创建、显示、隐藏与唤起延迟埋点。
final class LauncherController: NSObject, NSWindowDelegate {
    let model = LauncherModel()
    private let panel: LauncherPanel
    private let inserter = Inserter()
    private let hud = HUD()
    /// 唤起面板时的前台应用，作为插入目标。
    private var target: NSRunningApplication?

    static let size = NSSize(width: 640, height: 380)

    override init() {
        panel = LauncherPanel(contentRect: NSRect(origin: .zero, size: Self.size))
        super.init()
        let host = NSHostingView(rootView: LauncherView(model: model))
        host.frame = NSRect(origin: .zero, size: Self.size)
        panel.contentView = host
        panel.delegate = self
        model.onSubmit = { [weak self] text in self?.submit(text) }
        model.onCancel = { [weak self] in self?.hide() }
        // 预先布局并渲染一次，避免首次唤起时的冷启动开销。
        panel.setFrameOrigin(NSPoint(x: -10_000, y: -10_000))
        panel.orderFrontRegardless()
        panel.displayIfNeeded()
        panel.orderOut(nil)
    }

    var isVisible: Bool { panel.isVisible }

    func toggle(eventTime: TimeInterval, source: String) {
        isVisible ? hide() : show(eventTime: eventTime, source: source)
    }

    func show(eventTime: TimeInterval, source: String) {
        let t1 = ProcessInfo.processInfo.systemUptime
        target = NSWorkspace.shared.frontmostApplication
        model.reset()
        position()
        panel.makeKeyAndOrderFront(nil)
        panel.makeFirstResponder(model.fieldView)
        panel.displayIfNeeded()
        let t2 = ProcessInfo.processInfo.systemUptime
        // 下一轮主循环时视为已可输入
        DispatchQueue.main.async { [self] in
            let t3 = ProcessInfo.processInfo.systemUptime
            SpikeLog.event("show", [
                "source": source,
                "target": target?.bundleIdentifier ?? "?",
                "event_to_handler_ms": SpikeLog.ms(t1 - eventTime),
                "handler_to_shown_ms": SpikeLog.ms(t2 - t1),
                "event_to_ready_ms": SpikeLog.ms(t3 - eventTime),
                "is_key": panel.isKeyWindow,
                "app_active": NSApp.isActive,
                "target_still_active": target?.isActive ?? false,
            ])
        }
    }

    func hide() {
        panel.orderOut(nil)
    }

    private func position() {
        let mouse = NSEvent.mouseLocation
        let screen = NSScreen.screens.first { $0.frame.contains(mouse) } ?? NSScreen.main
        guard let vf = screen?.visibleFrame else { return }
        let x = vf.midX - Self.size.width / 2
        let y = vf.minY + vf.height * 0.72 - Self.size.height / 2
        panel.setFrame(NSRect(x: x, y: y, width: Self.size.width, height: Self.size.height), display: false)
    }

    private func submit(_ text: String) {
        hide()
        insert(text)
    }

    func insert(_ text: String, target override: NSRunningApplication? = nil) {
        inserter.insert(text, target: override ?? target) { [weak self] outcome in
            if case .copiedOnly(let reason) = outcome {
                self?.hud.show("已复制到剪贴板（\(reason)），按 ⌘V 粘贴")
            }
        }
    }

    func windowDidResignKey(_ notification: Notification) {
        // 点击面板外部时关闭，行为与 Spotlight 一致。
        hide()
    }
}

/// 简单的提示浮层。
final class HUD {
    private let panel: NSPanel
    private let label = NSTextField(labelWithString: "")
    private var hideWork: DispatchWorkItem?

    init() {
        panel = NSPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel],
                        backing: .buffered, defer: true)
        panel.level = .statusBar
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.ignoresMouseEvents = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        let bg = NSVisualEffectView()
        bg.material = .hudWindow
        bg.state = .active
        bg.wantsLayer = true
        bg.layer?.cornerRadius = 10
        label.font = .systemFont(ofSize: 14, weight: .medium)
        label.translatesAutoresizingMaskIntoConstraints = false
        bg.addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: bg.centerXAnchor),
            label.centerYAnchor.constraint(equalTo: bg.centerYAnchor),
        ])
        panel.contentView = bg
    }

    func show(_ text: String) {
        label.stringValue = text
        let width = label.intrinsicContentSize.width + 40
        let screen = NSScreen.screens.first { $0.frame.contains(NSEvent.mouseLocation) } ?? NSScreen.main
        guard let vf = screen?.visibleFrame else { return }
        panel.setFrame(NSRect(x: vf.midX - width / 2, y: vf.minY + vf.height * 0.2, width: width, height: 44),
                       display: true)
        panel.orderFrontRegardless()
        hideWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.panel.orderOut(nil) }
        hideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0, execute: work)
    }
}
