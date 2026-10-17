import Carbon
import Foundation

/// 基于 Carbon `RegisterEventHotKey` 的全局热键。该 API 不需要任何系统权限。
final class HotKey {
    /// 回调参数为按键事件发生时刻（systemUptime 时间基准）。
    var onPress: ((TimeInterval) -> Void)?
    private(set) var status: OSStatus = noErr
    private var ref: EventHotKeyRef?
    private var handler: EventHandlerRef?

    init(keyCode: UInt32, modifiers: UInt32) {
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        let me = Unmanaged.passUnretained(self).toOpaque()
        InstallEventHandler(GetApplicationEventTarget(), { _, event, userData in
            guard let event, let userData else { return noErr }
            let hk = Unmanaged<HotKey>.fromOpaque(userData).takeUnretainedValue()
            hk.onPress?(GetEventTime(event))
            return noErr
        }, 1, &spec, me, &handler)

        let id = EventHotKeyID(signature: OSType(0x5154_5250) /* 'QTRP' */, id: 1)
        status = RegisterEventHotKey(keyCode, modifiers, id, GetApplicationEventTarget(), 0, &ref)
    }

    var isRegistered: Bool { status == noErr && ref != nil }

    deinit {
        if let ref { UnregisterEventHotKey(ref) }
        if let handler { RemoveEventHandler(handler) }
    }
}
