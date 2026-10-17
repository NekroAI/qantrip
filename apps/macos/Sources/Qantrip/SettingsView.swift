import AppKit
import QantripCore
import SwiftUI

struct SettingsView: View {
    var body: some View {
        TabView {
            GeneralSettings().tabItem { Label("通用", systemImage: "gearshape") }
            InsertSettings().tabItem { Label("插入", systemImage: "text.insert") }
            TemplateSettings().tabItem { Label("模板", systemImage: "books.vertical") }
            PrivacySettings().tabItem { Label("隐私", systemImage: "hand.raised") }
            AboutView().tabItem { Label("关于", systemImage: "info.circle") }
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
                LabeledContent("唤起快捷键") {
                    HotKeyRecorder()
                }
            } footer: {
                Text("macOS 上默认使用 ⌃Q（docs/decisions.md D5）。Windows / Linux 将另行定稿。")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section {
                Toggle("登录时启动", isOn: Binding(
                    get: { _ = settings.launchAtLoginRevision; return settings.launchAtLogin },
                    set: { settings.launchAtLogin = $0 }))
                Toggle("唤起时切换到英文输入法", isOn: $settings.forceASCIIInput)
            } footer: {
                Text("中文输入法下唤起时自动切到英文，关闭面板后恢复原输入法。输入中文附加要求时手动切换即可。")
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
                    Text(recording ? "按下新的组合键…" : settings.hotKey.display)
                        .font(.qMono(13, weight: 600))
                        .frame(minWidth: 110)
                }
                .buttonStyle(.bordered)
                .tint(recording ? .accentColor : nil)
                if settings.hotKey != AppSettings.defaultHotKey {
                    Button("恢复默认") { apply(AppSettings.defaultHotKey) }.buttonStyle(.link)
                }
            }
            if let message {
                Text(message).font(.caption).foregroundStyle(isError ? .red : .secondary)
            } else if !AppServices.shared.hotKey.isRegistered {
                Text("当前快捷键注册失败（\(AppServices.shared.hotKey.status)），可能已被其他应用占用").font(.caption).foregroundStyle(.red)
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
                message = "需要包含至少一个修饰键（⌃ ⌥ ⇧ ⌘）"
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
            message = "\(spec.display) \(conflict)，请换一个组合"
            isError = true
            AppServices.shared.hotKey.register(settings.hotKey)
            return
        }
        if AppServices.shared.hotKey.register(spec) {
            settings.hotKey = spec
            isError = false
            message = "已设置为 \(spec.display)"
        } else {
            message = "\(spec.display) 已被其他应用占用，请换一个组合"
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
                Picker("插入方式", selection: $settings.copyOnly) {
                    Text("自动粘贴到当前应用").tag(false)
                    Text("只复制，手动粘贴").tag(true)
                }
                .pickerStyle(.radioGroup)
                LabeledContent("辅助功能权限") {
                    HStack {
                        Label(trusted ? "已授予" : "未授予", systemImage: trusted ? "checkmark.circle.fill" : "exclamationmark.circle")
                            .foregroundStyle(trusted ? .green : .orange)
                        if !trusted {
                            Button("授予…") { Inserter.requestTrust() }
                        }
                    }
                }
            } footer: {
                Text("自动粘贴需要辅助功能权限，用于模拟 ⌘V。没有权限时会自动退回只复制。剪贴板会在粘贴后恢复为原内容，Prompt 会被标记为临时内容，不进入剪贴板管理器的历史。")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section {
                Toggle("⌘↩ 展开粘贴（Claude Code）", isOn: $settings.expandPasteEnabled)
                LabeledContent("两次粘贴间隔") {
                    HStack {
                        Slider(value: $settings.expandPasteIntervalMs, in: 100...800, step: 50).frame(width: 180)
                        Text("\(Int(settings.expandPasteIntervalMs)) ms").font(.qMono(11)).frame(width: 56, alignment: .trailing)
                    }
                }
                .disabled(!settings.expandPasteEnabled)
            } footer: {
                Text("Claude Code 会把超过 800 字符或多于 3 行的粘贴折叠为 [Pasted text]，再次粘贴相同内容即可展开。⌘↩ 只在内容会被折叠时才粘贴两次。")
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
                LabeledContent("模板数量") { Text("\(count) 个") }
                LabeledContent("模板目录") {
                    HStack {
                        Text(AppServices.shared.templatesDir.path.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                            .font(.qMono(11)).lineLimit(1).truncationMode(.middle).textSelection(.enabled)
                        Button("在访达中显示") { NSWorkspace.shared.open(AppServices.shared.templatesDir) }
                    }
                }
                HStack {
                    Button("打开模板库…") { LibraryWindow.show() }
                    Button("重新加载") {
                        AppServices.shared.engine.reload()
                        problems = AppServices.shared.engine.problems()
                        count = AppServices.shared.engine.templates().count
                    }
                }
            }
            if !problems.isEmpty {
                Section("加载问题") {
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
                Toggle("记录使用习惯", isOn: $settings.recordUsage)
                Button(cleared ? "已清除" : "清除使用记录") {
                    AppServices.shared.engine.clearUsage()
                    cleared = true
                }
                .disabled(cleared)
            } footer: {
                Text("用于首屏的「最近施放」和「常用」排序，以及按应用区分常用模板。只记录模板名和参数，不记录附加要求和生成的 Prompt，数据只保存在本机。")
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
            Text("按 Q 施放一个戏法").foregroundStyle(.secondary)
            Text("版本 \(version) · core \(coreVersion())").font(.qMono(11)).foregroundStyle(.secondary)
            Divider().frame(width: 280)
            Text("以 GPL-3.0-or-later 许可开源\n内置字体 Monaspace Neon、Fraunces 以 SIL OFL 1.1 许可分发")
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
