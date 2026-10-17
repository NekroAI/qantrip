import AppKit
import QantripCore
import QuartzCore
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
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        animationBehavior = .none
    }

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}

/// 管理面板的预创建、显示、隐藏、插入与动效。
final class LauncherController: NSObject, NSWindowDelegate {
    let model: LauncherModel
    private let panel: LauncherPanel
    private let inserter = Inserter()
    private let hud = HUD()
    private let inputSource = InputSourceGuard()
    private var rejectObserved = 0

    init(engine: Engine) {
        model = LauncherModel(engine: engine)
        panel = LauncherPanel(contentRect: NSRect(origin: .zero, size: Theme.panelSize))
        super.init()
        let host = NSHostingView(rootView: LauncherView(model: model))
        host.frame = NSRect(origin: .zero, size: Theme.panelSize)
        host.wantsLayer = true
        panel.contentView = host
        panel.delegate = self
        model.onSubmit = { [weak self] text, expand in self?.submit(text, expand: expand) }
        model.onCancel = { [weak self] in self?.hide() }
        observeRejects()
        // 启动后离屏完整走一遍显示流程，避免首次唤起时的冷启动开销（实测约 250ms）
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [self] in prewarm() }
    }

    private func prewarm() {
        guard !panel.isVisible else { return }
        let t0 = ProcessInfo.processInfo.systemUptime
        panel.setFrameOrigin(NSPoint(x: -10_000, y: -10_000))
        panel.alphaValue = 0
        panel.orderFrontRegardless()
        panel.makeFirstResponder(model.field)
        for input in ["r", "r --scope=st", "r security 示例"] {
            model.field.setText(input)
            model.textDidChange(input)
            panel.contentView?.layoutSubtreeIfNeeded()
            panel.displayIfNeeded()
        }
        model.reset()
        panel.displayIfNeeded()
        panel.orderOut(nil)
        panel.alphaValue = 1
        SpikeLog.event("prewarm", ["ms": SpikeLog.ms(ProcessInfo.processInfo.systemUptime - t0)])
    }

    var isVisible: Bool { panel.isVisible }

    func toggle(eventTime: TimeInterval, source: String) {
        isVisible ? hide() : show(eventTime: eventTime, source: source)
    }

    func show(eventTime: TimeInterval, source: String) {
        let t1 = ProcessInfo.processInfo.systemUptime
        model.target = NSWorkspace.shared.frontmostApplication
        inputSource.engage()
        let tIME = ProcessInfo.processInfo.systemUptime
        model.reset()
        let tModel = ProcessInfo.processInfo.systemUptime
        position()
        panel.alphaValue = 0
        panel.makeKeyAndOrderFront(nil)
        panel.makeFirstResponder(model.field)
        panel.displayIfNeeded()
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0.12
            ctx.timingFunction = CAMediaTimingFunction(name: .easeOut)
            panel.animator().alphaValue = 1
        }
        let t2 = ProcessInfo.processInfo.systemUptime
        DispatchQueue.main.async { [self] in
            let t3 = ProcessInfo.processInfo.systemUptime
            SpikeLog.event("show", [
                "source": source,
                "target": model.target?.bundleIdentifier ?? "?",
                "event_to_handler_ms": SpikeLog.ms(t1 - eventTime),
                "handler_to_shown_ms": SpikeLog.ms(t2 - t1),
                "ime_ms": SpikeLog.ms(tIME - t1),
                "model_ms": SpikeLog.ms(tModel - tIME),
                "display_ms": SpikeLog.ms(t2 - tModel),
                "event_to_ready_ms": SpikeLog.ms(t3 - eventTime),
                "is_key": panel.isKeyWindow,
                // NSApp.isActive 在 non-activating 面板下读数不可靠，以系统视角的前台应用为准
                "frontmost": NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "?",
            ])
        }
    }

    func hide() {
        guard panel.isVisible else { return }
        panel.orderOut(nil)
        inputSource.restore()
    }

    private func position() {
        let mouse = NSEvent.mouseLocation
        let screen = NSScreen.screens.first { $0.frame.contains(mouse) } ?? NSScreen.main
        guard let vf = screen?.visibleFrame else { return }
        let size = Theme.panelSize
        let x = vf.midX - size.width / 2
        let y = vf.minY + vf.height * 0.68 - size.height / 2
        panel.setFrame(NSRect(x: x, y: y, width: size.width, height: size.height), display: false)
    }

    private func submit(_ text: String, expand: Bool) {
        castEffect()
        hide()
        insert(text, expand: expand)
    }

    func insert(_ text: String, target override: NSRunningApplication? = nil, expand: Bool = false) {
        inserter.insert(text, target: override ?? model.target, expand: expand) { [weak self] outcome in
            if case .copiedOnly(let reason) = outcome {
                self?.hud.show("已复制到剪贴板（\(reason)），按 ⌘V 粘贴")
            }
        }
    }

    func windowDidResignKey(_ notification: Notification) {
        // 点击面板外部时关闭，行为与 Spotlight 一致。
        hide()
    }

    // MARK: 动效

    /// "施法"：面板快照在独立的、永不成为 key 的窗口里上浮淡出，不阻塞粘贴。
    private func castEffect() {
        guard let view = panel.contentView, let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
        view.cacheDisplay(in: view.bounds, to: rep)
        let image = NSImage(size: view.bounds.size)
        image.addRepresentation(rep)

        let frame = panel.frame
        let ghost = NSPanel(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        ghost.isOpaque = false
        ghost.backgroundColor = .clear
        ghost.hasShadow = false
        ghost.ignoresMouseEvents = true
        ghost.level = .floating
        ghost.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        let iv = NSImageView(frame: NSRect(origin: .zero, size: frame.size))
        iv.image = image
        iv.imageScaling = .scaleAxesIndependently
        ghost.contentView = iv
        ghost.orderFrontRegardless()

        let end = frame.insetBy(dx: frame.width * 0.03, dy: frame.height * 0.03).offsetBy(dx: 0, dy: 14)
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = 0.22
            ctx.timingFunction = CAMediaTimingFunction(name: .easeIn)
            ghost.animator().alphaValue = 0
            ghost.animator().setFrame(end, display: true)
        }, completionHandler: { ghost.orderOut(nil) })
    }

    /// 回车被阻止时让面板轻微抖动。
    private func observeRejects() {
        withObservationTracking { _ = model.rejectCount } onChange: { [weak self] in
            DispatchQueue.main.async {
                self?.shake()
                self?.observeRejects()
            }
        }
    }

    private func shake() {
        guard let layer = panel.contentView?.layer else { return }
        let a = CAKeyframeAnimation(keyPath: "transform.translation.x")
        a.values = [0, -9, 8, -6, 4, -2, 0]
        a.duration = 0.32
        layer.add(a, forKey: "shake")
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
