import AppKit
import SwiftUI

/// 模板源码编辑器：等宽字体，关闭智能替换，简单高亮 frontmatter 与 Jinja 标签。
struct CodeEditor: NSViewRepresentable {
    @Binding var text: String
    var editable = true
    var fontSize: CGFloat = 13

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        let tv = scroll.documentView as! NSTextView
        tv.isRichText = false
        tv.allowsUndo = true
        tv.isAutomaticQuoteSubstitutionEnabled = false
        tv.isAutomaticDashSubstitutionEnabled = false
        tv.isAutomaticTextReplacementEnabled = false
        tv.isAutomaticSpellingCorrectionEnabled = false
        tv.isContinuousSpellCheckingEnabled = false
        tv.smartInsertDeleteEnabled = false
        tv.textContainerInset = NSSize(width: 8, height: 10)
        tv.font = Theme.mono(fontSize)
        tv.backgroundColor = .textBackgroundColor
        tv.delegate = context.coordinator
        tv.string = text
        context.coordinator.highlight(tv)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        let tv = scroll.documentView as! NSTextView
        context.coordinator.parent = self
        tv.isEditable = editable
        if tv.string != text {
            let sel = tv.selectedRanges
            tv.string = text
            tv.selectedRanges = sel.filter { NSMaxRange($0.rangeValue) <= (text as NSString).length }
            context.coordinator.highlight(tv)
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: CodeEditor
        init(_ parent: CodeEditor) { self.parent = parent }

        func textDidChange(_ notification: Notification) {
            guard let tv = notification.object as? NSTextView else { return }
            parent.text = tv.string
            highlight(tv)
        }

        private static let jinja = try! NSRegularExpression(pattern: #"\{\{.*?\}\}|\{%.*?%\}|\{#.*?#\}"#, options: [.dotMatchesLineSeparators])

        func highlight(_ tv: NSTextView) {
            guard let storage = tv.textStorage else { return }
            let ns = storage.string as NSString
            let full = NSRange(location: 0, length: ns.length)
            storage.beginEditing()
            storage.setAttributes([.font: Theme.mono(parent.fontSize), .foregroundColor: NSColor.labelColor], range: full)
            // frontmatter：开头的 --- … ---
            if ns.hasPrefix("---\n") {
                let end = ns.range(of: "\n---", options: [], range: NSRange(location: 4, length: max(0, ns.length - 4)))
                if end.location != NSNotFound {
                    storage.addAttribute(.foregroundColor, value: NSColor.secondaryLabelColor,
                                         range: NSRange(location: 0, length: NSMaxRange(end)))
                }
            }
            for m in Self.jinja.matches(in: storage.string, range: full) {
                storage.addAttribute(.foregroundColor, value: Theme.accent, range: m.range)
            }
            storage.endEditing()
            tv.typingAttributes = [.font: Theme.mono(parent.fontSize), .foregroundColor: NSColor.labelColor]
        }
    }
}
