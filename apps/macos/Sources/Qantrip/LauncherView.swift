import AppKit
import QantripCore
import SwiftUI

struct LauncherView: View {
    @Bindable var model: LauncherModel

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
                // 顶部一抹品牌色辉光
                LinearGradient(colors: [Color.qAccent.opacity(0.16), .clear], startPoint: .top, endPoint: .bottom)
                    .frame(height: 120)
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
                .symbolEffect(.bounce, value: model.rejectCount)
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

    // MARK: 左栏：候选

    private var candidateList: some View {
        Group {
            if model.candidates.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: model.analysis.phase == .extra ? "text.cursor" : "sparkles")
                        .font(.system(size: 22)).foregroundStyle(.tertiary)
                    Text(model.analysis.phase == .extra ? "正在输入附加要求" : "没有可补全的内容")
                        .font(.system(size: 12)).foregroundStyle(.tertiary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 2) {
                            ForEach(Array(model.candidates.enumerated()), id: \.offset) { i, c in
                                CandidateRow(candidate: c, selected: i == model.selectedIndex).id(i)
                            }
                        }
                        .padding(8)
                    }
                    .onChange(of: model.selectedIndex) { _, new in proxy.scrollTo(new) }
                }
            }
        }
    }

    // MARK: 右栏：咒文卡（模板 + 参数）与实时预览

    private var spellCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let t = model.analysis.template {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(t.name).font(.system(size: 17, weight: .semibold, design: .monospaced))
                        .foregroundStyle(Color.qAccent)
                    Text(t.description).font(.system(size: 13)).foregroundStyle(.secondary)
                    Spacer()
                    if !model.analysis.hint.isEmpty {
                        Text(model.analysis.hint).font(.system(size: 11)).foregroundStyle(.secondary)
                    }
                }
                if !model.analysis.params.isEmpty {
                    FlowLayout(spacing: 6) {
                        ForEach(model.analysis.params, id: \.name) { ParamChip(param: $0) }
                    }
                }
                if let p = model.analysis.preview {
                    previewBox(p)
                }
            } else {
                Spacer()
                Text("没有匹配的模板").foregroundStyle(.tertiary).frame(maxWidth: .infinity)
                Spacer()
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private func previewBox(_ p: QantripCore.Preview) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text("预览").font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary)
                Text("\(p.chars) 字 · \(p.lines) 行").font(.system(size: 11)).foregroundStyle(.tertiary)
                Spacer()
                if p.collapsesInClaudeCode && Inserter.expandPasteEnabled {
                    Label("Claude Code 中会折叠 · ⌘↩ 展开粘贴", systemImage: "rectangle.expand.vertical")
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
        let ns = NSMutableAttributedString(string: p.text)
        for h in p.highlights {
            let r = NSRange(location: Int(h.start), length: Int(h.end - h.start))
            guard NSMaxRange(r) <= ns.length else { continue }
            let color = h.kind == .extra ? Theme.value : Theme.accent
            ns.addAttribute(.backgroundColor, value: color.withAlphaComponent(0.20), range: r)
        }
        return (try? AttributedString(ns, including: \.appKit)) ?? AttributedString(p.text)
    }

    // MARK: 底栏：状态 + 按键提示

    private var footer: some View {
        HStack(spacing: 14) {
            status
            Spacer()
            Keycap("↩", "插入")
            if Inserter.expandPasteEnabled { Keycap("⌘↩", "展开粘贴") }
            Keycap("⇥", "补全")
            Keycap("↑↓", "选择")
            Keycap("esc", "关闭")
        }
        .padding(.horizontal, 16).padding(.vertical, 9)
    }

    @ViewBuilder private var status: some View {
        if let e = model.firstError {
            Label(e.message, systemImage: "exclamationmark.circle.fill")
                .foregroundStyle(e.atCursor ? AnyShapeStyle(.secondary) : AnyShapeStyle(Color.red))
                .lineLimit(1)
        } else if let w = model.analysis.diagnostics.first(where: { $0.severity == .warning }) {
            Label(w.message, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).lineLimit(1)
        } else if let c = model.analysis.canonical, model.analysis.locked {
            Text(c).font(.system(size: 11, design: .monospaced)).foregroundStyle(.tertiary).lineLimit(1)
                .truncationMode(.middle)
        }
    }
}

// MARK: - 组件

private struct CandidateRow: View {
    let candidate: Candidate
    let selected: Bool

    var body: some View {
        HStack(spacing: 8) {
            Text(candidate.title)
                .font(.system(size: 14, weight: candidate.kind == .template ? .semibold : .regular, design: .monospaced))
                .foregroundStyle(titleColor)
            if !candidate.subtitle.isEmpty {
                Text(candidate.subtitle).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 4)
            if candidate.active {
                Image(systemName: "checkmark").font(.system(size: 11, weight: .bold)).foregroundStyle(Color.qAccent)
            }
            if candidate.kind == .template {
                Text(candidate.detail).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .help(candidate.detail)
        .padding(.horizontal, 10).padding(.vertical, 7)
        .background(selected ? Color.qAccent.opacity(0.18) : .clear, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }

    private var titleColor: Color {
        switch candidate.kind {
        case .template: return .primary
        case .param: return .qOption
        case .choice: return .qValue
        }
    }
}

private struct ParamChip: View {
    let param: ParamState

    var body: some View {
        HStack(spacing: 4) {
            Text(param.name).font(.system(size: 11, design: .monospaced))
                .foregroundStyle(param.isSet ? Color.qOption : .secondary)
            Text(param.display).font(.system(size: 11, weight: param.isSet ? .semibold : .regular))
                .foregroundStyle(param.isSet ? .primary : .tertiary)
        }
        .padding(.horizontal, 8).padding(.vertical, 4)
        .background(param.isSet ? Color.qAccent.opacity(0.14) : .clear, in: Capsule())
        .overlay(Capsule().strokeBorder(param.isSet ? Color.qAccent.opacity(0.35) : Color.secondary.opacity(0.25)))
        .help(param.description)
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
