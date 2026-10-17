import AppKit
import Carbon
import QantripCore
import ServiceManagement

/// 界面与内置模板的语言（docs/decisions.md D29）。auto：界面跟随系统，模板跟随界面。
enum LanguageChoice: String, CaseIterable, Identifiable {
    case auto, zh, en
    var id: String { rawValue }
}

/// 界面文字：按当前界面语言选择中文或英文。在 SwiftUI 视图中调用时会随语言设置自动刷新。
func L(_ zh: String, _ en: String) -> String {
    AppSettings.shared.uiLocale == .en ? en : zh
}

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

    var interfaceLanguage: LanguageChoice {
        didSet { d.set(interfaceLanguage.rawValue, forKey: "interfaceLanguage"); onLanguageChange?() }
    }
    var templateLanguage: LanguageChoice {
        didSet { d.set(templateLanguage.rawValue, forKey: "templateLanguage"); onLanguageChange?() }
    }
    @ObservationIgnored var onLanguageChange: (() -> Void)?
    static let languageChanged = Notification.Name("ai.nekro.qantrip.languageChanged")

    /// 实际使用的界面语言
    var uiLocale: QantripCore.Locale {
        switch interfaceLanguage {
        case .zh: return .zh
        case .en: return .en
        case .auto: return (Foundation.Locale.preferredLanguages.first ?? "en").hasPrefix("zh") ? .zh : .en
        }
    }
    /// 实际使用的内置模板语言
    var templateLocale: QantripCore.Locale {
        switch templateLanguage {
        case .zh: return .zh
        case .en: return .en
        case .auto: return uiLocale
        }
    }

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
        interfaceLanguage = LanguageChoice(rawValue: ud.string(forKey: "interfaceLanguage") ?? "") ?? .auto
        templateLanguage = LanguageChoice(rawValue: ud.string(forKey: "templateLanguage") ?? "") ?? .auto
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
        let names: [String: String] = ["60": L("切换输入法", "Switch input source"), "61": L("切换到下一个输入法", "Switch to next input source"), "64": L("聚焦（Spotlight）", "Spotlight"), "65": L("访达搜索窗口", "Finder search window"),
                                        "32": L("调度中心", "Mission Control"), "33": L("应用程序窗口", "Application windows"), "36": L("显示桌面", "Show Desktop"), "28": L("截屏", "Screenshot"), "30": L("截屏（选区）", "Screenshot (selection)")]
        for (id, value) in dict {
            guard let v = value as? [String: Any], (v["enabled"] as? Bool) == true,
                  let params = (v["value"] as? [String: Any])?["parameters"] as? [Int], params.count == 3 else { continue }
            let flags = NSEvent.ModifierFlags(rawValue: UInt(params[2]))
            if UInt32(params[1]) == keyCode, Self.carbon(flags) == modifiers {
                return L("与系统快捷键「\(names[id] ?? L("系统功能 #\(id)", "System function #\(id)"))」冲突", "Conflicts with the system shortcut “\(names[id] ?? L("系统功能 #\(id)", "System function #\(id)"))”")
            }
        }
        return nil
    }
}
