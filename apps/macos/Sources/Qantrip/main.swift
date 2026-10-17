import AppKit
import Carbon
import QantripCore

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var hotKey: HotKey!
    private var launcher: LauncherController!
    private var statusItem: NSStatusItem!

    func applicationDidFinishLaunching(_ notification: Notification) {
        launcher = LauncherController()

        // 默认 ⌃Q（docs/decisions.md D5，待冲突实测定稿）
        hotKey = HotKey(keyCode: UInt32(kVK_ANSI_Q), modifiers: UInt32(controlKey))
        hotKey.onPress = { [weak self] t in self?.launcher.toggle(eventTime: t, source: "hotkey") }

        setupStatusItem()
        setupDebugHooks()

        SpikeLog.event("launch", [
            "core_version": coreVersion(),
            "hotkey_status": Int(hotKey.status),
            "ax_trusted": Inserter.isTrusted,
            "paste_keycode": Int(Inserter.keyCode(for: "v") ?? 9999),
        ])
    }

    private func setupStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.image = NSImage(systemSymbolName: "wand.and.stars", accessibilityDescription: "Qantrip")
        let menu = NSMenu()
        menu.delegate = self
        statusItem.menu = menu
    }

    /// 仅用于 Spike 自动化测试：`defaults write ai.nekro.qantrip debugHooks -bool true` 后生效。
    /// 正式版本必须移除——它允许本机其他进程借用本应用的辅助功能权限插入任意文本。
    private func setupDebugHooks() {
        guard UserDefaults.standard.bool(forKey: "debugHooks") else { return }
        let center = DistributedNotificationCenter.default()
        let hooks: [(String, Selector)] = [
            ("ai.nekro.qantrip.debug.toggle", #selector(debugToggle(_:))),
            ("ai.nekro.qantrip.debug.insert", #selector(debugInsert(_:))),
            ("ai.nekro.qantrip.debug.status", #selector(debugStatus(_:))),
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

    @objc private func debugInsert(_ note: Notification) {
        let input = (note.userInfo?["input"] as? String) ?? ""
        guard let id = note.userInfo?["id"] as? String, let text = render(templateId: id, input: input) else {
            SpikeLog.event("debug_insert_error", ["reason": "unknown template"])
            return
        }
        launcher.insert(text, target: NSWorkspace.shared.frontmostApplication)
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
        let hk = hotKey.isRegistered ? "全局快捷键：⌃Q 已注册" : "全局快捷键注册失败（\(hotKey.status)）"
        menu.addItem(withTitle: hk, action: nil, keyEquivalent: "")
        if Inserter.isTrusted {
            menu.addItem(withTitle: "辅助功能权限：已授予（自动粘贴）", action: nil, keyEquivalent: "")
        } else {
            menu.addItem(withTitle: "辅助功能权限：未授予（只复制模式）", action: nil, keyEquivalent: "")
            menu.addItem(withTitle: "授予辅助功能权限…", action: #selector(requestTrust), keyEquivalent: "").target = self
        }
        menu.addItem(withTitle: "Qantrip Spike · core \(coreVersion())", action: nil, keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: "退出", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
    }

    @objc private func openPanel() {
        launcher.show(eventTime: ProcessInfo.processInfo.systemUptime, source: "menu")
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
