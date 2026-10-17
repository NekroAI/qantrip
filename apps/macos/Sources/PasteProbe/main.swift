import AppKit

// Spike 测试用探针：一个普通文本窗口，内容每次变化都写入
// ~/Library/Logs/Qantrip/probe.txt，供测试脚本比对插入结果。
// 收到分布式通知 ai.nekro.qantrip.probe.reset 时清空并重新置前。

let outURL = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Logs/Qantrip/probe.txt")

final class ProbeDelegate: NSObject, NSApplicationDelegate, NSTextViewDelegate {
    var window: NSWindow!
    var textView: NSTextView!

    func applicationDidFinishLaunching(_ notification: Notification) {
        try? FileManager.default.createDirectory(at: outURL.deletingLastPathComponent(),
                                                 withIntermediateDirectories: true)
        window = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 700, height: 500),
                          styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "PasteProbe"
        let scroll = NSTextView.scrollableTextView()
        textView = (scroll.documentView as! NSTextView)
        textView.isRichText = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.delegate = self
        window.contentView = scroll
        reset()

        DistributedNotificationCenter.default().addObserver(
            self, selector: #selector(reset), name: .init("ai.nekro.qantrip.probe.reset"), object: nil,
            suspensionBehavior: .deliverImmediately)
    }

    @objc func reset() {
        textView.string = ""
        write()
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(textView)
    }

    func textDidChange(_ notification: Notification) { write() }

    func write() {
        try? textView.string.write(to: outURL, atomically: true, encoding: .utf8)
    }
}

let app = NSApplication.shared
let delegate = ProbeDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
