import Carbon

/// 唤起面板时切换到英文（ASCII）输入源，关闭时恢复（docs/decisions.md D9）。
final class InputSourceGuard {
    private var original: TISInputSource?
    private var switchedTo: TISInputSource?

    static var enabled: Bool { AppSettings.shared.forceASCIIInput }

    func engage() {
        original = nil
        switchedTo = nil
        guard Self.enabled,
              let current = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue(),
              !Self.isASCII(current),
              let ascii = TISCopyCurrentASCIICapableKeyboardLayoutInputSource()?.takeRetainedValue()
        else { return }
        if TISSelectInputSource(ascii) == noErr {
            original = current
            switchedTo = ascii
        }
    }

    func restore() {
        defer { original = nil; switchedTo = nil }
        guard let original, let switchedTo,
              let current = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue() else { return }
        // 用户在面板中自己切换过输入法时，以用户的选择为准，不再恢复
        if Self.id(of: current) == Self.id(of: switchedTo) {
            TISSelectInputSource(original)
        }
    }

    private static func isASCII(_ source: TISInputSource) -> Bool {
        guard let ptr = TISGetInputSourceProperty(source, kTISPropertyInputSourceIsASCIICapable) else { return false }
        return Unmanaged<CFBoolean>.fromOpaque(ptr).takeUnretainedValue() == kCFBooleanTrue
    }

    private static func id(of source: TISInputSource) -> String {
        guard let ptr = TISGetInputSourceProperty(source, kTISPropertyInputSourceID) else { return "" }
        return Unmanaged<CFString>.fromOpaque(ptr).takeUnretainedValue() as String
    }
}
