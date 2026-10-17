import AppKit
import QantripCore
import SwiftUI

/// 应用级服务，供设置、模板库、引导等窗口使用。
final class AppServices {
    static var shared: AppServices!
    let engine: Engine
    let hotKey: HotKey
    let launcher: LauncherController
    let templatesDir: URL

    init(engine: Engine, hotKey: HotKey, launcher: LauncherController, templatesDir: URL) {
        self.engine = engine
        self.hotKey = hotKey
        self.launcher = launcher
        self.templatesDir = templatesDir
    }
}

/// 普通窗口管理。本应用平时是菜单栏应用（accessory）；打开窗口时临时切到 regular，
/// 让窗口能正常获得焦点并出现在 ⌘Tab 中，全部关闭后切回。
final class WindowManager: NSObject, NSWindowDelegate {
    static let shared = WindowManager()
    private var windows: [String: NSWindow] = [:]

    func show<V: View>(_ id: String, title: String, size: NSSize, resizable: Bool = false, @ViewBuilder content: () -> V) {
        if let w = windows[id] {
            activate()
            w.makeKeyAndOrderFront(nil)
            return
        }
        var style: NSWindow.StyleMask = [.titled, .closable, .miniaturizable]
        // 可调整大小的窗口使用 SwiftUI 工具栏（NavigationSplitView），需要全尺寸内容视图，
        // SwiftUI 才能正确计算工具栏下方的安全区，否则内容顶部会被工具栏遮挡
        if resizable { style.formUnion([.resizable, .fullSizeContentView]) }
        let w = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: style, backing: .buffered, defer: false)
        w.title = title
        if resizable { w.toolbarStyle = .unified }
        w.contentViewController = NSHostingController(rootView: content())
        w.setContentSize(size)
        w.isReleasedWhenClosed = false
        w.delegate = self
        w.center()
        w.identifier = NSUserInterfaceItemIdentifier(id)
        windows[id] = w
        activate()
        w.makeKeyAndOrderFront(nil)
    }

    func close(_ id: String) {
        windows[id]?.close()
    }

    private func activate() {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }

    func windowWillClose(_ notification: Notification) {
        guard let w = notification.object as? NSWindow, let id = w.identifier?.rawValue else { return }
        windows[id] = nil
        if windows.isEmpty {
            DispatchQueue.main.async { NSApp.setActivationPolicy(.accessory) }
        }
    }
}
