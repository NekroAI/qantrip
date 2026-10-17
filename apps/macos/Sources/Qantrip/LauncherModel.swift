import AppKit
import QantripCore

/// 面板状态。解析、补全、校验、预览和首屏全部来自 Rust 核心；这里只管选择、编辑与动效状态。
/// 命令行文本是唯一的状态来源：键盘与鼠标操作都通过改写它生效（docs/decisions.md D16）。
@Observable
final class LauncherModel {
    private(set) var query = ""
    private(set) var analysis: Analysis
    /// 候选列表中的高亮项
    var selectedIndex = 0
    /// 回车被阻止时递增，用于触发抖动反馈
    private(set) var rejectCount = 0
    var target: NSRunningApplication?
    /// 面板入场状态（驱动候选错落出现）
    var entered = false
    /// 预览中新出现的高亮片段及其闪光强度（1 → 0）
    private(set) var flashRanges: [NSRange] = []
    private(set) var flashLevel: Double = 0
    /// 需要再按一次回车确认的指令（如 /rm）；输入改变后失效
    private(set) var armedCommand: String?

    @ObservationIgnored let engine: Engine
    @ObservationIgnored let field = CommandField()
    /// 提交回调：(最终文本, 是否展开粘贴)
    @ObservationIgnored var onSubmit: ((String, Bool) -> Void)?
    @ObservationIgnored var onCancel: (() -> Void)?
    /// 指令执行完成（docs/decisions.md D31）
    @ObservationIgnored var onCommand: ((CommandOutcome) -> Void)?
    @ObservationIgnored private var clipboardLoaded = false
    /// 界面高亮的模板名或首屏调用；命令词改变时清空
    @ObservationIgnored private var selectedKey: String?
    @ObservationIgnored private var commandWord = ""
    @ObservationIgnored private var flashTimer: Timer?

    static var reduceMotion: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    init(engine: Engine) {
        self.engine = engine
        analysis = engine.analyze(input: "", selected: nil)
        field.model = self
    }

    var candidates: [Candidate] { analysis.candidates }

    /// 候选列表的"类型"：首屏 / 模板 / 参数 / 取值。类型切换时整组替换，同类内部才做重排动画（D15）。
    var listKind: String {
        guard let c = candidates.first else { return "empty" }
        if query.trimmingCharacters(in: .whitespaces).isEmpty { return "home" }
        return analysis.locked ? "locked-\(c.kind)" : "command"
    }

    var firstError: Diagnostic? {
        analysis.diagnostics.first { $0.severity == .error && !$0.atCursor }
            ?? analysis.diagnostics.first { $0.severity == .error }
    }

    /// 面板打开：以当前前台应用取使用记录快照，回到首屏。
    func begin(target: NSRunningApplication?) {
        self.target = target
        engine.beginSession(app: target?.bundleIdentifier)
        selectedKey = nil
        commandWord = ""
        clipboardLoaded = false
        engine.setClipboard(text: nil)
        field.setText("")
        textDidChange("")
    }

    func textDidChange(_ text: String) {
        query = text
        armedCommand = nil
        // `/new 名称` 不写正文时用剪贴板文字：只在输入 /new 时读取剪贴板，且每次唤起只读一次
        if !clipboardLoaded, text.trimmingCharacters(in: .whitespaces).hasPrefix("/new ") {
            clipboardLoaded = true
            engine.setClipboard(text: NSPasteboard.general.string(forType: .string))
        }
        let cmd = text.split(whereSeparator: \.isWhitespace).first.map(String.init) ?? ""
        if cmd != commandWord {
            commandWord = cmd
            selectedKey = nil
        }
        reanalyze(resetSelection: true)
    }

    private func reanalyze(resetSelection: Bool) {
        let old = analysis
        analysis = engine.analyze(input: query, selected: selectedKey)
        if resetSelection { selectedIndex = 0 }
        selectedIndex = min(selectedIndex, max(candidates.count - 1, 0))
        field.applyHighlighting(analysis)
        flashNewHighlights(old: old)
    }

    // MARK: 键盘

    func move(_ delta: Int) {
        guard !candidates.isEmpty else { return }
        select(index: (selectedIndex + delta + candidates.count) % candidates.count)
    }

    private func select(index: Int) {
        selectedIndex = index
        // 命令阶段 / 首屏：切换高亮项时预览随之切换
        let c = candidates[index]
        if !analysis.locked, c.kind == .template || c.kind == .invocation {
            selectedKey = c.key
            reanalyze(resetSelection: false)
        }
    }

    /// Tab：接受高亮的候选，没有候选时接受错误的修正建议。
    func complete() {
        if candidates.indices.contains(selectedIndex) {
            let c = candidates[selectedIndex]
            if c.kind == .template { selectedKey = c.key }
            apply(c.edit)
        } else if let fix = analysis.diagnostics.first(where: { $0.fix != nil })?.fix {
            apply(fix)
        }
    }

    func confirm(expand: Bool) {
        switch analysis.enter {
        case .insert:
            guard let text = engine.render(input: query, selected: selectedKey) else { return reject() }
            let collapses = analysis.preview?.collapsesInClaudeCode ?? false
            if AppSettings.shared.recordUsage {
                engine.recordUse(input: query, selected: selectedKey, app: target?.bundleIdentifier)
            }
            onSubmit?(text, expand && collapses)
        case .complete(let edit):
            if !analysis.locked { selectedKey = analysis.template?.name }
            apply(edit)
        case .blocked:
            reject()
        case .run(let confirm):
            if confirm && armedCommand != query {
                armedCommand = query
                return
            }
            armedCommand = nil
            do {
                onCommand?(try engine.runCommand(input: query))
            } catch {
                reject()
            }
        }
    }

    /// 是否处于指令模式（输入以 / 开头）
    var isCommandMode: Bool {
        let t = query.trimmingCharacters(in: .whitespaces)
        return t.hasPrefix("/") || t.hasPrefix("／")
    }

    /// 回车被阻止：只做视觉反馈（抖动 + 图标），不发提示音。
    private func reject() {
        rejectCount += 1
    }

    // MARK: 鼠标

    func click(_ index: Int) {
        guard candidates.indices.contains(index) else { return }
        select(index: index)
        complete()
        refocus()
    }

    func doubleClick(_ index: Int) {
        guard candidates.indices.contains(index) else { return }
        let c = candidates[index]
        if c.kind == .template || c.kind == .invocation, !analysis.locked {
            select(index: index)
            confirm(expand: false)
        } else {
            click(index)
        }
    }

    func editParam(_ name: String, _ edit: ParamEdit) {
        guard let r = engine.editParam(input: query, selected: selectedKey, name: name, edit: edit) else { return reject() }
        if query.trimmingCharacters(in: .whitespaces).isEmpty { selectedKey = analysis.template?.name }
        setInput(r.text, caret: Int(r.caret))
        refocus()
    }

    private func refocus() {
        field.window?.makeFirstResponder(field)
        field.currentEditor()?.selectedRange = NSRange(location: field.caret, length: 0)
    }

    // MARK: 编辑

    private func apply(_ edit: Edit) {
        let ns = query as NSString
        let range = NSRange(location: Int(edit.start), length: Int(edit.end - edit.start))
        guard NSMaxRange(range) <= ns.length else { return }
        let new = ns.replacingCharacters(in: range, with: edit.text)
        setInput(new, caret: range.location + (edit.text as NSString).length)
    }

    private func setInput(_ text: String, caret: Int) {
        field.setText(text, caret: caret)
        commandWord = text.split(whereSeparator: \.isWhitespace).first.map(String.init) ?? ""
        query = text
        reanalyze(resetSelection: true)
    }

    // MARK: 预览闪光（D15）

    /// 同一模板下新出现的高亮片段先亮起，再淡成常驻高亮。
    private func flashNewHighlights(old: Analysis) {
        guard !Self.reduceMotion, let new = analysis.preview,
              old.template?.name == analysis.template?.name else { return }
        let before = Set((old.preview?.highlights ?? []).map { "\($0.kind)-\($0.start)-\($0.end)" })
        let fresh = new.highlights.filter { !before.contains("\($0.kind)-\($0.start)-\($0.end)") }
        guard !fresh.isEmpty else { return }
        flashRanges = fresh.map { NSRange(location: Int($0.start), length: Int($0.end - $0.start)) }
        flashLevel = 1
        flashTimer?.invalidate()
        let start = Date()
        flashTimer = Timer.scheduledTimer(withTimeInterval: 1 / 60, repeats: true) { [weak self] t in
            let p = Date().timeIntervalSince(start) / 0.45
            self?.flashLevel = max(0, 1 - p * p)
            if p >= 1 { t.invalidate(); self?.flashRanges = [] }
        }
    }
}
