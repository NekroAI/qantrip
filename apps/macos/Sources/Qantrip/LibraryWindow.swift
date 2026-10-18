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
        WindowManager.shared.show("library", title: "Qantrip", size: NSSize(width: 1180, height: 760), resizable: true) {
            LibraryView(model: model)
        }
    }

    /// 打开同一个窗口中的设置页
    static func showSettings(_ page: SettingsPage = .general) {
        show(select: page.key)
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

    /// 列表中的标识：已删除的内置模板可能与同名的用户模板并存
    static func key(_ e: TemplateEntry) -> String { e.deleted ? "deleted:\(e.name)" : e.name }

    func select(_ key: String?) {
        guard key != selection else { return }
        if let e = editor, e.dirty, !confirmDiscard(e.doc?.name ?? e.entry.name) { return }
        selection = key
        editor = SettingsPage(key: key) != nil ? nil : key.flatMap { k in entries.first { Self.key($0) == k } }.map { EditorModel(entry: $0, library: self) }
    }

    private func confirmDiscard(_ name: String) -> Bool {
        let a = NSAlert()
        a.messageText = L("放弃对「\(name)」的修改？", "Discard changes to “\(name)”?")
        a.addButton(withTitle: L("放弃修改", "Discard Changes"))
        a.addButton(withTitle: L("取消", "Cancel"))
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
        let source = serializeTemplateDoc(doc: newTemplateDoc(name: name, locale: AppSettings.shared.uiLocale))
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

    /// 删除模板（docs/decisions.md D34）：用户模板移到废纸篓；内置模板（含修改过的）删除后可以恢复。
    func delete(_ e: TemplateEntry) {
        let a = NSAlert()
        if e.builtin || e.overridesBuiltin {
            a.messageText = L("删除内置模板「\(e.name)」？", "Delete the built-in template “\(e.name)”?")
            a.informativeText = e.overridesBuiltin
                ? L("你的修改会移到废纸篓。之后可以在“已删除”中恢复内置版本，它的名称和别名在此期间可以给你自己的模板使用。", "Your changes go to the Trash. You can restore the built-in version from “Deleted” later; meanwhile its name and aliases are free for your own templates.")
                : L("之后可以在“已删除”中恢复。它的名称和别名在此期间可以给你自己的模板使用。", "You can restore it from “Deleted” later. Meanwhile its name and aliases are free for your own templates.")
        } else {
            a.messageText = L("删除模板「\(e.name)」？", "Delete template “\(e.name)”?")
            a.informativeText = L("文件会被移到废纸篓。", "The file will be moved to the Trash.")
        }
        a.addButton(withTitle: L("删除", "Delete"))
        a.addButton(withTitle: L("取消", "Cancel"))
        guard a.runModal() == .alertFirstButtonReturn else { return }
        apply(try? engine.deleteTemplate(name: e.name))
        selection = nil
        editor = nil
    }

    /// 恢复内置模板：撤销删除，或丢弃修改（修改版本移到废纸篓）。
    func restore(_ e: TemplateEntry) {
        if e.overridesBuiltin {
            let a = NSAlert()
            a.messageText = L("恢复「\(e.name)」为内置版本？", "Restore “\(e.name)” to the built-in version?")
            a.informativeText = L("你的修改会移到废纸篓。", "Your changes will be moved to the Trash.")
            a.addButton(withTitle: L("恢复内置", "Restore"))
            a.addButton(withTitle: L("取消", "Cancel"))
            guard a.runModal() == .alertFirstButtonReturn else { return }
        }
        apply(try? engine.restoreTemplate(name: e.name))
        editor?.dirty = false
        selection = nil
        select(e.name)
    }

    private func apply(_ outcome: CommandOutcome?) {
        if let path = outcome?.trashPath {
            try? FileManager.default.trashItem(at: URL(fileURLWithPath: path), resultingItemURL: nil)
        }
        reload()
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
        panel.message = L("选择模板文件（.md）、文件夹或 zip", "Choose template files (.md), folders, or zip archives")
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
        a.messageText = L("导入完成：\(imported) 个", "Import complete: \(imported)")
        var info: [String] = []
        if skipped > 0 { info.append(L("跳过 \(skipped) 个", "\(skipped) skipped")) }
        if !failures.isEmpty { info.append(L("无法导入：\(failures.joined(separator: "、"))", "Could not import: \(failures.joined(separator: ", "))")) }
        a.informativeText = info.joined(separator: "\n")
        a.runModal()
    }

    private func mdFiles(in dir: URL) -> [URL] {
        (FileManager.default.enumerator(at: dir, includingPropertiesForKeys: nil)?.allObjects as? [URL] ?? [])
            .filter { $0.pathExtension.lowercased() == "md" && !$0.path.contains("__MACOSX") }
    }

    private func askConflict(_ name: String, _ sticky: inout NSApplication.ModalResponse?) -> NSApplication.ModalResponse {
        let a = NSAlert()
        a.messageText = L("已存在名为「\(name)」的模板", "A template named “\(name)” already exists")
        a.addButton(withTitle: L("覆盖", "Replace"))
        a.addButton(withTitle: L("重命名导入", "Import as Copy"))
        a.addButton(withTitle: L("跳过", "Skip"))
        a.showsSuppressionButton = true
        a.suppressionButton?.title = L("对其余冲突执行相同操作", "Apply to remaining conflicts")
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
            saveError = L("已存在名为「\(name)」的模板", "A template named “\(name)” already exists")
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
            if let page = SettingsPage(key: model.selection) {
                SettingsContainer(page: page) { model.select($0.key) }
            } else if let editor = model.editor {
                EditorView(editor: editor, library: model).id(editor.entry.name + (editor.entry.path ?? ""))
            } else {
                ContentUnavailableView(L("选择或新建一个模板", "Select or create a template"), systemImage: "wand.and.stars",
                                       description: Text(L("模板保存为 Markdown 文件，也可以用任意编辑器直接修改。", "Templates are saved as Markdown files. You can also edit them in any editor.")))
            }
        }
        .toolbar {
            ToolbarItemGroup {
                if SettingsPage(key: model.selection) == nil {
                Button { model.newTemplate() } label: { Label(L("新建", "New"), systemImage: "plus") }.help(L("新建模板", "New template"))
                Button { model.importFiles() } label: { Label(L("导入", "Import"), systemImage: "square.and.arrow.down") }.help(L("导入模板", "Import templates"))
                Button { model.exportAll() } label: { Label(L("导出全部", "Export All"), systemImage: "square.and.arrow.up") }.help(L("把我的模板导出为 zip", "Export my templates as a zip"))
                }
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
            (L("我的模板", "My templates"), items.filter { !$0.builtin && !$0.partial }),
            (L("内置", "Built-in"), items.filter { $0.builtin && !$0.partial && !$0.deleted }),
            (L("公共片段", "Shared snippets"), items.filter { $0.partial }),
            (L("已删除", "Deleted"), items.filter { $0.deleted }),
        ]
        return List(selection: Binding(get: { model.selection }, set: { model.select($0) })) {
            ForEach(groups, id: \.0) { title, list in
                if !list.isEmpty {
                    Section(title) {
                        ForEach(list, id: \.name) { e in
                            SidebarRow(entry: e).tag(LibraryModel.key(e)).contextMenu { menu(e) }
                        }
                    }
                }
            }
        }
        .searchable(text: $model.search, placement: .sidebar, prompt: L("搜索模板", "Search templates"))
        // 设置与模板库在同一个窗口：侧边栏底部固定入口
        .safeAreaInset(edge: .bottom, spacing: 0) {
            let active = SettingsPage(key: model.selection) != nil
            Button { model.select(SettingsPage.general.key) } label: {
                Label(L("设置", "Settings"), systemImage: "gearshape")
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 10).padding(.vertical, 7)
                    .background(active ? AnyShapeStyle(Color.accentColor.opacity(0.22)) : AnyShapeStyle(.clear), in: RoundedRectangle(cornerRadius: 7))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .padding(8)
            .background(.bar)
            .overlay(alignment: .top) { Divider() }
        }
    }

    @ViewBuilder private func menu(_ e: TemplateEntry) -> some View {
        Button(L("复制为新模板", "Duplicate as New Template")) { model.duplicate(e) }
        Button(L("导出…", "Export…")) { model.exportOne(e) }
        if let path = e.path, !e.deleted {
            Button(L("在访达中显示", "Show in Finder")) { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
        }
        if !e.partial {
            Divider()
            if e.deleted || e.overridesBuiltin {
                Button(L("恢复内置版本", "Restore Built-in Version")) { model.restore(e) }
            }
            if !e.deleted {
                Button(L("删除", "Delete"), role: .destructive) { model.delete(e) }
            }
        }
    }
}

private struct SidebarRow: View {
    let entry: TemplateEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 5) {
                Text(entry.name).font(.qMono(13, weight: 600))
                ForEach(entry.aliases.prefix(2), id: \.self) { a in
                    Text(a).font(.qMono(10)).foregroundStyle(.secondary)
                        .padding(.horizontal, 4).padding(.vertical, 0.5)
                        .background(.quaternary.opacity(0.6), in: RoundedRectangle(cornerRadius: 3))
                }
                if entry.overridesBuiltin {
                    Text(L("已自定义", "Customized")).font(.system(size: 9, weight: .semibold))
                        .padding(.horizontal, 5).padding(.vertical, 1)
                        .background(Color.qAccent.opacity(0.2), in: Capsule())
                }
            }
            if !entry.description.isEmpty {
                Text(entry.description).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .padding(.vertical, 3)
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
        .navigationSubtitle(editor.saveError ?? (editor.dirty ? L("未保存", "Unsaved") : (editor.readOnly ? L("内置模板", "Built-in template") : "")))
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Picker("", selection: $editor.mode) {
                    Label(L("表单", "Form"), systemImage: "list.bullet.rectangle").tag(EditorModel.Mode.form)
                    Label(L("源码", "Source"), systemImage: "chevron.left.forwardslash.chevron.right").tag(EditorModel.Mode.source)
                }
                .pickerStyle(.segmented).fixedSize()
                .help(L("表单 / 源码", "Form / Source"))
            }
            ToolbarItem(placement: .primaryAction) {
                Button { editor.save() } label: {
                    Label(L("保存", "Save"), systemImage: editor.dirty ? "checkmark.circle.fill" : "checkmark.circle")
                }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(!editor.canSave)
                .help(L("保存（⌘S）", "Save (⌘S)"))
            }
        }
    }

    @ViewBuilder private var readOnlyBanner: some View {
        if editor.entry.deleted {
            HStack {
                Image(systemName: "trash")
                Text(L("这个内置模板已删除，它的名称和别名可以给你自己的模板使用。", "This built-in template is deleted; its name and aliases are free for your own templates."))
                Spacer()
                Button(L("恢复", "Restore")) { library.restore(editor.entry) }
            }
            .font(.callout)
            .padding(.horizontal, 16).padding(.vertical, 8)
            .background(Color.orange.opacity(0.12))
        } else {
            builtinBanner
        }
    }

    private var builtinBanner: some View {
        HStack {
            Image(systemName: "lock")
            Text(L("内置模板为只读。自定义后会保存为你自己的模板并覆盖内置版本，随时可以恢复。", "Built-in templates are read-only. Customizing saves your own copy that overrides the built-in version; you can restore it anytime."))
            Spacer()
            Button(L("自定义此模板", "Customize This Template")) { library.customize(editor.entry) }
            Button(L("复制为新模板", "Duplicate as New Template")) { library.duplicate(editor.entry) }
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
            ContentUnavailableView(L("源码无法解析", "Source can’t be parsed"), systemImage: "exclamationmark.triangle",
                                   description: Text(L("请在「源码」中修正格式错误后再使用表单。", "Fix the errors in Source before using the form.")))
        } else {
            Form {
                if editor.readOnly, let d = editor.doc {
                    Section(L("基本信息", "Basics")) {
                        LabeledContent(L("名称", "Name")) { Text(d.name).font(.qMono(13)).textSelection(.enabled) }
                        LabeledContent(L("别名", "Aliases")) { Text(d.aliases.isEmpty ? "—" : d.aliases.joined(separator: ", ")).font(.qMono(13)) }
                        LabeledContent(L("描述", "Description")) { Text(d.description) }
                        LabeledContent(L("分类", "Category")) { Text(d.category.isEmpty ? "—" : d.category) }
                    }
                } else {
                Section(L("基本信息", "Basics")) {
                    TextField(L("名称", "Name"), text: bind({ $0.name }, { $0.name = $1 })).font(.qMono(13))
                    TextField(L("别名", "Aliases"), text: bind({ $0.aliases.joined(separator: ", ") },
                                                { $0.aliases = $1.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty } }),
                              prompt: Text(L("用逗号分隔，如 r, rev", "Separate with commas, e.g. r, rev")))
                    TextField(L("描述", "Description"), text: bind({ $0.description }, { $0.description = $1 }))
                    TextField(L("分类", "Category"), text: bind({ $0.category }, { $0.category = $1 }), prompt: Text(L("如 编程、日常", "e.g. Coding, Daily")))
                }
                }
                ParamsSection(editor: editor)
                Section {
                    CodeEditor(text: bind({ $0.body }, { $0.body = $1 }), editable: !editor.readOnly)
                        .frame(minHeight: 220)
                } header: {
                    Text(L("正文", "Body"))
                } footer: {
                    Text(L("MiniJinja 语法：{{ 参数名 }}（参数名中的 - 写作 _）、{% if strict %}…{% endif %}、{% for x in focus %}…{% endfor %}；附加要求为 extra；{% include \"_extra\" %} 引用公共片段。", "MiniJinja syntax: {{ param_name }} (use _ in place of - in names), {% if strict %}…{% endif %}, {% for x in focus %}…{% endfor %}. Extra instructions are available as extra; {% include \"_extra\" %} pulls in a shared snippet."))
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
                    Button { addParam() } label: { Label(L("添加参数", "Add Parameter"), systemImage: "plus") }
                    if let i = editor.selectedParam {
                        Button { move(i, -1) } label: { Image(systemName: "arrow.up") }.disabled(i == 0)
                        Button { move(i, 1) } label: { Image(systemName: "arrow.down") }.disabled(i >= params.count - 1)
                        Spacer()
                        Button(role: .destructive) { remove(i) } label: { Label(L("删除参数", "Delete Parameter"), systemImage: "trash") }
                    }
                }
                .buttonStyle(.borderless)
            }
        } header: {
            Text(L("参数", "Parameters"))
        } footer: {
            Text(L("参数顺序即命令补全和规范形式中的顺序。至多一个单选 / 多选参数可设为位置参数（可直接输入候选值，如 r security）。", "Parameter order determines the order in completion and the canonical form. At most one single- or multi-choice parameter can be positional (its values can be typed directly, e.g. r security)."))
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
    case .bool: return L("开关", "Switch")
    case .enum: return L("单选", "Single choice")
    case .multi: return L("多选", "Multiple choice")
    case .string: return L("文本", "Text")
    case .number: return L("数字", "Number")
    case .text: return L("多行文本", "Multiline text")
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
            if param.positional { Text(L("位置", "Positional")).font(.caption).foregroundStyle(Color.qAccent) }
            if param.required { Text(L("必填", "Required")).font(.caption).foregroundStyle(.orange) }
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
                TextField(L("参数名", "Name"), text: bind({ $0.name }, { $0.name = $1 })).font(.qMono(13))
                Picker(L("类型", "Type"), selection: bind({ $0.kind }, { p, k in
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
            TextField(L("说明", "Description"), text: bind({ $0.description }, { $0.description = $1 }))
            HStack(spacing: 16) {
                Toggle(L("必填", "Required"), isOn: bind({ $0.required }, { $0.required = $1 }))
                if p.kind == .bool {
                    LabeledContent(L("短名", "Short name")) {
                        TextField("", text: bind({ $0.short ?? "" }, { $0.short = $1.isEmpty ? nil : String($1.prefix(1)) }))
                            .font(.qMono(13)).frame(width: 36)
                    }
                }
                if p.kind == .enum || p.kind == .multi {
                    Toggle(L("位置参数", "Positional"), isOn: bind({ $0.positional }, { $0.positional = $1 }))
                }
            }
            defaultEditor
            if p.kind == .number {
                HStack {
                    numberField(L("最小值", "Minimum"), bind({ $0.min }, { $0.min = $1 }))
                    numberField(L("最大值", "Maximum"), bind({ $0.max }, { $0.max = $1 }))
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
            Toggle(L("默认开启", "On by default"), isOn: bind({ $0.defaultValue == "true" }, { $0.defaultValue = $1 ? "true" : nil }))
        case .enum:
            Picker(L("默认值", "Default"), selection: bind({ $0.defaultValue ?? "" }, { $0.defaultValue = $1.isEmpty ? nil : $1 })) {
                Text(L("无", "None")).tag("")
                ForEach(p.choices, id: \.value) { Text("\($0.label)（\($0.value)）").tag($0.value) }
            }
        case .multi:
            LabeledContent(L("默认选中", "Selected by default")) {
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
            numberField(L("默认值", "Default"), bind({ $0.defaultValue.flatMap(Double.init) }, { $0.defaultValue = $1.map { $0.truncatingRemainder(dividingBy: 1) == 0 ? String(Int($0)) : String($0) } }))
        default:
            TextField(L("默认值", "Default"), text: bind({ $0.defaultValue ?? "" }, { $0.defaultValue = $1.isEmpty ? nil : $1 }))
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
            Text(L("候选值", "Choices")).font(.caption).foregroundStyle(.secondary)
            ForEach(Array(choices.enumerated()), id: \.offset) { i, _ in
                HStack {
                    TextField(L("值", "Value"), text: bind(i, \.value)).font(.qMono(12)).frame(width: 140)
                    TextField(L("显示名", "Label"), text: bind(i, \.label)).frame(width: 120)
                    TextField(L("别名（逗号分隔）", "Aliases (comma-separated)"), text: Binding(
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
            } } label: { Label(L("添加候选值", "Add Choice"), systemImage: "plus") }
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
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 6) {
                Text(L("试用", "Try it")).font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary).textCase(.uppercase)
                TextField("", text: $editor.tryInput, prompt: Text(L("输入命令试用，如 \(editor.check.name ?? "r") --strict", "Try a command, e.g. \(editor.check.name ?? "r") --strict")))
                    .font(.qMono(13)).textFieldStyle(.plain)
                    .padding(.horizontal, 10).padding(.vertical, 7)
                    .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
                    .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.secondary.opacity(0.18)))
            }
            if !editor.check.problems.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(editor.check.problems, id: \.self) { p in
                        Label(p, systemImage: "xmark.octagon.fill").foregroundStyle(.red).font(.callout)
                            .textSelection(.enabled)
                    }
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.red.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
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
                                    .foregroundStyle(p.isSet ? .primary : .secondary)
                            }
                            .padding(.horizontal, 8).padding(.vertical, 3)
                            .background(p.isSet ? AnyShapeStyle(Color.qAccent.opacity(0.16)) : AnyShapeStyle(.clear), in: Capsule())
                            .overlay(Capsule().strokeBorder(p.isSet ? Color.qAccent.opacity(0.4) : Color.secondary.opacity(0.25)))
                        }
                    }
                }
                if let p = a.preview {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Text(L("预览", "Preview")).font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary).textCase(.uppercase)
                            Spacer()
                            Text(L("\(p.chars) 字 · \(p.lines) 行", "\(p.chars) chars · \(p.lines) lines")).font(.system(size: 11)).foregroundStyle(.tertiary).monospacedDigit()
                        }
                        Text(p.text).font(.system(size: 13)).lineSpacing(4).textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(12)
                            .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 10))
                            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.secondary.opacity(0.12)))
                    }
                }
            }
        }
    }
}
