import AppKit
import QantripCore
import SwiftUI

private let snappy: Animation = .snappy(duration: 0.2)

struct LauncherView: View {
    @Bindable var model: LauncherModel
    @Namespace private var ns

    private var motion: Bool { !LauncherModel.reduceMotion }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider().opacity(0.5)
            HStack(spacing: 0) {
                candidateList.frame(width: 300)
                Divider().opacity(0.5)
                spellCard
            }
            Divider().opacity(0.5)
            footer
        }
        .frame(width: Theme.panelSize.width, height: Theme.panelSize.height)
        .background {
            ZStack(alignment: .top) {
                Rectangle().fill(.regularMaterial)
                // 顶部一抹品牌色辉光；命令锁定时增强
                LinearGradient(colors: [Color.qAccent.opacity(model.analysis.locked ? 0.24 : 0.14), .clear],
                               startPoint: .top, endPoint: .bottom)
                    .frame(height: 140)
                    .animation(motion ? .easeOut(duration: 0.3) : nil, value: model.analysis.locked)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(.white.opacity(0.10)))
    }

    // MARK: 顶栏：命令行 + 插入目标

    private var header: some View {
        HStack(spacing: 12) {
            Image(systemName: "wand.and.stars")
                .font(.system(size: 20, weight: .medium))
                .foregroundStyle(Color.qAccent)
                .symbolEffect(.bounce, value: model.analysis.locked)
                .symbolEffect(.bounce.down, value: model.rejectCount)
            CommandFieldView(field: model.field).frame(height: 30)
            if let app = model.target {
                HStack(spacing: 6) {
                    Text("→").foregroundStyle(.tertiary)
                    if let icon = app.icon { Image(nsImage: icon).resizable().frame(width: 18, height: 18) }
                    Text(app.localizedName ?? "").lineLimit(1)
                }
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 10).padding(.vertical, 5)
                .background(.quaternary.opacity(0.5), in: Capsule())
            }
        }
        .padding(.horizontal, 18).padding(.vertical, 14)
    }

    // MARK: 左栏：候选（分段、可点击、带入场与重排动画）

    private enum Row: Identifiable {
        case header(String)
        case item(Int, Candidate)
        var id: String {
            switch self {
            case .header(let s): return "h:\(s)"
            case .item(_, let c): return "\(c.section)|\(c.kind)|\(c.key)"
            }
        }
    }

    private var rows: [Row] {
        var out: [Row] = []
        var section: String?
        for (i, c) in model.candidates.enumerated() {
            if !c.section.isEmpty, c.section != section {
                out.append(.header(c.section))
                section = c.section
            }
            out.append(.item(i, c))
        }
        return out
    }

    private var candidateList: some View {
        Group {
            if model.candidates.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: model.analysis.phase == .extra ? "text.cursor" : "sparkles")
                        .font(.system(size: 22)).foregroundStyle(.tertiary)
                        .symbolEffect(.pulse, isActive: motion && model.analysis.phase == .extra)
                    Text(model.analysis.phase == .extra ? L("正在输入附加要求", "Entering extra instructions") : L("没有可补全的内容", "Nothing to complete"))
                        .font(.system(size: 12)).foregroundStyle(.tertiary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .transition(.opacity)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(Array(rows.enumerated()), id: \.element.id) { n, row in
                                Group {
                                    switch row {
                                    case .header(let title):
                                        Text(title).font(.system(size: 11, weight: .semibold)).foregroundStyle(.tertiary)
                                            .padding(.horizontal, 10).padding(.top, n == 0 ? 2 : 10).padding(.bottom, 2)
                                    case .item(let i, let c):
                                        CandidateRow(candidate: c, selected: i == model.selectedIndex, ns: ns)
                                            .contentShape(Rectangle())
                                            .onTapGesture(count: 2) { model.doubleClick(i) }
                                            .onTapGesture { model.click(i) }
                                    }
                                }
                                .opacity(model.entered ? 1 : 0)
                                .offset(y: model.entered ? 0 : 8)
                                .animation(motion ? .spring(duration: 0.28, bounce: 0.2).delay(Double(min(n, 12)) * 0.016) : nil,
                                           value: model.entered)
                            }
                        }
                        .padding(8)
                        .animation(motion ? snappy : nil, value: rows.map(\.id))
                        .modifier(FadeIn(enabled: motion))
                        .id(model.listKind)
                    }
                    .onChange(of: model.selectedIndex) { _, new in
                        guard let id = rows.first(where: { if case .item(new, _) = $0 { return true } else { return false } })?.id else { return }
                        withAnimation(motion ? snappy : nil) { proxy.scrollTo(id) }
                    }
                }
            }
        }
    }

    // MARK: 右栏：咒文卡（模板 + 参数）与实时预览

    private var spellCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let t = model.analysis.template {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(t.name).font(.qDisplay(22))
                        .foregroundStyle(Color.qAccent)
                    Text(t.description).font(.system(size: 13)).foregroundStyle(.secondary)
                    Spacer()
                    if !model.analysis.hint.isEmpty {
                        Text(model.analysis.hint).font(.system(size: 11)).foregroundStyle(.secondary)
                            .transition(.opacity)
                    }
                }
                .id(t.name)
                .transition(motion ? .asymmetric(insertion: .move(edge: .leading).combined(with: .opacity), removal: .opacity) : .identity)
                if !model.analysis.params.isEmpty {
                    FlowLayout(spacing: 6) {
                        ForEach(model.analysis.params, id: \.name) { p in
                            ParamChip(param: p, motion: motion) { model.editParam(p.name, $0) }
                        }
                    }
                }
                if let p = model.analysis.preview {
                    previewBox(p)
                }
            } else {
                Spacer()
                Text(L("没有匹配的模板", "No matching templates")).foregroundStyle(.tertiary).frame(maxWidth: .infinity)
                Spacer()
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .animation(motion ? snappy : nil, value: model.analysis.template?.name)
    }

    private func previewBox(_ p: QantripCore.Preview) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(L("预览", "Preview")).font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary)
                Text(L("\(p.chars) 字 · \(p.lines) 行", "\(p.chars) chars · \(p.lines) lines")).font(.system(size: 11)).foregroundStyle(.tertiary)
                    .contentTransition(.numericText())
                    .animation(motion ? snappy : nil, value: p.chars)
                Spacer()
                if p.collapsesInClaudeCode && Inserter.expandPasteEnabled {
                    Label(L("Claude Code 中会折叠 · ⌘↩ 展开粘贴", "Collapsed in Claude Code · ⌘↩ to expand"), systemImage: "rectangle.expand.vertical")
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                }
            }
            ScrollView {
                Text(highlighted(p))
                    .font(.system(size: 13))
                    .lineSpacing(3)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
            }
            .background(.background.opacity(0.35), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
    }

    private func highlighted(_ p: QantripCore.Preview) -> AttributedString {
        let s = NSMutableAttributedString(string: p.text)
        for h in p.highlights {
            let r = NSRange(location: Int(h.start), length: Int(h.end - h.start))
            guard NSMaxRange(r) <= s.length else { continue }
            let color = h.kind == .extra ? Theme.value : Theme.accent
            s.addAttribute(.backgroundColor, value: color.withAlphaComponent(0.26), range: r)
        }
        // 新出现的片段闪光：从高亮度淡回常驻高亮
        if model.flashLevel > 0 {
            for r in model.flashRanges where NSMaxRange(r) <= s.length {
                s.addAttribute(.backgroundColor, value: Theme.accent.withAlphaComponent(0.26 + 0.5 * model.flashLevel), range: r)
            }
        }
        return (try? AttributedString(s, including: \.appKit)) ?? AttributedString(p.text)
    }

    // MARK: 底栏：状态 + 可点击的操作

    private var footer: some View {
        HStack(spacing: 14) {
            status
            Spacer()
            KeyButton("↩", model.isCommandMode ? L("执行", "Run") : L("插入", "Insert")) { model.confirm(expand: false) }
            if Inserter.expandPasteEnabled && !model.isCommandMode { KeyButton("⌘↩", L("展开粘贴", "Expand paste")) { model.confirm(expand: true) } }
            Keycap("⇥", L("补全", "Complete"))
            Keycap("↑↓", L("选择", "Select"))
            KeyButton("esc", L("关闭", "Close")) { model.onCancel?() }
        }
        .padding(.horizontal, 16).padding(.vertical, 9)
    }

    @ViewBuilder private var status: some View {
        if model.armedCommand != nil {
            Label(L("再按一次 ↩ 确认", "Press ↩ again to confirm"), systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange).lineLimit(1)
        } else if let e = model.firstError {
            Label(e.message, systemImage: "exclamationmark.circle.fill")
                .foregroundStyle(e.atCursor ? AnyShapeStyle(.secondary) : AnyShapeStyle(Color.red))
                .lineLimit(1)
        } else if let w = model.analysis.diagnostics.first(where: { $0.severity == .warning }) {
            Label(w.message, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).lineLimit(1)
        } else if let c = model.analysis.canonical, model.analysis.locked {
            Text(c).font(.qMono(11)).foregroundStyle(.tertiary).lineLimit(1)
                .truncationMode(.middle)
        }
    }
}

// MARK: - 组件

/// 简单着色：命令词、--参数、=取值（用于首屏的调用记录）
private func colorizedInvocation(_ s: String) -> AttributedString {
    var out = AttributedString()
    for (i, word) in s.split(separator: " ", omittingEmptySubsequences: false).enumerated() {
        if i > 0 { out += AttributedString(" ") }
        var piece: AttributedString
        if i == 0 {
            piece = AttributedString(word)
            piece.foregroundColor = Color.qAccent
        } else if word.hasPrefix("-"), let eq = word.firstIndex(of: "=") {
            var a = AttributedString(word[..<word.index(after: eq)])
            a.foregroundColor = Color.qOption
            var b = AttributedString(word[word.index(after: eq)...])
            b.foregroundColor = Color.qValue
            piece = a + b
        } else if word.hasPrefix("-") {
            piece = AttributedString(word)
            piece.foregroundColor = Color.qOption
        } else {
            piece = AttributedString(word)
        }
        out += piece
    }
    return out
}

private struct CandidateRow: View {
    let candidate: Candidate
    let selected: Bool
    let ns: Namespace.ID
    @State private var hover = false

    var body: some View {
        HStack(spacing: 8) {
            if candidate.kind == .invocation {
                Image(systemName: (candidate.section == "试试看" || candidate.section == "Try these") ? "lightbulb" : "clock.arrow.circlepath")
                    .font(.system(size: 11)).foregroundStyle(.tertiary)
                Text(colorizedInvocation(candidate.title))
                    .font(.qMono(13)).lineLimit(1).truncationMode(.tail)
            } else {
                Text(candidate.title)
                    .font(.qMono(14, weight: candidate.kind == .template ? 600 : 400))
                    .foregroundStyle(titleColor)
                if !candidate.subtitle.isEmpty {
                    Text(candidate.subtitle).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 4)
            if candidate.active {
                Image(systemName: "checkmark").font(.system(size: 11, weight: .bold)).foregroundStyle(Color.qAccent)
                    .transition(.scale.combined(with: .opacity))
            }
            if candidate.kind == .template {
                Text(candidate.detail).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .help(candidate.detail)
        .padding(.horizontal, 10).padding(.vertical, 7)
        .background {
            if selected {
                RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.qAccent.opacity(0.20))
                    .matchedGeometryEffect(id: "selection", in: ns)
            } else if hover {
                RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.primary.opacity(0.05))
            }
        }
        .onHover { hover = $0 }
    }

    private var titleColor: Color {
        switch candidate.kind {
        case .template, .invocation: return .primary
        case .param: return .qOption
        case .choice: return .qValue
        }
    }
}

/// 参数标签：显示当前取值，点击可修改（D16）；取值变化时弹跳（D15）。
private struct ParamChip: View {
    let param: ParamState
    let motion: Bool
    let edit: (ParamEdit) -> Void
    @State private var hover = false

    var body: some View {
        Group {
            switch param.kind {
            case .bool:
                Button { edit(.toggle) } label: { label }.buttonStyle(.plain)
            case .enum, .multi:
                Menu {
                    ForEach(param.choices, id: \.value) { c in
                        Button {
                            edit(param.kind == .multi ? .toggleChoice(value: c.value) : .set(value: c.value))
                        } label: {
                            if c.on { Label("\(c.label)  \(c.value)", systemImage: "checkmark") } else { Text("\(c.label)  \(c.value)") }
                        }
                    }
                    if param.isSet {
                        Divider()
                        Button(L("恢复默认", "Restore default")) { edit(.clear) }
                    }
                } label: { label }
                    .menuStyle(.button).buttonStyle(.plain).menuIndicator(.hidden).fixedSize()
            default:
                Button { edit(.begin) } label: { label }.buttonStyle(.plain)
            }
        }
        .help(param.description)
        .onHover { hover = $0 }
    }

    private var label: some View {
        HStack(spacing: 4) {
            Text(param.name).font(.qMono(11))
                .foregroundStyle(param.isSet ? Color.qOption : .secondary)
            Text(param.display).font(.system(size: 11, weight: param.isSet ? .semibold : .regular))
                .foregroundStyle(param.isSet ? .primary : .tertiary)
                .contentTransition(.interpolate)
        }
        .padding(.horizontal, 8).padding(.vertical, 4)
        .background(param.isSet ? Color.qAccent.opacity(0.16) : (hover ? Color.primary.opacity(0.06) : .clear), in: Capsule())
        .overlay(Capsule().strokeBorder(param.isSet ? Color.qAccent.opacity(0.4) : Color.secondary.opacity(0.25)))
        .keyframeAnimator(initialValue: 1.0, trigger: param.display) { content, scale in
            content.scaleEffect(scale)
        } keyframes: { _ in
            SpringKeyframe(motion ? 1.12 : 1.0, duration: 0.09)
            SpringKeyframe(1.0, duration: 0.22, spring: .bouncy)
        }
        .animation(motion ? snappy : nil, value: param.isSet)
        .contentShape(Capsule())
    }
}

/// 列表类型切换时：旧列表直接移除，新列表在出现时淡入，避免新旧两组重叠（D15）。
private struct FadeIn: ViewModifier {
    let enabled: Bool
    @State private var shown = false

    func body(content: Content) -> some View {
        content
            .opacity(shown || !enabled ? 1 : 0)
            .offset(x: shown || !enabled ? 0 : -6)
            .onAppear { withAnimation(.easeOut(duration: 0.14)) { shown = true } }
    }
}

private struct Keycap: View {
    let key: String
    let label: String
    init(_ key: String, _ label: String) { self.key = key; self.label = label }

    var body: some View {
        HStack(spacing: 4) {
            Text(key).font(.system(size: 10, weight: .medium, design: .rounded))
                .padding(.horizontal, 5).padding(.vertical, 1)
                .background(.quaternary, in: RoundedRectangle(cornerRadius: 4))
            Text(label).font(.system(size: 11))
        }
        .foregroundStyle(.secondary)
    }
}

private struct KeyButton: View {
    let key: String
    let label: String
    let action: () -> Void
    @State private var hover = false
    init(_ key: String, _ label: String, action: @escaping () -> Void) { self.key = key; self.label = label; self.action = action }

    var body: some View {
        Button(action: action) {
            Keycap(key, label)
                .padding(.horizontal, 4).padding(.vertical, 2)
                .background(hover ? Color.primary.opacity(0.07) : .clear, in: RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
    }
}

/// 自动换行的横向排列。
struct FlowLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(proposal.width ?? .infinity, subviews)
        let h = rows.map(\.height).reduce(0, +) + spacing * CGFloat(max(rows.count - 1, 0))
        return CGSize(width: proposal.width ?? rows.map(\.width).max() ?? 0, height: h)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in arrange(bounds.width, subviews) {
            var x = bounds.minX
            for i in row.items {
                let s = subviews[i].sizeThatFits(.unspecified)
                subviews[i].place(at: CGPoint(x: x, y: y), proposal: .unspecified)
                x += s.width + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Row { var items: [Int] = []; var width: CGFloat = 0; var height: CGFloat = 0 }

    private func arrange(_ maxWidth: CGFloat, _ subviews: Subviews) -> [Row] {
        var rows = [Row()]
        for (i, v) in subviews.enumerated() {
            let s = v.sizeThatFits(.unspecified)
            if !rows[rows.count - 1].items.isEmpty, rows[rows.count - 1].width + spacing + s.width > maxWidth {
                rows.append(Row())
            }
            var r = rows[rows.count - 1]
            r.width += (r.items.isEmpty ? 0 : spacing) + s.width
            r.height = max(r.height, s.height)
            r.items.append(i)
            rows[rows.count - 1] = r
        }
        return rows
    }
}
