import AppKit
import QantripCore
import SwiftUI

/// 设置页面：与模板库合在同一个窗口的侧边栏中（LibraryWindow）
enum SettingsPage: String, CaseIterable, Identifiable {
    case general, insert, templates, sync, privacy, about
    var id: String { rawValue }
    /// 侧边栏中的选择标识
    var key: String { "settings:\(rawValue)" }

    init?(key: String?) {
        guard let key, key.hasPrefix("settings:") else { return nil }
        self.init(rawValue: String(key.dropFirst("settings:".count)))
    }

    var title: String {
        switch self {
        case .general: L("通用", "General")
        case .insert: L("插入", "Insert")
        case .templates: L("模板文件", "Template Files")
        case .sync: L("同步", "Sync")
        case .privacy: L("隐私", "Privacy")
        case .about: L("关于", "About")
        }
    }

    var icon: String {
        switch self {
        case .general: "gearshape"
        case .insert: "text.insert"
        case .templates: "folder"
        case .sync: "arrow.triangle.2.circlepath"
        case .privacy: "hand.raised"
        case .about: "info.circle"
        }
    }
}

/// 设置页容器：顶部分段控件切换页面
struct SettingsContainer: View {
    let page: SettingsPage
    let select: (SettingsPage) -> Void

    var body: some View {
        VStack(spacing: 0) {
            Picker("", selection: Binding(get: { page }, set: select)) {
                ForEach(SettingsPage.allCases) { p in Label(p.title, systemImage: p.icon).tag(p) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .padding(.top, 12)
            SettingsPageView(page: page)
        }
        .navigationTitle(L("设置", "Settings"))
    }
}

struct SettingsPageView: View {
    let page: SettingsPage

    var body: some View {
        Group {
            switch page {
            case .general: GeneralSettings()
            case .insert: InsertSettings()
            case .templates: TemplateSettings()
            case .sync: SyncSettings()
            case .privacy: PrivacySettings()
            case .about: AboutView()
            }
        }
        .frame(maxWidth: 680, maxHeight: .infinity, alignment: .top)
        .frame(maxWidth: .infinity)
    }
}

// MARK: - 通用

private struct GeneralSettings: View {
    @Bindable private var settings = AppSettings.shared

    var body: some View {
        Form {
            Section {
                LabeledContent(L("唤起快捷键", "Panel shortcut")) {
                    HotKeyRecorder()
                }
            } footer: {
                Text(L("默认是 ⌃Q。点击右侧按键可以修改。", "The default is ⌃Q. Click the shortcut to change it."))
                    .font(.caption).foregroundStyle(.secondary)
            }
            ExcludedAppsSection()
            Section {
                Picker(L("界面语言", "Language"), selection: $settings.interfaceLanguage) {
                    Text(L("跟随系统", "System default")).tag(LanguageChoice.auto)
                    Text("中文").tag(LanguageChoice.zh)
                    Text("English").tag(LanguageChoice.en)
                }
                Picker(L("内置模板语言", "Built-in templates"), selection: $settings.templateLanguage) {
                    Text(L("跟随界面语言", "Same as language")).tag(LanguageChoice.auto)
                    Text("中文").tag(LanguageChoice.zh)
                    Text("English").tag(LanguageChoice.en)
                }
            } footer: {
                Text(L("你自己的模板不受影响。", "Your own templates are not affected."))
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section {
                Toggle(L("登录时启动", "Launch at login"), isOn: Binding(
                    get: { _ = settings.launchAtLoginRevision; return settings.launchAtLogin },
                    set: { settings.launchAtLogin = $0 }))
                Toggle(L("唤起时切换到英文输入法", "Switch to English input source when opened"), isOn: $settings.forceASCIIInput)
            } footer: {
                Text(L("中文输入法下唤起时自动切到英文，关闭面板后恢复原输入法。输入中文附加要求时手动切换即可。", "When opened with a Chinese input source, it switches to English and is restored when the panel closes. Switch manually to type extra instructions in Chinese."))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

/// 快捷键录制：点击后按下新的组合键；Esc 取消。
struct HotKeyRecorder: View {
    @Bindable private var settings = AppSettings.shared
    @State private var recording = false
    @State private var message: String?
    @State private var isError = false
    @State private var monitor: Any?

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            HStack(spacing: 8) {
                Button {
                    recording ? stop() : start()
                } label: {
                    Text(recording ? L("按下新的组合键…", "Press new shortcut…") : settings.hotKey.display)
                        .font(.qMono(13, weight: 600))
                        .frame(minWidth: 110)
                }
                .buttonStyle(.bordered)
                .tint(recording ? .accentColor : nil)
                if settings.hotKey != AppSettings.defaultHotKey {
                    Button(L("恢复默认", "Restore default")) { apply(AppSettings.defaultHotKey) }.buttonStyle(.link)
                }
            }
            if let message {
                Text(message).font(.caption).foregroundStyle(isError ? .red : .secondary)
            } else if !AppServices.shared.hotKey.isRegistered {
                Text(L("当前快捷键注册失败（\(AppServices.shared.hotKey.status)），可能已被其他应用占用", "Could not register the current shortcut (\(AppServices.shared.hotKey.status)). It may be in use by another app")).font(.caption).foregroundStyle(.red)
            }
        }
        .onDisappear { stop() }
    }

    private func start() {
        recording = true
        message = nil
        AppServices.shared.hotKey.suspend()
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            if event.keyCode == 53 { stop(); return nil } // Esc
            let mods = HotKeySpec.carbon(event.modifierFlags)
            let isFunctionKey = (122...127).contains(event.keyCode) || [96, 97, 98, 99, 100, 101, 103, 109, 111].contains(event.keyCode)
            if mods == 0 && !isFunctionKey {
                message = L("需要包含至少一个修饰键（⌃ ⌥ ⇧ ⌘）", "Include at least one modifier key (⌃ ⌥ ⇧ ⌘)")
                isError = true
                return nil
            }
            apply(HotKeySpec(keyCode: UInt32(event.keyCode), modifiers: mods))
            return nil
        }
    }

    private func stop() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        if recording {
            recording = false
            AppServices.shared.hotKey.register(settings.hotKey)
        }
    }

    private func apply(_ spec: HotKeySpec) {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        recording = false
        if let conflict = spec.systemConflict() {
            message = L("\(spec.display) \(conflict)，请换一个组合", "\(spec.display): \(conflict). Choose another shortcut")
            isError = true
            AppServices.shared.hotKey.register(settings.hotKey)
            return
        }
        if AppServices.shared.hotKey.register(spec) {
            settings.hotKey = spec
            isError = false
            message = L("已设置为 \(spec.display)", "Set to \(spec.display)")
        } else {
            message = L("\(spec.display) 已被其他应用占用，请换一个组合", "\(spec.display) is in use by another app. Choose another shortcut")
            isError = true
            AppServices.shared.hotKey.register(settings.hotKey)
        }
    }
}

// MARK: - 插入

private struct InsertSettings: View {
    @Bindable private var settings = AppSettings.shared
    @State private var trusted = Inserter.isTrusted
    private let timer = Timer.publish(every: 1, on: .main, in: .common).autoconnect()

    var body: some View {
        Form {
            Section {
                Picker(L("插入方式", "Insert method"), selection: $settings.copyOnly) {
                    Text(L("自动粘贴到当前应用", "Auto-paste into the current app")).tag(false)
                    Text(L("只复制，手动粘贴", "Copy only, paste manually")).tag(true)
                }
                .pickerStyle(.radioGroup)
                LabeledContent(L("辅助功能权限", "Accessibility permission")) {
                    HStack {
                        Label(trusted ? L("已授予", "Granted") : L("未授予", "Not granted"), systemImage: trusted ? "checkmark.circle.fill" : "exclamationmark.circle")
                            .foregroundStyle(trusted ? .green : .orange)
                        if !trusted {
                            Button(L("授予…", "Grant…")) { Inserter.requestTrust() }
                        }
                    }
                }
            } footer: {
                Text(L("自动粘贴需要辅助功能权限，用于模拟 ⌘V。没有权限时会自动退回只复制。剪贴板会在粘贴后恢复为原内容，Prompt 会被标记为临时内容，不进入剪贴板管理器的历史。", "Auto-paste needs Accessibility permission to simulate ⌘V. Without it, Qantrip falls back to copy only. The clipboard is restored after pasting, and the prompt is marked as transient so clipboard managers don’t keep it in their history."))
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section {
                Toggle(L("⌘↩ 展开粘贴（Claude Code）", "⌘↩ expand paste (Claude Code)"), isOn: $settings.expandPasteEnabled)
                LabeledContent(L("两次粘贴间隔", "Interval between pastes")) {
                    HStack {
                        Slider(value: $settings.expandPasteIntervalMs, in: 100...800, step: 50).frame(width: 180)
                        Text("\(Int(settings.expandPasteIntervalMs)) ms").font(.qMono(11)).frame(width: 56, alignment: .trailing)
                    }
                }
                .disabled(!settings.expandPasteEnabled)
            } footer: {
                Text(L("Claude Code 会把超过 800 字符或多于 3 行的粘贴折叠为 [Pasted text]，再次粘贴相同内容即可展开。⌘↩ 只在内容会被折叠时才粘贴两次。", "Claude Code collapses pastes over 800 characters or more than 3 lines into [Pasted text]; pasting the same content again expands it. ⌘↩ pastes twice only when the content would be collapsed."))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .onReceive(timer) { _ in trusted = Inserter.isTrusted }
    }
}

// MARK: - 模板

private struct TemplateSettings: View {
    @State private var problems = AppServices.shared.engine.problems()
    @State private var count = AppServices.shared.engine.templates().count

    var body: some View {
        Form {
            Section {
                LabeledContent(L("模板数量", "Template count")) { Text(L("\(count) 个", "\(count)")) }
                LabeledContent(L("模板目录", "Template folder")) {
                    HStack {
                        Text(AppServices.shared.templatesDir.path.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                            .font(.qMono(11)).lineLimit(1).truncationMode(.middle).textSelection(.enabled)
                        Button(L("在访达中显示", "Show in Finder")) { NSWorkspace.shared.open(AppServices.shared.templatesDir) }
                    }
                }
                HStack {
                    Button(L("打开模板库…", "Open Template Library…")) { LibraryWindow.show() }
                    Button(L("重新加载", "Reload")) {
                        AppServices.shared.engine.reload()
                        problems = AppServices.shared.engine.problems()
                        count = AppServices.shared.engine.templates().count
                    }
                }
            }
            if !problems.isEmpty {
                Section(L("加载问题", "Loading issues")) {
                    ForEach(problems, id: \.self) { Label($0, systemImage: "exclamationmark.triangle").foregroundStyle(.orange) }
                }
            }
        }
        .formStyle(.grouped)
    }
}

// MARK: - 隐私

private struct PrivacySettings: View {
    @Bindable private var settings = AppSettings.shared
    @State private var cleared = false

    var body: some View {
        Form {
            Section {
                Toggle(L("记录使用习惯", "Record usage"), isOn: $settings.recordUsage)
                Button(cleared ? L("已清除", "Cleared") : L("清除使用记录", "Clear Usage History")) {
                    AppServices.shared.engine.clearUsage()
                    cleared = true
                }
                .disabled(cleared)
            } footer: {
                Text(L("用于首屏的「最近施放」和「常用」排序，以及按应用区分常用模板。只记录模板名和参数，不记录附加要求和生成的 Prompt，数据只保存在本机。", "Used for “Recent” and “Frequent” ordering on the start screen, and to tell which templates you use in which app. Only template names and parameters are recorded, never extra instructions or generated prompts. Data stays on this Mac."))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

// MARK: - 关于

private struct AboutView: View {
    private var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "?"
    }

    var body: some View {
        VStack(spacing: 12) {
            Image(nsImage: NSApp.applicationIconImage).resizable().frame(width: 72, height: 72)
            Text("Nekro Qantrip").font(.qDisplay(26))
            Text(L("按 Q 施放一个戏法", "Press Q to cast a spell")).foregroundStyle(.secondary)
            Text(L("版本 \(version) · core \(coreVersion())", "Version \(version) · core \(coreVersion())")).font(.qMono(11)).foregroundStyle(.secondary)
            UpdateSection().frame(width: 360)
            Divider().frame(width: 280)
            Text(L("以 GPL-3.0-or-later 许可开源\n内置字体 Monaspace Neon、Fraunces 以 SIL OFL 1.1 许可分发", "Open source under GPL-3.0-or-later\nBundled fonts Monaspace Neon and Fraunces are distributed under SIL OFL 1.1"))
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}


/// 排除的应用（docs/decisions.md D35）：前台是这些应用时，唤起快捷键原样交给应用。
private struct ExcludedAppsSection: View {
    @Bindable private var settings = AppSettings.shared

    var body: some View {
        Section {
            ForEach(settings.excludedApps) { app in
                HStack {
                    AppIcon(bundleId: app.bundleId)
                    Text(app.name)
                    Text(app.bundleId).font(.caption).foregroundStyle(.tertiary)
                    Spacer()
                    Button(L("移除", "Remove")) { settings.excludedApps.removeAll { $0 == app } }
                        .buttonStyle(.borderless)
                }
            }
            Menu(L("添加应用…", "Add App…")) {
                let running = runningApps
                if !running.isEmpty {
                    Section(L("正在运行", "Running")) {
                        ForEach(running) { app in
                            Button(app.name) { add(app) }
                        }
                    }
                }
                Button(L("从「应用程序」中选择…", "Choose from Applications…")) { chooseApp() }
            }
            .fixedSize()
        } header: {
            Text(L("排除的应用", "Excluded apps"))
        } footer: {
            Text(L("在这些应用里按快捷键不会唤起 Qantrip，按键照常交给应用本身。也可以在菜单栏菜单中一键停用当前应用。", "In these apps the shortcut doesn't open Qantrip and goes to the app as usual. You can also disable the current app from the menu bar menu."))
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private var runningApps: [ExcludedApp] {
        NSWorkspace.shared.runningApplications
            .filter { $0.activationPolicy == .regular && $0.bundleIdentifier != Bundle.main.bundleIdentifier }
            .compactMap { app in app.bundleIdentifier.map { ExcludedApp(bundleId: $0, name: app.localizedName ?? $0) } }
            .filter { !settings.isExcluded($0.bundleId) }
            .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    private func add(_ app: ExcludedApp) {
        guard !settings.isExcluded(app.bundleId) else { return }
        settings.excludedApps.append(app)
    }

    private func chooseApp() {
        let panel = NSOpenPanel()
        panel.directoryURL = URL(fileURLWithPath: "/Applications")
        panel.allowedContentTypes = [.application]
        panel.allowsMultipleSelection = true
        guard panel.runModal() == .OK else { return }
        for url in panel.urls {
            guard let bundle = Bundle(url: url), let bid = bundle.bundleIdentifier else { continue }
            let name = (bundle.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String)
                ?? (bundle.object(forInfoDictionaryKey: "CFBundleName") as? String)
                ?? url.deletingPathExtension().lastPathComponent
            add(ExcludedApp(bundleId: bid, name: name))
        }
    }
}

private struct AppIcon: View {
    let bundleId: String

    var body: some View {
        if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleId) {
            Image(nsImage: NSWorkspace.shared.icon(forFile: url.path)).resizable().frame(width: 18, height: 18)
        } else {
            Image(systemName: "app").frame(width: 18, height: 18)
        }
    }
}


/// 设置 › 关于：检查更新（只提示，不自动更新）
private struct UpdateSection: View {
    @Bindable private var updates = UpdateChecker.shared
    @State private var auto = UpdateChecker.shared.autoCheck

    var body: some View {
        VStack(spacing: 8) {
            if let r = updates.available {
                VStack(spacing: 6) {
                    Text(L("有新版本 \(r.version)", "Version \(r.version) is available")).font(.headline)
                    HStack {
                        Button(L("前往下载", "Download")) { updates.openDownload() }.buttonStyle(.borderedProminent)
                        if updates.skippedVersion != r.version {
                            Button(L("跳过此版本", "Skip This Version")) { updates.skippedVersion = r.version }
                        } else {
                            Text(L("已跳过", "Skipped")).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            } else {
                HStack(spacing: 8) {
                    Button(L("检查更新", "Check for Updates")) { updates.check(manual: true) }
                        .disabled(updates.state == .checking)
                    switch updates.state {
                    case .checking: ProgressView().controlSize(.small)
                    case .upToDate: Text(L("已是最新版本", "You're up to date")).font(.caption).foregroundStyle(.secondary)
                    case .failed: Text(L("检查失败，请稍后再试", "Couldn't check. Try again later.")).font(.caption).foregroundStyle(.red)
                    case .idle: EmptyView()
                    }
                }
            }
            Toggle(L("自动检查更新", "Check for updates automatically"), isOn: $auto)
                .onChange(of: auto) { _, v in updates.autoCheck = v }
                .font(.caption)
        }
    }
}
