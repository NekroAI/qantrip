import AppKit
import Carbon
import ServiceManagement

/// 应用设置（UserDefaults）。热键等需要即时生效的项通过回调通知。
@Observable
final class AppSettings {
    static let shared = AppSettings()
    private let d = UserDefaults.standard

    /// 默认 ⌃Q（docs/decisions.md D5）
    static let defaultHotKey = HotKeySpec(keyCode: UInt32(kVK_ANSI_Q), modifiers: UInt32(controlKey))

    var hotKey: HotKeySpec {
        didSet {
            d.set(Int(hotKey.keyCode), forKey: "hotKeyCode")
            d.set(Int(hotKey.modifiers), forKey: "hotKeyModifiers")
            onHotKeyChange?(hotKey)
        }
    }
    @ObservationIgnored var onHotKeyChange: ((HotKeySpec) -> Void)?

    var forceASCIIInput: Bool { didSet { d.set(forceASCIIInput, forKey: "forceASCIIInput") } }
    var expandPasteEnabled: Bool { didSet { d.set(expandPasteEnabled, forKey: "expandPasteEnabled") } }
    var expandPasteIntervalMs: Double { didSet { d.set(expandPasteIntervalMs, forKey: "expandPasteIntervalMs") } }
    /// 只复制模式：不模拟粘贴，不需要辅助功能权限
    var copyOnly: Bool { didSet { d.set(copyOnly, forKey: "copyOnly") } }
    var recordUsage: Bool { didSet { d.set(recordUsage, forKey: "recordUsage") } }
    var onboardingDone: Bool { didSet { d.set(onboardingDone, forKey: "onboardingDone") } }

    var launchAtLogin: Bool {
        get { SMAppService.mainApp.status == .enabled }
        set {
            do {
                if newValue { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
            } catch {
                NSLog("Qantrip: 开机启动设置失败：\(error)")
            }
            launchAtLoginRevision += 1
        }
    }
    /// 让 SwiftUI 在开机启动状态变化后刷新
    var launchAtLoginRevision = 0

    private init() {
        let ud = UserDefaults.standard
        func bool(_ k: String, _ def: Bool) -> Bool { ud.object(forKey: k) as? Bool ?? def }
        let code = ud.object(forKey: "hotKeyCode") as? Int
        let mods = ud.object(forKey: "hotKeyModifiers") as? Int
        hotKey = (code != nil && mods != nil) ? HotKeySpec(keyCode: UInt32(code!), modifiers: UInt32(mods!)) : Self.defaultHotKey
        forceASCIIInput = bool("forceASCIIInput", true)
        expandPasteEnabled = bool("expandPasteEnabled", true)
        expandPasteIntervalMs = ud.object(forKey: "expandPasteIntervalMs") as? Double ?? 250
        copyOnly = bool("copyOnly", false)
        recordUsage = bool("recordUsage", true)
        onboardingDone = bool("onboardingDone", false)
    }
}

/// 热键：Carbon 键码 + Carbon 修饰键掩码。
struct HotKeySpec: Equatable {
    var keyCode: UInt32
    var modifiers: UInt32

    var display: String {
        var s = ""
        if modifiers & UInt32(controlKey) != 0 { s += "⌃" }
        if modifiers & UInt32(optionKey) != 0 { s += "⌥" }
        if modifiers & UInt32(shiftKey) != 0 { s += "⇧" }
        if modifiers & UInt32(cmdKey) != 0 { s += "⌘" }
        return s + Self.keyName(keyCode)
    }

    static func keyName(_ code: UInt32) -> String {
        let special: [UInt32: String] = [
            UInt32(kVK_Space): "Space", UInt32(kVK_Return): "↩", UInt32(kVK_Tab): "⇥", UInt32(kVK_Escape): "⎋",
            UInt32(kVK_F1): "F1", UInt32(kVK_F2): "F2", UInt32(kVK_F3): "F3", UInt32(kVK_F4): "F4",
            UInt32(kVK_F5): "F5", UInt32(kVK_F6): "F6", UInt32(kVK_F7): "F7", UInt32(kVK_F8): "F8",
            UInt32(kVK_F9): "F9", UInt32(kVK_F10): "F10", UInt32(kVK_F11): "F11", UInt32(kVK_F12): "F12",
        ]
        if let s = special[code] { return s }
        guard let source = TISCopyCurrentASCIICapableKeyboardLayoutInputSource()?.takeRetainedValue(),
              let ptr = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else { return "?" }
        let data = Unmanaged<CFData>.fromOpaque(ptr).takeUnretainedValue() as Data
        return data.withUnsafeBytes { raw -> String in
            guard let layout = raw.baseAddress?.assumingMemoryBound(to: UCKeyboardLayout.self) else { return "?" }
            var dead: UInt32 = 0, len = 0
            var chars = [UniChar](repeating: 0, count: 4)
            UCKeyTranslate(layout, UInt16(code), UInt16(kUCKeyActionDisplay), 0, UInt32(LMGetKbdType()),
                           OptionBits(kUCKeyTranslateNoDeadKeysBit), &dead, 4, &len, &chars)
            return String(utf16CodeUnits: chars, count: len).uppercased()
        }
    }

    /// NSEvent 修饰键 → Carbon 掩码
    static func carbon(_ flags: NSEvent.ModifierFlags) -> UInt32 {
        var m: UInt32 = 0
        if flags.contains(.control) { m |= UInt32(controlKey) }
        if flags.contains(.option) { m |= UInt32(optionKey) }
        if flags.contains(.shift) { m |= UInt32(shiftKey) }
        if flags.contains(.command) { m |= UInt32(cmdKey) }
        return m
    }

    /// 与系统快捷键（键盘设置中启用的项）冲突的描述；无冲突返回 nil。
    func systemConflict() -> String? {
        guard let dict = UserDefaults(suiteName: "com.apple.symbolichotkeys")?.dictionary(forKey: "AppleSymbolicHotKeys") else { return nil }
        let names: [String: String] = ["60": "切换输入法", "61": "切换到下一个输入法", "64": "聚焦（Spotlight）", "65": "访达搜索窗口",
                                        "32": "调度中心", "33": "应用程序窗口", "36": "显示桌面", "28": "截屏", "30": "截屏（选区）"]
        for (id, value) in dict {
            guard let v = value as? [String: Any], (v["enabled"] as? Bool) == true,
                  let params = (v["value"] as? [String: Any])?["parameters"] as? [Int], params.count == 3 else { continue }
            let flags = NSEvent.ModifierFlags(rawValue: UInt(params[2]))
            if UInt32(params[1]) == keyCode, Self.carbon(flags) == modifiers {
                return "与系统快捷键「\(names[id] ?? "系统功能 #\(id)")」冲突"
            }
        }
        return nil
    }
}
