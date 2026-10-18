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

    /// ⌘L 打开模板库、⌘, 打开设置
    var onShortcut: ((String) -> Bool)?

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
           let key = event.charactersIgnoringModifiers?.lowercased(), onShortcut?(key) == true {
            return true
        }
        return super.performKeyEquivalent(with: event)
    }
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
        model.onCommand = { [weak self] outcome in self?.finishCommand(outcome) }
        model.onOpenLibrary = { [weak self] in self?.openWindow { LibraryWindow.show() } }
        model.onOpenSettings = { [weak self] in self?.openWindow { LibraryWindow.showSettings() } }
        panel.onShortcut = { [weak self] key in
            switch key {
            case "l": self?.model.onOpenLibrary?(); return true
            case ",": self?.model.onOpenSettings?(); return true
            default: return false
            }
        }
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
        model.begin(target: nil)
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
        MainActor.assumeIsolated { SyncService.shared.panelOpened() }
        let t1 = ProcessInfo.processInfo.systemUptime
        let target = NSWorkspace.shared.frontmostApplication
        // 在 Qantrip 自己的窗口里唤起（如引导页的练习框）：记住原来的键窗口，插入时直接写入，不走模拟粘贴
        ownKeyWindow = target?.processIdentifier == ProcessInfo.processInfo.processIdentifier ? NSApp.keyWindow.flatMap { $0 === panel ? nil : $0 } : nil
        inputSource.engage()
        let tIME = ProcessInfo.processInfo.systemUptime
        model.begin(target: target)
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
        springIn()
        DispatchQueue.main.async { [self] in model.entered = true }
        let t2 = ProcessInfo.processInfo.systemUptime
        DispatchQueue.main.async { [self] in
            let t3 = ProcessInfo.processInfo.systemUptime
            SpikeLog.event("show", [
                "source": source,
                "target": target?.bundleIdentifier ?? "?",
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
        // 在自己的窗口里唤起时，关闭面板后把键盘焦点还给那个窗口
        ownKeyWindow?.makeKeyAndOrderFront(nil)
        model.entered = false
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

    /// 指令完成：关闭面板，删除的文件移到废纸篓，需要时打开模板库，最后提示。
    private func finishCommand(_ outcome: CommandOutcome) {
        hide()
        if let path = outcome.trashPath {
            try? FileManager.default.trashItem(at: URL(fileURLWithPath: path), resultingItemURL: nil)
            AppServices.shared.engine.reload()
        }
        LibraryWindow.model.reload()
        if let name = outcome.openEditor {
            LibraryWindow.show(select: name)
        }
        var message = outcome.message
        switch outcome.action ?? "" {
        case "sync":
            let started = MainActor.assumeIsolated { () -> Bool in
                let sync = SyncService.shared
                guard sync.enrolled else { return false }
                sync.syncNow()
                return true
            }
            if started {
                message = L("正在同步…", "Syncing…")
            } else {
                (NSApp.delegate as? AppDelegate)?.openSyncSettings()
            }
        case "settings":
            (NSApp.delegate as? AppDelegate)?.openSettings()
        case "library":
            LibraryWindow.show()
        case let a where a.hasPrefix("lang:"):
            if let choice = LanguageChoice(rawValue: String(a.dropFirst(5))) {
                AppSettings.shared.templateLanguage = choice
            }
        default:
            break
        }
        if !message.isEmpty { hud.show(message) }
    }

    /// 关闭面板后打开 Qantrip 的窗口（不把焦点还给原应用）
    private func openWindow(_ open: @escaping () -> Void) {
        ownKeyWindow = nil
        hide()
        DispatchQueue.main.async(execute: open)
    }

    /// 唤起面板前 Qantrip 自己的键窗口（见 show）
    private var ownKeyWindow: NSWindow?

    func insert(_ text: String, target override: NSRunningApplication? = nil, expand: Bool = false) {
        if override == nil, let window = ownKeyWindow {
            ownKeyWindow = nil
            insertIntoOwnWindow(text, window: window)
            return
        }
        inserter.insert(text, target: override ?? model.target, expand: expand) { [weak self] outcome in
            if case .copiedOnly(let reason) = outcome {
                self?.hud.show(L("已复制到剪贴板（\(reason)），按 ⌘V 粘贴", "Copied to clipboard (\(reason)). Press ⌘V to paste"))
            }
        }
    }

    /// 目标是 Qantrip 自己的窗口：恢复键窗口后直接把文字插入当前输入框
    private func insertIntoOwnWindow(_ text: String, window: NSWindow) {
        window.makeKeyAndOrderFront(nil)
        DispatchQueue.main.async {
            SpikeLog.event("insert_own", ["responder": window.firstResponder.map { String(describing: type(of: $0)) } ?? "nil", "chars": text.count])
            if let client = window.firstResponder as? NSTextInputClient {
                client.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
            } else {
                let pb = NSPasteboard.general
                pb.prepareForNewContents(with: .currentHostOnly)
                pb.setString(text, forType: .string)
                NSApp.sendAction(#selector(NSText.paste(_:)), to: nil, from: nil)
            }
        }
    }

    func windowDidResignKey(_ notification: Notification) {
        // 点击面板外部时关闭，行为与 Spotlight 一致。
        hide()
    }

    // MARK: 动效

    /// 入场：从略小、略低处弹入（D15）。
    private func springIn() {
        guard !LauncherModel.reduceMotion, let layer = panel.contentView?.layer else { return }
        let b = layer.bounds
        var from = CATransform3DMakeTranslation(b.midX, b.midY, 0)
        from = CATransform3DScale(from, 0.965, 0.965, 1)
        from = CATransform3DTranslate(from, -b.midX, -b.midY - 8, 0)
        let a = CASpringAnimation(keyPath: "transform")
        a.fromValue = NSValue(caTransform3D: from)
        a.toValue = NSValue(caTransform3D: CATransform3DIdentity)
        a.mass = 1
        a.stiffness = 420
        a.damping = 26
        a.duration = a.settlingDuration
        layer.add(a, forKey: "springIn")
    }

    private static let sparkle: CGImage? = {
        let config = NSImage.SymbolConfiguration(pointSize: 11, weight: .bold)
        guard let symbol = NSImage(systemSymbolName: "sparkle", accessibilityDescription: nil)?.withSymbolConfiguration(config)
        else { return nil }
        let size = symbol.size
        let img = NSImage(size: size, flipped: false) { rect in
            symbol.draw(in: rect)
            NSColor.white.set()
            rect.fill(using: .sourceAtop)
            return true
        }
        return img.cgImage(forProposedRect: nil, context: nil, hints: nil)
    }()

    /// "施法"：面板快照在独立的、永不成为 key 的窗口里上浮淡出，并从魔杖处迸出星光（D15）。
    /// 该窗口忽略鼠标、不抢焦点，因此不影响随后发往目标应用的 ⌘V。
    private func castEffect() {
        guard let view = panel.contentView, let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
        view.cacheDisplay(in: view.bounds, to: rep)
        let image = NSImage(size: view.bounds.size)
        image.addRepresentation(rep)

        let frame = panel.frame
        let pad: CGFloat = 60
        let outer = frame.insetBy(dx: -pad, dy: -pad)
        let ghost = NSPanel(contentRect: outer, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        ghost.isOpaque = false
        ghost.backgroundColor = .clear
        ghost.hasShadow = false
        ghost.ignoresMouseEvents = true
        ghost.level = .floating
        ghost.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        let container = NSView(frame: NSRect(origin: .zero, size: outer.size))
        container.wantsLayer = true
        let iv = NSImageView(frame: NSRect(x: pad, y: pad, width: frame.width, height: frame.height))
        iv.image = image
        iv.imageScaling = .scaleAxesIndependently
        iv.wantsLayer = true
        container.addSubview(iv)
        ghost.contentView = container
        ghost.orderFrontRegardless()

        let reduce = LauncherModel.reduceMotion
        if !reduce, let star = Self.sparkle, let root = container.layer {
            // 魔杖图标位于面板左上角
            let emitter = CAEmitterLayer()
            emitter.emitterPosition = CGPoint(x: pad + 30, y: pad + frame.height - 30)
            emitter.emitterShape = .point
            emitter.renderMode = .additive
            let cell = CAEmitterCell()
            cell.contents = star
            cell.birthRate = 160
            cell.lifetime = 0.55
            cell.lifetimeRange = 0.2
            cell.velocity = 260
            cell.velocityRange = 120
            cell.emissionLongitude = .pi / 5
            cell.emissionRange = .pi / 2.2
            cell.yAcceleration = -220
            cell.scale = 0.9
            cell.scaleRange = 0.5
            cell.scaleSpeed = -1.2
            cell.alphaSpeed = -1.6
            cell.spin = 4
            cell.spinRange = 6
            cell.color = Theme.accent.usingColorSpace(.sRGB)?.cgColor
            cell.redRange = 0.3
            cell.greenRange = 0.2
            emitter.emitterCells = [cell]
            root.addSublayer(emitter)
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.12) { emitter.birthRate = 0 }
        }

        let end = iv.frame.insetBy(dx: frame.width * 0.03, dy: frame.height * 0.03).offsetBy(dx: 0, dy: 40)
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = reduce ? 0.12 : 0.24
            ctx.timingFunction = CAMediaTimingFunction(name: .easeIn)
            iv.animator().alphaValue = 0
            if !reduce { iv.animator().frame = end }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + (reduce ? 0.15 : 0.75)) { ghost.orderOut(nil) }
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
