import AppKit
import QantripCore
import SwiftUI

@Observable
final class LauncherModel {
    var query = "" { didSet { refresh() } }
    var results: [TemplateSummary] = []
    var selected = 0
    /// 只有模糊匹配时，第一次 Enter 先补全命令词（PRD 3.3）。
    var hint = ""

    @ObservationIgnored var onSubmit: ((String) -> Void)?
    @ObservationIgnored var onCancel: (() -> Void)?
    @ObservationIgnored let fieldView = CommandField()

    init() {
        fieldView.model = self
        refresh()
    }

    func reset() {
        query = ""
        fieldView.stringValue = ""
        selected = 0
    }

    private func refresh() {
        results = search(input: query)
        selected = 0
        hint = results.first.map { $0.tier == .fuzzy ? "模糊匹配：Enter 补全为 \($0.name)" : "" } ?? "无匹配模板"
    }

    func move(_ delta: Int) {
        guard !results.isEmpty else { return }
        selected = (selected + delta + results.count) % results.count
    }

    func confirm() {
        guard results.indices.contains(selected) else { return }
        let item = results[selected]
        let words = query.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: true)
        if item.tier == .fuzzy, words.first.map(String.init) != item.name {
            // 补全命令词，保留后续附加文本
            let rest = words.count > 1 ? " " + words[1] : " "
            fieldView.stringValue = item.name + rest
            query = fieldView.stringValue
            fieldView.currentEditor()?.selectedRange = NSRange(location: (query as NSString).length, length: 0)
            return
        }
        guard let text = render(templateId: item.id, input: query) else { return }
        onSubmit?(text)
    }

    func complete() {
        guard results.indices.contains(selected) else { return }
        let words = query.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: true)
        let rest = words.count > 1 ? " " + words[1] : " "
        fieldView.stringValue = results[selected].name + rest
        query = fieldView.stringValue
        fieldView.currentEditor()?.selectedRange = NSRange(location: (query as NSString).length, length: 0)
    }
}

/// 输入框用 AppKit `NSTextField`：按键命令经 `doCommandBy` 分发，
/// 输入法组字期间（marked text）系统不会把 Enter / 方向键交给我们，避免误触发。
final class CommandField: NSTextField, NSTextFieldDelegate {
    weak var model: LauncherModel?

    init() {
        super.init(frame: .zero)
        isBordered = false
        drawsBackground = false
        focusRingType = .none
        font = .systemFont(ofSize: 22, weight: .regular)
        placeholderString = "输入命令，如 r security 重点检查认证"
        cell?.usesSingleLineMode = true
        cell?.wraps = false
        cell?.isScrollable = true
        delegate = self
    }

    required init?(coder: NSCoder) { fatalError() }

    func controlTextDidChange(_ obj: Notification) {
        model?.query = stringValue
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard let model else { return false }
        switch selector {
        case #selector(NSResponder.insertNewline(_:)): model.confirm()
        case #selector(NSResponder.moveUp(_:)): model.move(-1)
        case #selector(NSResponder.moveDown(_:)): model.move(1)
        case #selector(NSResponder.insertTab(_:)): model.complete()
        case #selector(NSResponder.cancelOperation(_:)): model.onCancel?()
        default: return false
        }
        return true
    }
}

struct CommandFieldView: NSViewRepresentable {
    let field: CommandField
    func makeNSView(context: Context) -> CommandField { field }
    func updateNSView(_ nsView: CommandField, context: Context) {}
}

struct LauncherView: View {
    @Bindable var model: LauncherModel

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "wand.and.stars").font(.system(size: 20)).foregroundStyle(.secondary)
                CommandFieldView(field: model.fieldView).frame(height: 30)
            }
            .padding(.horizontal, 16).padding(.vertical, 14)
            Divider()
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(spacing: 2) {
                        ForEach(Array(model.results.enumerated()), id: \.element.id) { i, item in
                            row(item, selected: i == model.selected).id(i)
                        }
                    }
                    .padding(6)
                }
                .onChange(of: model.selected) { _, new in proxy.scrollTo(new) }
            }
            Divider()
            HStack {
                Text(model.hint).foregroundStyle(.secondary)
                Spacer()
                Text("↑↓ 选择  Tab 补全  ↩ 插入  esc 关闭").foregroundStyle(.tertiary)
            }
            .font(.system(size: 12)).padding(.horizontal, 14).padding(.vertical, 8)
        }
        .frame(width: LauncherController.size.width, height: LauncherController.size.height)
        .background(.regularMaterial)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
    }

    private func row(_ item: TemplateSummary, selected: Bool) -> some View {
        HStack(spacing: 10) {
            Text(item.name).font(.system(size: 15, weight: .semibold, design: .monospaced))
            if !item.aliases.isEmpty {
                Text(item.aliases.joined(separator: " · ")).font(.system(size: 12)).foregroundStyle(.secondary)
            }
            Spacer()
            Text(item.description).font(.system(size: 13)).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 12).padding(.vertical, 9)
        .background(selected ? Color.accentColor.opacity(0.22) : .clear,
                    in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}
