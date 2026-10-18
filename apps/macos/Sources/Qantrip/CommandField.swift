import AppKit
import CoreText
import QantripCore
import SwiftUI

/// 命令输入框：AppKit `NSTextField`，带语法高亮与灰字补全。
///
/// 按键命令经 `doCommandBy` 分发；输入法组字期间系统不会把 Enter / 方向键交给我们。
/// ⌘↩ 不是文本命令，在 `performKeyEquivalent` 中处理。
final class CommandField: NSTextField, NSTextFieldDelegate {
    weak var model: LauncherModel?
    private let ghost = GhostView()
    private var lastAnalysis: Analysis?

    /// 占位文字不用 placeholderString：输入框自带的占位与光标基线不一致（光标按等宽字体的行高绘制），
    /// 改为和灰字补全一样画在字段编辑器内、与光标同一基线
    private var placeholder = ""

    private func updatePlaceholder() {
        placeholder = L("施放一个戏法…  如 r security 重点检查认证", "Cast a spell…  e.g. r security focus on auth")
        updateGhost()
    }

    init() {
        super.init(frame: .zero)
        isBordered = false
        drawsBackground = false
        focusRingType = .none
        font = Theme.commandFont
        updatePlaceholder()
        NotificationCenter.default.addObserver(forName: AppSettings.languageChanged, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.updatePlaceholder() }
        }
        cell?.usesSingleLineMode = true
        cell?.wraps = false
        cell?.isScrollable = true
        delegate = self

        ghost.isHidden = true
    }

    required init?(coder: NSCoder) { fatalError() }

    private var editor: NSTextView? { currentEditor() as? NSTextView }

    /// 最近一次设置的光标位置（重新获得焦点时恢复）
    private(set) var caret = 0

    func setText(_ text: String, caret: Int? = nil) {
        stringValue = text
        self.caret = caret ?? (text as NSString).length
        if let editor {
            editor.string = text
            editor.selectedRange = NSRange(location: self.caret, length: 0)
        }
    }

    // MARK: 按键

    func controlTextDidChange(_ obj: Notification) {
        // 组字期间不重新解析，避免拼音字母触发匹配和高亮闪烁
        if editor?.hasMarkedText() == true {
            ghost.isHidden = true
            return
        }
        model?.textDidChange(stringValue)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard let model else { return false }
        switch selector {
        case #selector(NSResponder.insertNewline(_:)): model.confirm(expand: false)
        case #selector(NSResponder.moveUp(_:)): model.move(-1)
        case #selector(NSResponder.moveDown(_:)): model.move(1)
        case #selector(NSResponder.insertTab(_:)): model.complete()
        case #selector(NSResponder.cancelOperation(_:)): model.onCancel?()
        case #selector(NSResponder.moveRight(_:)), #selector(NSResponder.moveToEndOfLine(_:)):
            // 光标在末尾时 → 接受灰字补全（类似 fish shell）
            guard let editor, editor.selectedRange.location == (stringValue as NSString).length,
                  let g = lastAnalysis?.ghost, !g.isEmpty else { return false }
            setText(stringValue + g)
            model.textDidChange(stringValue)
        default: return false
        }
        return true
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if event.keyCode == 36 || event.keyCode == 76, mods == .command, editor?.hasMarkedText() != true {
            model?.confirm(expand: true)
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    // MARK: 高亮

    func applyHighlighting(_ a: Analysis) {
        lastAnalysis = a
        guard let editor, let storage = editor.textStorage, !editor.hasMarkedText() else { return }
        let full = NSRange(location: 0, length: storage.length)
        let base: [NSAttributedString.Key: Any] = [.font: Theme.commandFont, .foregroundColor: NSColor.labelColor]
        storage.beginEditing()
        storage.setAttributes(base, range: full)
        for t in a.tokens {
            let r = NSRange(location: Int(t.start), length: Int(t.end - t.start))
            guard NSMaxRange(r) <= storage.length else { continue }
            switch t.kind {
            case .command:
                storage.addAttributes([.foregroundColor: Theme.accent, .font: Theme.commandFontBold], range: r)
            case .commandUnknown, .invalid:
                storage.addAttributes([.foregroundColor: Theme.error], range: r)
            case .optionName, .shortFlag:
                storage.addAttributes([.foregroundColor: Theme.option], range: r)
            case .optionValue, .positional:
                storage.addAttributes([.foregroundColor: Theme.value], range: r)
            case .extraMarker:
                storage.addAttributes([.foregroundColor: NSColor.tertiaryLabelColor], range: r)
            case .extra:
                storage.addAttributes([.foregroundColor: Theme.extra], range: r)
            }
        }
        // 错误加波浪下划线；正在输入的词不标，避免边打字边报错
        for d in a.diagnostics where !d.atCursor && d.end > d.start {
            let r = NSRange(location: Int(d.start), length: Int(d.end - d.start))
            guard NSMaxRange(r) <= storage.length else { continue }
            let color = d.severity == .error ? Theme.error : Theme.warning
            storage.addAttributes([
                .underlineStyle: NSUnderlineStyle.thick.rawValue | NSUnderlineStyle.patternDot.rawValue,
                .underlineColor: color,
            ], range: r)
        }
        storage.endEditing()
        editor.typingAttributes = base
        updateGhost()
    }

    /// 灰字补全画在字段编辑器内部、与输入文字同一行，保证基线一致。
    private func updateGhost() {
        let empty = stringValue.isEmpty
        guard let editor, let lm = editor.layoutManager, let tc = editor.textContainer,
              let g = empty ? placeholder : lastAnalysis?.ghost, !g.isEmpty,
              editor.selectedRange.location == (stringValue as NSString).length,
              !editor.hasMarkedText()
        else {
            ghost.isHidden = true
            return
        }
        if ghost.superview !== editor { editor.addSubview(ghost) }
        lm.ensureLayout(for: tc)
        let used = lm.usedRect(for: tc)
        let line = lm.extraLineFragmentRect.isEmpty && lm.numberOfGlyphs > 0
            ? lm.lineFragmentRect(forGlyphAt: max(lm.numberOfGlyphs - 1, 0), effectiveRange: nil)
            : NSRect(x: 0, y: used.minY, width: 0, height: used.height)
        ghost.text = NSAttributedString(string: g, attributes: [
            .font: Theme.commandFont, .foregroundColor: NSColor.tertiaryLabelColor,
        ])
        // 基线：行片段顶部 + 字形在行内的基线偏移（取最后一个字形；空输入时按字体度量估算）
        let font = Theme.commandFont
        let baselineInLine = lm.numberOfGlyphs > 0
            ? lm.location(forGlyphAt: lm.numberOfGlyphs - 1).y
            : font.ascender
        let baseline = line.minY + editor.textContainerOrigin.y + baselineInLine
        let width = ceil(ghost.text.size().width) + 2
        let top = baseline - ceil(font.ascender) - 2
        let x = (lm.numberOfGlyphs == 0 ? 0 : used.maxX) + editor.textContainerOrigin.x
        ghost.baseline = baseline - top
        ghost.frame = NSRect(x: x, y: top, width: width, height: ceil(font.ascender - font.descender) + 4)
        // 超出可见宽度时不显示（占位文字总是显示）
        ghost.isHidden = !empty && convert(NSPoint(x: ghost.frame.maxX, y: 0), from: editor).x > bounds.width
    }
}

/// 在翻转坐标系中从左上角绘制文字，与 NSTextView 的行布局一致。
final class GhostView: NSView {
    var text = NSAttributedString() { didSet { needsDisplay = true } }
    /// 视图内基线的 y（翻转坐标）
    var baseline: CGFloat = 0 { didSet { needsDisplay = true } }
    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        ctx.saveGState()
        ctx.translateBy(x: 0, y: baseline)
        ctx.scaleBy(x: 1, y: -1)
        ctx.textPosition = .zero
        CTLineDraw(CTLineCreateWithAttributedString(text), ctx)
        ctx.restoreGState()
    }
}

struct CommandFieldView: NSViewRepresentable {
    let field: CommandField
    func makeNSView(context: Context) -> CommandField { field }
    func updateNSView(_ nsView: CommandField, context: Context) {}
}
