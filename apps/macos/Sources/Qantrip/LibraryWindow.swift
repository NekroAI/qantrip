import AppKit
import QantripCore
import SwiftUI
import UniformTypeIdentifiers

/// 模板库与编辑器窗口（PRD 4.5 / 11.2，docs/decisions.md D19）。
enum LibraryWindow {
    static let model = LibraryModel()

    static func show(select name: String? = nil) {
        model.reload()
        if let name { model.select(name) }
        WindowManager.shared.show("library", title: "模板库", size: NSSize(width: 1180, height: 760), resizable: true) {
            LibraryView(model: model)
        }
    }
}

// MARK: - 模型

@Observable
final class LibraryModel {
    private(set) var entries: [TemplateEntry] = []
    var search = ""
    private(set) var selection: String?
    private(set) var editor: EditorModel?

    private var engine: Engine { AppServices.shared.engine }
    private var dir: URL { AppServices.shared.templatesDir }

    func reload() {
        engine.reload()
        entries = engine.entries()
        // 当前编辑的模板未修改时，跟随磁盘内容刷新（外部编辑器修改）
        if let e = editor, !e.dirty, let fresh = entries.first(where: { $0.name == e.entry.name }) {
            editor = EditorModel(entry: fresh, library: self)
        }
    }

    var filtered: [TemplateEntry] {
        let q = search.lowercased()
        guard !q.isEmpty else { return entries }
        return entries.filter {
            $0.name.lowercased().contains(q) || $0.description.lowercased().contains(q)
                || $0.aliases.contains { $0.lowercased().contains(q) }
        }
    }

    func select(_ name: String?) {
        guard name != selection else { return }
        if let e = editor, e.dirty, !confirmDiscard(e.doc?.name ?? e.entry.name) { return }
        selection = name
        editor = name.flatMap { n in entries.first { $0.name == n } }.map { EditorModel(entry: $0, library: self) }
    }

    private func confirmDiscard(_ name: String) -> Bool {
        let a = NSAlert()
        a.messageText = "放弃对「\(name)」的修改？"
        a.addButton(withTitle: "放弃修改")
        a.addButton(withTitle: "取消")
        return a.runModal() == .alertFirstButtonReturn
    }

    // MARK: 文件操作

    func fileURL(for name: String) -> URL { dir.appendingPathComponent("\(name).md") }

    func userEntry(_ name: String) -> TemplateEntry? { entries.first { $0.name == name && !$0.builtin } }

    func uniqueName(_ base: String) -> String {
        var n = base, i = 2
        while entries.contains(where: { $0.name == n }) || FileManager.default.fileExists(atPath: fileURL(for: n).path) {
            n = "\(base)-\(i)"; i += 1
        }
        return n
    }

    @discardableResult
    func write(source: String, name: String, replacing oldPath: String?) throws -> URL {
        let url = fileURL(for: name)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try source.write(to: url, atomically: true, encoding: .utf8)
        if let old = oldPath, old != url.path, FileManager.default.fileExists(atPath: old) {
            try? FileManager.default.trashItem(at: URL(fileURLWithPath: old), resultingItemURL: nil)
        }
        return url
    }

    func newTemplate() {
        let name = uniqueName("my-template")
        let source = serializeTemplateDoc(doc: newTemplateDoc(name: name))
        try? write(source: source, name: name, replacing: nil)
        reload()
        select(name)
    }

    func duplicate(_ e: TemplateEntry) {
        guard var doc = try? parseTemplateDoc(source: e.source) else { return }
        doc.name = uniqueName(e.partial ? e.name : "\(e.name)-copy")
        doc.aliases = []
        try? write(source: serializeTemplateDoc(doc: doc), name: doc.name, replacing: nil)
        reload()
        select(doc.name)
    }

    /// 自定义内置模板：原样复制源码到用户目录，覆盖内置版本。
    func customize(_ e: TemplateEntry) {
        try? write(source: e.source, name: e.name, replacing: nil)
        reload()
        editor = entries.first { $0.name == e.name }.map { EditorModel(entry: $0, library: self) }
    }

    /// 删除用户模板（移到废纸篓）；覆盖内置的模板删除后恢复内置版本。
    func delete(_ e: TemplateEntry) {
        guard let path = e.path else { return }
        let a = NSAlert()
        a.messageText = e.overridesBuiltin ? "恢复「\(e.name)」为内置版本？" : "删除模板「\(e.name)」？"
        a.informativeText = "文件会被移到废纸篓。"
        a.addButton(withTitle: e.overridesBuiltin ? "恢复内置" : "删除")
        a.addButton(withTitle: "取消")
        guard a.runModal() == .alertFirstButtonReturn else { return }
        try? FileManager.default.trashItem(at: URL(fileURLWithPath: path), resultingItemURL: nil)
        editor?.dirty = false
        reload()
        if e.overridesBuiltin {
            editor = entries.first { $0.name == e.name }.map { EditorModel(entry: $0, library: self) }
        } else {
            selection = nil
            editor = nil
        }
    }

    // MARK: 导入导出

    func exportOne(_ e: TemplateEntry) {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(e.name).md"
        panel.allowedContentTypes = [UTType(filenameExtension: "md") ?? .plainText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        try? e.source.write(to: url, atomically: true, encoding: .utf8)
    }

    func exportAll() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "qantrip-templates.zip"
        panel.allowedContentTypes = [.zip]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        try? FileManager.default.removeItem(at: url)
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        p.arguments = ["-c", "-k", "--keepParent", dir.path, url.path]
        try? p.run()
        p.waitUntilExit()
    }

    func importFiles() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = true
        panel.allowedContentTypes = [UTType(filenameExtension: "md") ?? .plainText, .zip, .folder]
        panel.message = "选择模板文件（.md）、文件夹或 zip"
        guard panel.runModal() == .OK else { return }
        importURLs(panel.urls)
    }

    func importURLs(_ urls: [URL]) {
        var files: [URL] = []
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("qantrip-import-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: tmp) }
        for url in urls {
            if url.pathExtension.lowercased() == "zip" {
                let p = Process()
                p.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
                p.arguments = ["-x", "-k", url.path, tmp.path]
                try? p.run()
                p.waitUntilExit()
                files += mdFiles(in: tmp)
            } else if (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true {
                files += mdFiles(in: url)
            } else {
                files.append(url)
            }
        }

        var imported = 0, skipped = 0
        var failures: [String] = []
        var sticky: NSApplication.ModalResponse?
        for f in files {
            guard let source = try? String(contentsOf: f, encoding: .utf8) else { failures.append(f.lastPathComponent); continue }
            guard var doc = try? parseTemplateDoc(source: source) else { failures.append(f.lastPathComponent); continue }
            var text = source
            if entries.contains(where: { $0.name == doc.name && !$0.builtin }) {
                let choice = sticky ?? askConflict(doc.name, &sticky)
                switch choice {
                case .alertFirstButtonReturn: break // 覆盖
                case .alertSecondButtonReturn:
                    doc.name = uniqueName(doc.name)
                    text = serializeTemplateDoc(doc: doc)
                default:
                    skipped += 1
                    continue
                }
            }
            if (try? write(source: text, name: doc.name, replacing: nil)) != nil {
                imported += 1
                reload()
            } else {
                failures.append(f.lastPathComponent)
            }
        }
        let a = NSAlert()
        a.messageText = "导入完成：\(imported) 个"
        var info: [String] = []
        if skipped > 0 { info.append("跳过 \(skipped) 个") }
        if !failures.isEmpty { info.append("无法导入：\(failures.joined(separator: "、"))") }
        a.informativeText = info.joined(separator: "\n")
        a.runModal()
    }

    private func mdFiles(in dir: URL) -> [URL] {
        (FileManager.default.enumerator(at: dir, includingPropertiesForKeys: nil)?.allObjects as? [URL] ?? [])
            .filter { $0.pathExtension.lowercased() == "md" && !$0.path.contains("__MACOSX") }
    }

    private func askConflict(_ name: String, _ sticky: inout NSApplication.ModalResponse?) -> NSApplication.ModalResponse {
        let a = NSAlert()
        a.messageText = "已存在名为「\(name)」的模板"
        a.addButton(withTitle: "覆盖")
        a.addButton(withTitle: "重命名导入")
        a.addButton(withTitle: "跳过")
        a.showsSuppressionButton = true
        a.suppressionButton?.title = "对其余冲突执行相同操作"
        let r = a.runModal()
        if a.suppressionButton?.state == .on { sticky = r }
        return r
    }
}

/// 单个模板的编辑状态。表单与源码互相同步：表单修改 → 重新生成源码；源码修改 → 能解析时更新表单。
@Observable
final class EditorModel {
    let entry: TemplateEntry
    private(set) var source: String
    private(set) var doc: TemplateDoc?
    private(set) var check: SourceCheck
    var tryInput = "" { didSet { recheck() } }
    var dirty = false
    var mode: Mode = .form
    var selectedParam: Int?
    private(set) var saveError: String?
    @ObservationIgnored private weak var library: LibraryModel?

    enum Mode: String { case form, source }

    var readOnly: Bool { entry.builtin }

    init(entry: TemplateEntry, library: LibraryModel) {
        self.entry = entry
        self.library = library
        source = entry.source
        doc = try? parseTemplateDoc(source: entry.source)
        check = AppServices.shared.engine.checkSource(source: entry.source, tryInput: "")
    }

    /// 源码模式下的编辑
    func setSource(_ s: String) {
        guard s != source else { return }
        source = s
        dirty = true
        if let d = try? parseTemplateDoc(source: s) { doc = d }
        recheck()
    }

    /// 表单模式下的编辑
    func updateDoc(_ change: (inout TemplateDoc) -> Void) {
        guard var d = doc else { return }
        change(&d)
        doc = d
        source = serializeTemplateDoc(doc: d)
        dirty = true
        recheck()
    }

    private func recheck() {
        check = AppServices.shared.engine.checkSource(source: source, tryInput: tryInput)
    }

    var canSave: Bool { dirty && !readOnly && check.problems.isEmpty && check.name != nil }

    func save() {
        guard canSave, let name = check.name, let library else { return }
        // 改名时不得与其他用户模板重名
        if name != entry.name, library.userEntry(name) != nil {
            saveError = "已存在名为「\(name)」的模板"
            return
        }
        do {
            try library.write(source: source, name: name, replacing: entry.path)
            saveError = nil
            dirty = false
            library.reload()
            library.select(nil)
            library.select(name)
        } catch {
            saveError = error.localizedDescription
        }
    }
}

// MARK: - 视图

struct LibraryView: View {
    @Bindable var model: LibraryModel

    var body: some View {
        NavigationSplitView {
            sidebar.navigationSplitViewColumnWidth(min: 220, ideal: 260)
        } detail: {
            if let editor = model.editor {
                EditorView(editor: editor, library: model).id(editor.entry.name + (editor.entry.path ?? ""))
            } else {
                ContentUnavailableView("选择或新建一个模板", systemImage: "wand.and.stars",
                                       description: Text("模板保存为 Markdown 文件，也可以用任意编辑器直接修改。"))
            }
        }
        .toolbar {
            ToolbarItemGroup {
                Button { model.newTemplate() } label: { Label("新建", systemImage: "plus") }.help("新建模板")
                Button { model.importFiles() } label: { Label("导入", systemImage: "square.and.arrow.down") }.help("导入模板")
                Button { model.exportAll() } label: { Label("导出全部", systemImage: "square.and.arrow.up") }.help("把我的模板导出为 zip")
            }
        }
        .onDrop(of: [.fileURL], isTargeted: nil) { providers in
            let group = DispatchGroup()
            var urls: [URL] = []
            for p in providers {
                group.enter()
                _ = p.loadObject(ofClass: URL.self) { url, _ in
                    if let url { urls.append(url) }
                    group.leave()
                }
            }
            group.notify(queue: .main) { model.importURLs(urls) }
            return true
        }
    }

    private var sidebar: some View {
        let items = model.filtered
        let groups: [(String, [TemplateEntry])] = [
            ("我的模板", items.filter { !$0.builtin && !$0.partial }),
            ("内置", items.filter { $0.builtin && !$0.partial }),
            ("公共片段", items.filter { $0.partial }),
        ]
        return List(selection: Binding(get: { model.selection }, set: { model.select($0) })) {
            ForEach(groups, id: \.0) { title, list in
                if !list.isEmpty {
                    Section(title) {
                        ForEach(list, id: \.name) { e in
                            SidebarRow(entry: e).tag(e.name).contextMenu { menu(e) }
                        }
                    }
                }
            }
        }
        .searchable(text: $model.search, placement: .sidebar, prompt: "搜索模板")
    }

    @ViewBuilder private func menu(_ e: TemplateEntry) -> some View {
        Button("复制为新模板") { model.duplicate(e) }
        Button("导出…") { model.exportOne(e) }
        if let path = e.path {
            Button("在访达中显示") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
            Divider()
            Button(e.overridesBuiltin ? "恢复内置版本" : "删除", role: .destructive) { model.delete(e) }
        }
    }
}

private struct SidebarRow: View {
    let entry: TemplateEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Text(entry.name).font(.qMono(13, weight: 600))
                if !entry.aliases.isEmpty {
                    Text(entry.aliases.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                }
                if entry.overridesBuiltin {
                    Text("已自定义").font(.system(size: 9, weight: .semibold))
                        .padding(.horizontal, 5).padding(.vertical, 1)
                        .background(Color.qAccent.opacity(0.2), in: Capsule())
                }
            }
            if !entry.description.isEmpty {
                Text(entry.description).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .padding(.vertical, 2)
    }
}

struct EditorView: View {
    @Bindable var editor: EditorModel
    let library: LibraryModel

    var body: some View {
        VStack(spacing: 0) {
            // 用 HStack 而不是 HSplitView：后者基于 NSSplitView，不传递工具栏安全区，内容顶部会被工具栏遮挡
            HStack(spacing: 0) {
                Group {
                    switch editor.mode {
                    case .form: FormEditor(editor: editor)
                    case .source:
                        CodeEditor(text: Binding(get: { editor.source }, set: { editor.setSource($0) }), editable: !editor.readOnly)
                    }
                }
                .frame(minWidth: 460, maxWidth: .infinity, maxHeight: .infinity)
                Divider()
                TryPanel(editor: editor).frame(width: 380).frame(maxHeight: .infinity)
            }
            // 提示条放在底部：顶部会落入工具栏的模糊区域
            if editor.readOnly {
                Divider()
                readOnlyBanner
            }
        }
        // 标题与操作放在窗口工具栏中（内容区顶部会被工具栏的模糊效果遮挡）
        .navigationTitle(editor.doc?.name ?? editor.entry.name)
        .navigationSubtitle(editor.saveError ?? (editor.dirty ? "未保存" : (editor.readOnly ? "内置模板" : "")))
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Picker("", selection: $editor.mode) {
                    Text("表单").tag(EditorModel.Mode.form)
                    Text("源码").tag(EditorModel.Mode.source)
                }
                .pickerStyle(.segmented).fixedSize()
            }
            ToolbarItem(placement: .primaryAction) {
                Button("保存") { editor.save() }
                    .keyboardShortcut("s", modifiers: .command)
                    .disabled(!editor.canSave)
                    .buttonStyle(.borderedProminent)
            }
        }
    }

    private var readOnlyBanner: some View {
        HStack {
            Image(systemName: "lock")
            Text("内置模板为只读。自定义后会保存为你自己的模板并覆盖内置版本，随时可以恢复。")
            Spacer()
            Button("自定义此模板") { library.customize(editor.entry) }
            Button("复制为新模板") { library.duplicate(editor.entry) }
        }
        .font(.callout)
        .padding(.horizontal, 16).padding(.vertical, 8)
        .background(Color.qAccent.opacity(0.10))
    }
}

// MARK: - 表单编辑

private struct FormEditor: View {
    @Bindable var editor: EditorModel

    private func bind<T>(_ get: @escaping (TemplateDoc) -> T, _ set: @escaping (inout TemplateDoc, T) -> Void) -> Binding<T> {
        Binding(get: { editor.doc.map(get)! }, set: { v in editor.updateDoc { set(&$0, v) } })
    }

    var body: some View {
        if editor.doc == nil {
            ContentUnavailableView("源码无法解析", systemImage: "exclamationmark.triangle",
                                   description: Text("请在「源码」中修正格式错误后再使用表单。"))
        } else {
            Form {
                Section("基本信息") {
                    TextField("名称", text: bind({ $0.name }, { $0.name = $1 })).font(.qMono(13))
                    TextField("别名", text: bind({ $0.aliases.joined(separator: ", ") },
                                                { $0.aliases = $1.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty } }),
                              prompt: Text("用逗号分隔，如 r, rev"))
                    TextField("描述", text: bind({ $0.description }, { $0.description = $1 }))
                    TextField("分类", text: bind({ $0.category }, { $0.category = $1 }), prompt: Text("如 编程、日常"))
                }
                .disabled(editor.readOnly)
                ParamsSection(editor: editor)
                Section {
                    CodeEditor(text: bind({ $0.body }, { $0.body = $1 }), editable: !editor.readOnly)
                        .frame(minHeight: 220)
                } header: {
                    Text("正文")
                } footer: {
                    Text("MiniJinja 语法：{{ 参数名 }}（参数名中的 - 写作 _）、{% if strict %}…{% endif %}、{% for x in focus %}…{% endfor %}；附加要求为 extra；{% include \"_extra\" %} 引用公共片段。")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
        }
    }
}

private struct ParamsSection: View {
    @Bindable var editor: EditorModel

    private var params: [ParamDoc] { editor.doc?.params ?? [] }

    var body: some View {
        Section {
            ForEach(Array(params.enumerated()), id: \.offset) { i, p in
                ParamRow(param: p, selected: editor.selectedParam == i)
                    .contentShape(Rectangle())
                    .onTapGesture { editor.selectedParam = editor.selectedParam == i ? nil : i }
                if editor.selectedParam == i {
                    ParamInspector(editor: editor, index: i).disabled(editor.readOnly)
                }
            }
            if !editor.readOnly {
                HStack {
                    Button { addParam() } label: { Label("添加参数", systemImage: "plus") }
                    if let i = editor.selectedParam {
                        Button { move(i, -1) } label: { Image(systemName: "arrow.up") }.disabled(i == 0)
                        Button { move(i, 1) } label: { Image(systemName: "arrow.down") }.disabled(i >= params.count - 1)
                        Spacer()
                        Button(role: .destructive) { remove(i) } label: { Label("删除参数", systemImage: "trash") }
                    }
                }
                .buttonStyle(.borderless)
            }
        } header: {
            Text("参数")
        } footer: {
            Text("参数顺序即命令补全和规范形式中的顺序。至多一个单选 / 多选参数可设为位置参数（可直接输入候选值，如 r security）。")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private func addParam() {
        editor.updateDoc { d in
            var n = "param", k = 2
            while d.params.contains(where: { $0.name == n }) { n = "param\(k)"; k += 1 }
            d.params.append(ParamDoc(name: n, kind: .bool, description: "", choices: [], defaultValue: nil, positional: false,
                                     short: nil, required: false, requires: [], conflicts: [], min: nil, max: nil))
        }
        editor.selectedParam = params.count - 1
    }

    private func move(_ i: Int, _ delta: Int) {
        editor.updateDoc { d in d.params.swapAt(i, i + delta) }
        editor.selectedParam = i + delta
    }

    private func remove(_ i: Int) {
        editor.updateDoc { d in d.params.remove(at: i) }
        editor.selectedParam = nil
    }
}

private func kindLabel(_ k: ParamKind) -> String {
    switch k {
    case .bool: return "开关"
    case .enum: return "单选"
    case .multi: return "多选"
    case .string: return "文本"
    case .number: return "数字"
    case .text: return "多行文本"
    }
}

private struct ParamRow: View {
    let param: ParamDoc
    let selected: Bool

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: selected ? "chevron.down" : "chevron.right").font(.caption).foregroundStyle(.tertiary)
            Text("--\(param.name)").font(.qMono(13)).foregroundStyle(Color.qOption)
            if let s = param.short, !s.isEmpty { Text("-\(s)").font(.qMono(11)).foregroundStyle(.secondary) }
            Text(kindLabel(param.kind)).font(.caption)
                .padding(.horizontal, 6).padding(.vertical, 1).background(.quaternary, in: Capsule())
            if param.positional { Text("位置").font(.caption).foregroundStyle(Color.qAccent) }
            if param.required { Text("必填").font(.caption).foregroundStyle(.orange) }
            Spacer()
            Text(param.description).font(.caption).foregroundStyle(.secondary).lineLimit(1)
        }
    }
}

private struct ParamInspector: View {
    @Bindable var editor: EditorModel
    let index: Int

    private var p: ParamDoc { editor.doc!.params[index] }

    private func bind<T>(_ get: @escaping (ParamDoc) -> T, _ set: @escaping (inout ParamDoc, T) -> Void) -> Binding<T> {
        Binding(get: { get(editor.doc!.params[index]) }, set: { v in editor.updateDoc { set(&$0.params[index], v) } })
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                TextField("参数名", text: bind({ $0.name }, { $0.name = $1 })).font(.qMono(13))
                Picker("类型", selection: bind({ $0.kind }, { p, k in
                    p.kind = k
                    p.defaultValue = nil
                    if k != .bool { p.short = nil }
                    if k != .enum && k != .multi { p.positional = false; p.choices = [] }
                    if k != .number { p.min = nil; p.max = nil }
                })) {
                    ForEach([ParamKind.bool, .enum, .multi, .string, .number, .text], id: \.self) { Text(kindLabel($0)).tag($0) }
                }
                .frame(width: 160)
            }
            TextField("说明", text: bind({ $0.description }, { $0.description = $1 }))
            HStack(spacing: 16) {
                Toggle("必填", isOn: bind({ $0.required }, { $0.required = $1 }))
                if p.kind == .bool {
                    LabeledContent("短名") {
                        TextField("", text: bind({ $0.short ?? "" }, { $0.short = $1.isEmpty ? nil : String($1.prefix(1)) }))
                            .font(.qMono(13)).frame(width: 36)
                    }
                }
                if p.kind == .enum || p.kind == .multi {
                    Toggle("位置参数", isOn: bind({ $0.positional }, { $0.positional = $1 }))
                }
            }
            defaultEditor
            if p.kind == .number {
                HStack {
                    numberField("最小值", bind({ $0.min }, { $0.min = $1 }))
                    numberField("最大值", bind({ $0.max }, { $0.max = $1 }))
                }
            }
            if p.kind == .enum || p.kind == .multi { ChoicesEditor(editor: editor, index: index) }
        }
        .padding(10)
        .background(Color.qAccent.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
    }

    @ViewBuilder private var defaultEditor: some View {
        switch p.kind {
        case .bool:
            Toggle("默认开启", isOn: bind({ $0.defaultValue == "true" }, { $0.defaultValue = $1 ? "true" : nil }))
        case .enum:
            Picker("默认值", selection: bind({ $0.defaultValue ?? "" }, { $0.defaultValue = $1.isEmpty ? nil : $1 })) {
                Text("无").tag("")
                ForEach(p.choices, id: \.value) { Text("\($0.label)（\($0.value)）").tag($0.value) }
            }
        case .multi:
            LabeledContent("默认选中") {
                HStack {
                    ForEach(p.choices, id: \.value) { c in
                        let on = (p.defaultValue ?? "").split(separator: ",").map(String.init).contains(c.value)
                        Toggle(c.label, isOn: bind({ _ in on }, { pd, v in
                            var set = (pd.defaultValue ?? "").split(separator: ",").map(String.init)
                            if v { set.append(c.value) } else { set.removeAll { $0 == c.value } }
                            let ordered = pd.choices.map(\.value).filter { set.contains($0) }
                            pd.defaultValue = ordered.isEmpty ? nil : ordered.joined(separator: ",")
                        }))
                        .toggleStyle(.button)
                    }
                }
            }
        case .number:
            numberField("默认值", bind({ $0.defaultValue.flatMap(Double.init) }, { $0.defaultValue = $1.map { $0.truncatingRemainder(dividingBy: 1) == 0 ? String(Int($0)) : String($0) } }))
        default:
            TextField("默认值", text: bind({ $0.defaultValue ?? "" }, { $0.defaultValue = $1.isEmpty ? nil : $1 }))
        }
    }

    private func numberField(_ title: String, _ value: Binding<Double?>) -> some View {
        TextField(title, value: value, format: .number).frame(width: 140)
    }
}

private struct ChoicesEditor: View {
    @Bindable var editor: EditorModel
    let index: Int

    private var choices: [Choice] { editor.doc!.params[index].choices }

    private func bind(_ i: Int, _ kp: WritableKeyPath<Choice, String>) -> Binding<String> {
        Binding(get: { editor.doc!.params[index].choices[i][keyPath: kp] },
                set: { v in editor.updateDoc { $0.params[index].choices[i][keyPath: kp] = v } })
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("候选值").font(.caption).foregroundStyle(.secondary)
            ForEach(Array(choices.enumerated()), id: \.offset) { i, _ in
                HStack {
                    TextField("值", text: bind(i, \.value)).font(.qMono(12)).frame(width: 140)
                    TextField("显示名", text: bind(i, \.label)).frame(width: 120)
                    TextField("别名（逗号分隔）", text: Binding(
                        get: { editor.doc!.params[index].choices[i].aliases.joined(separator: ", ") },
                        set: { v in editor.updateDoc { $0.params[index].choices[i].aliases = v.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty } } }))
                    Button { editor.updateDoc { $0.params[index].choices.remove(at: i) } } label: { Image(systemName: "minus.circle") }
                        .buttonStyle(.borderless)
                }
            }
            Button { editor.updateDoc { d in
                var n = "option", k = 2
                while d.params[index].choices.contains(where: { $0.value == n }) { n = "option\(k)"; k += 1 }
                d.params[index].choices.append(Choice(value: n, label: n, aliases: []))
            } } label: { Label("添加候选值", systemImage: "plus") }
                .buttonStyle(.borderless)
        }
    }
}

// MARK: - 试用与预览

private struct TryPanel: View {
    @Bindable var editor: EditorModel

    var body: some View {
        // 滚动视图会自动避开窗口工具栏（普通视图会落到工具栏的模糊区域下）
        ScrollView { content.padding(14) }
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("试用").font(.headline)
            TextField("输入命令试用，如 \(editor.check.name ?? "r") --strict 附加要求", text: $editor.tryInput)
                .font(.qMono(13)).textFieldStyle(.roundedBorder)
            if !editor.check.problems.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(editor.check.problems, id: \.self) { p in
                        Label(p, systemImage: "xmark.octagon.fill").foregroundStyle(.red).font(.callout)
                            .textSelection(.enabled)
                    }
                }
            } else if let a = editor.check.analysis {
                if let e = a.diagnostics.first(where: { $0.severity == .error }) {
                    Label(e.message, systemImage: "exclamationmark.circle").foregroundStyle(.orange).font(.callout)
                }
                if !a.params.isEmpty {
                    FlowLayout(spacing: 6) {
                        ForEach(a.params, id: \.name) { p in
                            HStack(spacing: 4) {
                                Text(p.name).font(.qMono(11)).foregroundStyle(p.isSet ? Color.qOption : .secondary)
                                Text(p.display).font(.system(size: 11, weight: p.isSet ? .semibold : .regular))
                            }
                            .padding(.horizontal, 8).padding(.vertical, 3)
                            .overlay(Capsule().strokeBorder(Color.secondary.opacity(0.3)))
                        }
                    }
                }
                if let c = a.canonical { Text(c).font(.qMono(11)).foregroundStyle(.tertiary).textSelection(.enabled) }
                if let p = a.preview {
                    Text("预览 · \(p.chars) 字 · \(p.lines) 行").font(.caption).foregroundStyle(.secondary)
                    Text(p.text).font(.system(size: 13)).lineSpacing(3).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(10)
                        .background(.background.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
                }
            }
        }
    }
}
