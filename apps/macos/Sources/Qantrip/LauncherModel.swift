import AppKit
import QantripCore

/// 面板状态。解析、补全、校验和预览全部来自 Rust 核心的 `analyze`，这里只管选择与编辑。
@Observable
final class LauncherModel {
    private(set) var query = ""
    private(set) var analysis: Analysis
    /// 候选列表中的高亮项
    var selectedIndex = 0
    /// 回车被阻止时递增，用于触发抖动反馈
    private(set) var rejectCount = 0
    var target: NSRunningApplication?

    @ObservationIgnored let engine: Engine
    @ObservationIgnored let field = CommandField()
    /// 提交回调：(最终文本, 是否展开粘贴)
    @ObservationIgnored var onSubmit: ((String, Bool) -> Void)?
    @ObservationIgnored var onCancel: (() -> Void)?
    /// 命令阶段界面高亮的模板；命令词改变时清空
    @ObservationIgnored private var selectedTemplate: String?
    @ObservationIgnored private var commandWord = ""

    init(engine: Engine) {
        self.engine = engine
        analysis = engine.analyze(input: "", selected: nil)
        field.model = self
    }

    var candidates: [Candidate] { analysis.candidates }

    var firstError: Diagnostic? {
        analysis.diagnostics.first { $0.severity == .error && !$0.atCursor }
            ?? analysis.diagnostics.first { $0.severity == .error }
    }

    func reset() {
        selectedTemplate = nil
        commandWord = ""
        field.setText("")
        textDidChange("")
    }

    func textDidChange(_ text: String) {
        query = text
        let cmd = text.split(whereSeparator: \.isWhitespace).first.map(String.init) ?? ""
        if cmd != commandWord {
            commandWord = cmd
            selectedTemplate = nil
        }
        reanalyze(resetSelection: true)
    }

    private func reanalyze(resetSelection: Bool) {
        analysis = engine.analyze(input: query, selected: selectedTemplate)
        if resetSelection { selectedIndex = 0 }
        selectedIndex = min(selectedIndex, max(candidates.count - 1, 0))
        field.applyHighlighting(analysis)
    }

    func move(_ delta: Int) {
        guard !candidates.isEmpty else { return }
        selectedIndex = (selectedIndex + delta + candidates.count) % candidates.count
        // 命令阶段：切换高亮模板时，预览随之切换
        if !analysis.locked, candidates[selectedIndex].kind == .template {
            selectedTemplate = candidates[selectedIndex].key
            reanalyze(resetSelection: false)
        }
    }

    /// Tab：接受高亮的候选，没有候选时接受错误的修正建议。
    func complete() {
        if candidates.indices.contains(selectedIndex) {
            let c = candidates[selectedIndex]
            if c.kind == .template { selectedTemplate = c.key }
            apply(c.edit)
        } else if let fix = analysis.diagnostics.first(where: { $0.fix != nil })?.fix {
            apply(fix)
        }
    }

    func confirm(expand: Bool) {
        switch analysis.enter {
        case .insert:
            guard let text = engine.render(input: query, selected: selectedTemplate) else { return reject() }
            let collapses = analysis.preview?.collapsesInClaudeCode ?? false
            onSubmit?(text, expand && collapses)
        case .complete(let edit):
            if !analysis.locked { selectedTemplate = analysis.template?.name }
            apply(edit)
        case .blocked:
            reject()
        }
    }

    private func reject() {
        rejectCount += 1
        NSSound.beep()
    }

    private func apply(_ edit: Edit) {
        let ns = query as NSString
        let range = NSRange(location: Int(edit.start), length: Int(edit.end - edit.start))
        guard NSMaxRange(range) <= ns.length else { return }
        let new = ns.replacingCharacters(in: range, with: edit.text)
        field.setText(new, caret: range.location + (edit.text as NSString).length)
        let cmd = new.split(whereSeparator: \.isWhitespace).first.map(String.init) ?? ""
        commandWord = cmd
        query = new
        reanalyze(resetSelection: true)
    }
}
