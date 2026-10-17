import Carbon
import Foundation

/// 基于 Carbon `RegisterEventHotKey` 的全局热键。该 API 不需要任何系统权限。
final class HotKey {
    /// 回调参数为按键事件发生时刻（systemUptime 时间基准）。
    var onPress: ((TimeInterval) -> Void)?
    private(set) var status: OSStatus = noErr
    private(set) var spec: HotKeySpec
    private var ref: EventHotKeyRef?
    private var handler: EventHandlerRef?

    init(_ spec: HotKeySpec) {
        self.spec = spec
        var eventSpec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        let me = Unmanaged.passUnretained(self).toOpaque()
        InstallEventHandler(GetApplicationEventTarget(), { _, event, userData in
            guard let event, let userData else { return noErr }
            let hk = Unmanaged<HotKey>.fromOpaque(userData).takeUnretainedValue()
            hk.onPress?(GetEventTime(event))
            return noErr
        }, 1, &eventSpec, me, &handler)
        register(spec)
    }

    var isRegistered: Bool { status == noErr && ref != nil }

    /// 重新注册为新的组合键；失败时保留错误码（例如被其他应用占用）。
    @discardableResult
    func register(_ spec: HotKeySpec) -> Bool {
        if let ref { UnregisterEventHotKey(ref) }
        ref = nil
        self.spec = spec
        let id = EventHotKeyID(signature: OSType(0x5154_5250) /* 'QTRP' */, id: 1)
        status = RegisterEventHotKey(spec.keyCode, spec.modifiers, id, GetApplicationEventTarget(), 0, &ref)
        return isRegistered
    }

    /// 暂停（录制新快捷键时，避免按下旧组合直接唤起面板）。
    func suspend() {
        if let ref { UnregisterEventHotKey(ref) }
        ref = nil
    }

    deinit {
        if let ref { UnregisterEventHotKey(ref) }
        if let handler { RemoveEventHandler(handler) }
    }
}
