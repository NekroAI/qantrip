import AppKit
import Carbon
import QantripCore

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var hotKey: HotKey!
    private var launcher: LauncherController!
    private var engine: Engine!
    private var statusItem: NSStatusItem!

    /// 用户模板目录（docs/decisions.md D11）
    static let templatesDir: URL = {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Qantrip/templates", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()

    func applicationDidFinishLaunching(_ notification: Notification) {
        let usage = Self.templatesDir.deletingLastPathComponent().appendingPathComponent("usage.json")
        engine = Engine(userDir: Self.templatesDir.path, usagePath: usage.path)
        launcher = LauncherController(engine: engine)

        // 默认 ⌃Q（docs/decisions.md D5，待冲突实测定稿）
        hotKey = HotKey(keyCode: UInt32(kVK_ANSI_Q), modifiers: UInt32(controlKey))
        hotKey.onPress = { [weak self] t in self?.launcher.toggle(eventTime: t, source: "hotkey") }

        setupStatusItem()
        setupDebugHooks()

        SpikeLog.event("launch", [
            "core_version": coreVersion(),
            "hotkey_status": Int(hotKey.status),
            "ax_trusted": Inserter.isTrusted,
            "templates": engine.templates().count,
            "template_problems": engine.problems(),
        ])
    }

    private func setupStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.image = NSImage(systemSymbolName: "wand.and.stars", accessibilityDescription: "Qantrip")
        let menu = NSMenu()
        menu.delegate = self
        statusItem.menu = menu
    }

    /// 仅用于自动化测试：`defaults write ai.nekro.qantrip debugHooks -bool true` 后生效。
    /// 正式版本必须移除——它允许本机其他进程借用本应用的辅助功能权限插入任意文本。
    private func setupDebugHooks() {
        guard UserDefaults.standard.bool(forKey: "debugHooks") else { return }
        let center = DistributedNotificationCenter.default()
        let hooks: [(String, Selector)] = [
            ("ai.nekro.qantrip.debug.toggle", #selector(debugToggle(_:))),
            ("ai.nekro.qantrip.debug.insert", #selector(debugInsert(_:))),
            ("ai.nekro.qantrip.debug.status", #selector(debugStatus(_:))),
            ("ai.nekro.qantrip.debug.type", #selector(debugType(_:))),
            ("ai.nekro.qantrip.debug.confirm", #selector(debugConfirm(_:))),
            ("ai.nekro.qantrip.debug.param", #selector(debugParam(_:))),
        ]
        for (name, sel) in hooks {
            center.addObserver(self, selector: sel, name: .init(name), object: nil,
                               suspensionBehavior: .deliverImmediately)
        }
    }

    @objc private func debugToggle(_ note: Notification) {
        let t = (note.userInfo?["t"] as? Double) ?? ProcessInfo.processInfo.systemUptime
        launcher.toggle(eventTime: t, source: "debug")
    }

    /// userInfo: text（直接插入的文本）或 input（按面板输入渲染）；expand 为 "1" 时展开粘贴
    @objc private func debugInsert(_ note: Notification) {
        let info = note.userInfo ?? [:]
        let text = (info["text"] as? String) ?? (info["input"] as? String).flatMap { engine.render(input: $0, selected: nil) }
        guard let text else {
            SpikeLog.event("debug_insert_error", ["reason": "render failed"])
            return
        }
        launcher.insert(text, target: NSWorkspace.shared.frontmostApplication, expand: info["expand"] as? String == "1")
    }

    /// 打开面板并把输入框设置为指定文本（用于截图检查各种输入状态）
    @objc private func debugType(_ note: Notification) {
        let text = (note.userInfo?["text"] as? String) ?? ""
        if !launcher.isVisible { launcher.show(eventTime: ProcessInfo.processInfo.systemUptime, source: "debug") }
        guard !text.isEmpty else { return }
        launcher.model.field.setText(text)
        launcher.model.textDidChange(text)
        if let n = Int((note.userInfo?["down"] as? String) ?? "0"), n > 0 {
            for _ in 0..<n { launcher.model.move(1) }
        }
    }

    /// 模拟回车（会记录使用）
    @objc private func debugConfirm(_ note: Notification) {
        launcher.model.confirm(expand: false)
    }

    /// 模拟点击参数标签：name + op（toggle / set:<v> / choice:<v> / clear / begin）
    @objc private func debugParam(_ note: Notification) {
        guard let name = note.userInfo?["name"] as? String, let op = note.userInfo?["op"] as? String else { return }
        let edit: ParamEdit
        if op == "toggle" { edit = .toggle }
        else if op == "clear" { edit = .clear }
        else if op == "begin" { edit = .begin }
        else if op.hasPrefix("set:") { edit = .set(value: String(op.dropFirst(4))) }
        else if op.hasPrefix("choice:") { edit = .toggleChoice(value: String(op.dropFirst(7))) }
        else { return }
        launcher.model.editParam(name, edit)
    }

    @objc private func debugStatus(_ note: Notification) {
        SpikeLog.event("status", [
            "ax_trusted": Inserter.isTrusted,
            "hotkey_status": Int(hotKey.status),
            "rss_mb": Self.residentMB(),
        ])
    }

    static func residentMB() -> Double {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return kr == KERN_SUCCESS ? (Double(info.phys_footprint) / 1_048_576 * 10).rounded() / 10 : -1
    }
}

extension AppDelegate: NSMenuDelegate {
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        menu.addItem(withTitle: "打开面板（⌃Q）", action: #selector(openPanel), keyEquivalent: "").target = self
        menu.addItem(.separator())

        let ascii = menu.addItem(withTitle: "唤起时切换到英文输入法", action: #selector(toggleASCII), keyEquivalent: "")
        ascii.target = self
        ascii.state = InputSourceGuard.enabled ? .on : .off
        let expand = menu.addItem(withTitle: "⌘↩ 展开粘贴（Claude Code）", action: #selector(toggleExpand), keyEquivalent: "")
        expand.target = self
        expand.state = Inserter.expandPasteEnabled ? .on : .off
        menu.addItem(.separator())

        let count = engine.templates().count
        menu.addItem(withTitle: "模板：\(count) 个", action: nil, keyEquivalent: "")
        for p in engine.problems().prefix(5) {
            menu.addItem(withTitle: "⚠︎ \(p)", action: nil, keyEquivalent: "")
        }
        menu.addItem(withTitle: "打开模板目录", action: #selector(openTemplates), keyEquivalent: "").target = self
        menu.addItem(withTitle: "重新加载模板", action: #selector(reloadTemplates), keyEquivalent: "r").target = self
        menu.addItem(withTitle: "清除使用记录", action: #selector(clearUsage), keyEquivalent: "").target = self
        menu.addItem(.separator())

        let hk = hotKey.isRegistered ? "全局快捷键：⌃Q 已注册" : "全局快捷键注册失败（\(hotKey.status)）"
        menu.addItem(withTitle: hk, action: nil, keyEquivalent: "")
        if Inserter.isTrusted {
            menu.addItem(withTitle: "辅助功能权限：已授予（自动粘贴）", action: nil, keyEquivalent: "")
        } else {
            menu.addItem(withTitle: "辅助功能权限：未授予（只复制模式）", action: nil, keyEquivalent: "")
            menu.addItem(withTitle: "授予辅助功能权限…", action: #selector(requestTrust), keyEquivalent: "").target = self
        }
        menu.addItem(withTitle: "Nekro Qantrip Demo · core \(coreVersion())", action: nil, keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: "退出", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
    }

    @objc private func openPanel() {
        launcher.show(eventTime: ProcessInfo.processInfo.systemUptime, source: "menu")
    }

    @objc private func toggleASCII() {
        UserDefaults.standard.set(!InputSourceGuard.enabled, forKey: "forceASCIIInput")
    }

    @objc private func toggleExpand() {
        Inserter.expandPasteEnabled.toggle()
    }

    @objc private func openTemplates() {
        NSWorkspace.shared.open(Self.templatesDir)
    }

    @objc private func clearUsage() {
        engine.clearUsage()
    }

    @objc private func reloadTemplates() {
        engine.reload()
    }

    @objc private func requestTrust() {
        Inserter.requestTrust()
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
