import AppKit
import QantripCore
import SwiftUI

struct SettingsView: View {
    var body: some View {
        TabView {
            GeneralSettings().tabItem { Label(L("通用", "General"), systemImage: "gearshape") }
            InsertSettings().tabItem { Label(L("插入", "Insert"), systemImage: "text.insert") }
            TemplateSettings().tabItem { Label(L("模板", "Templates"), systemImage: "books.vertical") }
            PrivacySettings().tabItem { Label(L("隐私", "Privacy"), systemImage: "hand.raised") }
            AboutView().tabItem { Label(L("关于", "About"), systemImage: "info.circle") }
        }
        .frame(width: 560, height: 400)
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
            Divider().frame(width: 280)
            Text(L("以 GPL-3.0-or-later 许可开源\n内置字体 Monaspace Neon、Fraunces 以 SIL OFL 1.1 许可分发", "Open source under GPL-3.0-or-later\nBundled fonts Monaspace Neon and Fraunces are distributed under SIL OFL 1.1"))
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
