import AppKit
import QantripCore
import SwiftUI
import UniformTypeIdentifiers

// 设置 › 同步（docs/sync-design.md，C3）。按状态分为：未登录 → 创建 / 加入 / 恢复 → 已加入（状态、设备、加入请求）。

struct SyncSettings: View {
    @Bindable private var sync = SyncService.shared
    @Bindable private var settings = AppSettings.shared

    var body: some View {
        Form {
            if let working = sync.working {
                Section {
                    HStack(spacing: 10) {
                        ProgressView().controlSize(.small)
                        Text(working)
                        Spacer()
                        if !sync.signedIn { Button(L("取消", "Cancel")) { sync.cancelSignIn() } }
                    }
                }
            }
            if let error = sync.error {
                Section {
                    Label(error, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.red)
                }
            }

            if !sync.signedIn {
                SignedOut()
            } else if let max = sync.status?.deviceLimit {
                DeviceLimitSection(max: max)
            } else if !(sync.status?.hasVault ?? false) {
                NoVault()
            } else {
                Enrolled()
            }

            Section {
                DisclosureGroup(L("高级", "Advanced")) {
                    LabeledContent(L("服务器", "Server")) {
                        TextField("", text: $settings.syncServer, prompt: Text("https://qantrip.nekro.ai"))
                            .textFieldStyle(.roundedBorder)
                            .onSubmit { sync.start() }
                            .disabled(sync.signedIn)
                    }
                    if let id = sync.status?.deviceId {
                        LabeledContent(L("本机标识", "This device's ID")) { Text(id).font(.caption.monospaced()).textSelection(.enabled) }
                    }
                }
            } footer: {
                Text(L("可以把服务端部署到自己的 Cloudflare 账号，在这里填写地址。登录后不能修改。", "You can deploy the server to your own Cloudflare account and enter its address here. Locked while signed in."))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .sheet(isPresented: Binding(get: { sync.phrase != nil }, set: { if !$0 { sync.phrase = nil } })) {
            PhraseSheet(phrase: sync.phrase ?? "") { sync.phrase = nil }
        }
        .onAppear {
            sync.settingsVisible = true
            sync.start()
            sync.refresh()
        }
        .onDisappear { sync.settingsVisible = false }
        .task {
            // 设置页打开期间刷新加入请求（关闭后只在服务端报告有待处理请求时才查）
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(6))
                sync.pollJoinRequests()
            }
        }
    }
}

private struct SignedOut: View {
    private var sync = SyncService.shared
    var body: some View {
        Section {
            VStack(alignment: .leading, spacing: 10) {
                Text(L("在多台设备之间同步你的模板", "Sync your templates across your devices")).font(.headline)
                Text(L("端到端加密：密钥只在你的设备上，服务器只保存无法解密的密文。", "End-to-end encrypted: keys stay on your devices, and the server only stores ciphertext it can't decrypt."))
                    .foregroundStyle(.secondary)
                Button(L("使用 GitHub 登录", "Sign in with GitHub")) { sync.signIn() }
                    .buttonStyle(.borderedProminent)
                    .disabled(sync.working != nil)
            }
            .padding(.vertical, 4)
        }
    }
}

private struct NoVault: View {
    private var sync = SyncService.shared
    @State private var mode: Mode = .choose
    @State private var phrase = ""

    enum Mode { case choose, recover }

    var body: some View {
        Section {
            LabeledContent(L("账号", "Account")) { Text(sync.status?.account ?? "") }
            Button(L("退出登录", "Sign out")) { sync.signOut() }
        }
        if let join = sync.join {
            JoinWaiting(join: join)
        } else if mode == .recover {
            Section {
                Text(L("输入创建保险库时保存的 24 个词", "Enter the 24 words you saved when creating the vault")).font(.headline)
                TextEditor(text: $phrase)
                    .font(.body.monospaced())
                    .frame(height: 80)
                HStack {
                    Button(L("返回", "Back")) { mode = .choose }
                    Spacer()
                    Button(L("恢复", "Recover")) { sync.recover(phrase) }
                        .buttonStyle(.borderedProminent)
                        .disabled(phrase.split(whereSeparator: \.isWhitespace).count < 12 || sync.working != nil)
                }
            }
        } else if sync.remoteVault == false {
            Section {
                VStack(alignment: .leading, spacing: 8) {
                    Text(L("这是你的第一台设备", "This is your first device")).font(.headline)
                    Text(L("创建保险库后会显示一份恢复短语，请抄写保存。它是所有设备都丢失时找回数据的唯一方式。", "Creating a vault shows a recovery phrase. Write it down: it's the only way back in if you lose every device."))
                        .foregroundStyle(.secondary)
                    Button(L("创建保险库", "Create vault")) { sync.createVault() }
                        .buttonStyle(.borderedProminent)
                        .disabled(sync.working != nil)
                }
                .padding(.vertical, 4)
            }
        } else if sync.remoteVault == true {
            Section {
                VStack(alignment: .leading, spacing: 8) {
                    Text(L("把这台设备加入保险库", "Add this device to your vault")).font(.headline)
                    Text(L("开始后，在一台已经同步的设备上打开 Qantrip 设置 › 同步，从三个码中点选这里显示的那个。", "Then, on a device that already syncs, open Qantrip Settings › Sync and pick the code shown here."))
                        .foregroundStyle(.secondary)
                    HStack {
                        Button(L("开始加入", "Start")) { sync.startJoin() }
                            .buttonStyle(.borderedProminent)
                            .disabled(sync.working != nil)
                        Button(L("改用恢复短语", "Use recovery phrase")) { mode = .recover }
                    }
                }
                .padding(.vertical, 4)
            }
        } else {
            Section { ProgressView().controlSize(.small) }
        }
    }
}

private struct JoinWaiting: View {
    let join: JoinProgress
    private var sync = SyncService.shared

    init(join: JoinProgress) { self.join = join }

    private var code: some View {
        Text(join.sas ?? "")
            .font(.system(size: 34, weight: .semibold, design: .monospaced))
            .tracking(4)
            .textSelection(.enabled)
    }

    var body: some View {
        Section {
            VStack(alignment: .leading, spacing: 12) {
                switch join.stage {
                case .waitingForApprover:
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text(L("等待已同步的设备响应…", "Waiting for a device that already syncs…"))
                    }
                    Text(L("在那台设备上打开 Qantrip 设置 › 同步。", "Open Qantrip Settings › Sync on that device."))
                        .foregroundStyle(.secondary)
                case .showCode:
                    Text(L("在另一台设备上点选这个码", "Pick this code on your other device")).font(.headline)
                    code
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text(L("在已同步的设备上打开 Qantrip 设置 › 同步，从三个码中点选这一个，这台设备随后自动加入。", "On a device that already syncs, open Qantrip Settings › Sync and pick this code from the three shown. This device then joins on its own."))
                            .foregroundStyle(.secondary)
                    }
                case .completed:
                    Label(L("已加入", "Joined"), systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                case .expired:
                    Label(L("请求已过期或已取消", "The request expired or was cancelled"), systemImage: "clock.badge.exclamationmark")
                }
                Button(join.stage == .expired ? L("关闭", "Close") : L("取消", "Cancel")) { sync.cancelJoin() }
            }
            .padding(.vertical, 4)
        }
    }
}

private struct Enrolled: View {
    private var sync = SyncService.shared
    @State private var confirmRevoke: DeviceInfo?
    @State private var phraseFor: DeviceInfo?
    @State private var phrase = ""
    @State private var confirmReset = false
    @State private var releaseTarget: AccountDevice?

    var body: some View {
        let status = sync.status

        if status?.writesBlocked == true {
            Section {
                Label(L("检测到异常，已暂停上传。本地模板不受影响。", "Something looked wrong, so uploads are paused. Your local templates are fine."), systemImage: "exclamationmark.shield.fill")
                    .foregroundStyle(.red)
                Button(L("重新验证", "Check again")) { sync.reverify() }.disabled(sync.working != nil)
            }
        }

        Section {
            LabeledContent(L("账号", "Account")) { Text(status?.account ?? "") }
            LabeledContent(L("上次同步", "Last synced")) {
                if let t = status?.lastSync {
                    Text(Date(timeIntervalSince1970: TimeInterval(t)), style: .relative)
                    + Text(L("前", " ago"))
                } else {
                    Text(L("尚未同步", "Not yet")).foregroundStyle(.secondary)
                }
            }
            if let usage = status?.usage {
                UsageRow(usage: usage)
            }
            if let at = status?.nextPushAt {
                Text(L("本地修改将在 \(Date(timeIntervalSince1970: TimeInterval(at)).formatted(date: .omitted, time: .shortened)) 自动上传；点「立即同步」可马上上传。", "Local changes upload automatically at \(Date(timeIntervalSince1970: TimeInterval(at)).formatted(date: .omitted, time: .shortened)). Click Sync Now to upload right away."))
                    .font(.caption).foregroundStyle(.secondary)
            }
            if let report = sync.lastReport, report.replaced > 0 {
                Text(L("有 \(report.replaced) 个模板在两台设备上都改过，已保留最后修改的版本，另一版存入历史版本。", "\(report.replaced) template(s) were changed on two devices. The latest edit was kept; the other version is in History."))
                    .font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Button(L("立即同步", "Sync now")) { sync.syncNow() }.disabled(sync.working != nil)
                Button(L("历史版本", "History")) { sync.openHistory() }
                    .help(L("同一模板在两台设备上都改过时，保留最后修改的版本；被覆盖的版本保存在这里 30 天。", "When a template is changed on two devices, the latest edit wins. The overwritten version is kept here for 30 days."))
                Spacer()
                Button(L("退出登录", "Sign out")) { sync.signOut() }
            }
        }

        if let alerts = status?.alerts, !alerts.isEmpty {
            Section(L("安全提醒", "Security alerts")) {
                ForEach(Array(alerts.enumerated()), id: \.offset) { _, a in
                    Text(AppSettings.shared.uiLocale == .zh ? a.message : a.code.replacingOccurrences(of: "_", with: " "))
                        .font(.callout)
                }
                Button(L("知道了", "Dismiss")) { sync.dismissAlerts() }
            }
        }

        ForEach(sync.approvedJoins.sorted(), id: \.self) { id in
            Section {
                VStack(alignment: .leading, spacing: 8) {
                    Label(L("已批准", "Approved"), systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                    Text(L("新设备会自动完成加入。", "The new device finishes joining on its own."))
                        .foregroundStyle(.secondary)
                    Button(L("完成", "Done")) { sync.dismissApproved(id) }
                }
                .padding(.vertical, 4)
            }
        }

        let requests = sync.joinRequests.filter { $0.status != .handledElsewhere && !sync.approvedJoins.contains($0.joinId) }
        if !requests.isEmpty {
            Section(L("新设备请求加入", "A new device wants to join")) {
                ForEach(requests, id: \.joinId) { r in
                    VStack(alignment: .leading, spacing: 10) {
                        switch r.status {
                        case .waitingForNewDevice:
                            HStack(spacing: 8) {
                                ProgressView().controlSize(.small)
                                Text(L("等待新设备显示校验码…", "Waiting for the new device to show its code…"))
                            }
                        case .incompatible:
                            Text(L("新设备上的 Qantrip 版本与这台不同，无法加入。请把两台设备都更新到最新版本。", "The new device runs a different version of Qantrip and can't join. Update both devices to the latest version."))
                                .font(.callout)
                            Button(L("拒绝", "Reject"), role: .destructive) { sync.reject(r.joinId) }
                        default:
                            Text(L("新设备上显示的是哪一个码？如果不是你本人在操作，请拒绝。", "Which code does the new device show? If this isn't you, reject it."))
                                .font(.callout)
                            HStack(spacing: 10) {
                                ForEach(r.choices, id: \.self) { c in
                                    Button { sync.approve(r.joinId, code: c) } label: {
                                        Text(c)
                                            .font(.system(size: 18, weight: .semibold, design: .monospaced))
                                            .tracking(2)
                                            .padding(.horizontal, 6)
                                            .padding(.vertical, 4)
                                    }
                                    .disabled(sync.working != nil)
                                }
                            }
                            Button(L("都不是，拒绝", "None of these — reject"), role: .destructive) { sync.reject(r.joinId) }
                        }
                    }
                    .padding(.vertical, 2)
                }
            }
        }

        Section(L("设备", "Devices")) {
            if let account = sync.accountDevices {
                ForEach(account.devices, id: \.grantId) { d in
                    AccountDeviceRow(device: d, onRelease: { releaseTarget = d })
                }
                // 已退出登录、但仍在保险库中的设备：建议从保险库移除并轮换密钥
                ForEach(sync.devices.filter { r in !r.isSelf && !account.devices.contains { $0.deviceId == r.deviceId } }, id: \.deviceId) { d in
                    HStack {
                        Image(systemName: "laptopcomputer.slash").foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(L("已退出登录的设备", "Signed-out device"))
                            Text(L("仍在保险库中。建议移除并轮换密钥。", "Still in the vault. Removing it rotates the keys.")).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button(L("从保险库移除", "Remove from Vault"), role: .destructive) { confirmRevoke = d }
                            .disabled(sync.working != nil)
                    }
                }
                Text(L("这个账号最多同时登录 \(account.maxDevices) 台设备。要添加设备，在新设备上登录同一个账号并按提示操作。", "This account can be signed in on up to \(account.maxDevices) devices. To add one, sign in with the same account on it and follow the steps."))
                    .font(.caption).foregroundStyle(.secondary)
            } else {
                ProgressView().controlSize(.small)
            }
        }
        .confirmationDialog(L("解除这台设备？", "Release this device?"), isPresented: Binding(get: { releaseTarget != nil }, set: { if !$0 { releaseTarget = nil } })) {
            Button(L("解除", "Release"), role: .destructive) {
                if let d = releaseTarget { sync.release(d) }
                releaseTarget = nil
            }
        } message: {
            Text(L("它会退出登录，无法再上传或下载。如果它还在保险库中，之后可以在这里把它移除并轮换密钥。", "It will be signed out and can no longer upload or download. If it is still in the vault, you can then remove it here, which rotates the keys."))
        }

        Section {
            HStack {
                Button(L("轮换密钥", "Rotate keys")) { sync.rotateKeys() }.disabled(sync.working != nil)
                Spacer()
                Button(L("重置这台设备的同步", "Reset sync on this device"), role: .destructive) { confirmReset = true }
            }
        } footer: {
            Text(L("重置只删除这台设备的同步身份，不影响模板文件，也不影响其他设备。", "Resetting only removes this device's sync identity. Your template files and other devices are not affected."))
                .font(.caption).foregroundStyle(.secondary)
        }
        .confirmationDialog(L("移除这台设备？", "Remove this device?"), isPresented: Binding(get: { confirmRevoke != nil }, set: { if !$0 { confirmRevoke = nil } })) {
            Button(L("移除并轮换密钥", "Remove and rotate keys"), role: .destructive) {
                if let d = confirmRevoke {
                    sync.revoke(d.deviceId, phrase: nil) { phraseFor = d }
                }
                confirmRevoke = nil
            }
        } message: {
            Text(L("被移除的设备将无法读取之后的任何同步内容。", "The removed device won't be able to read anything synced afterwards."))
        }
        .confirmationDialog(L("重置这台设备的同步？", "Reset sync on this device?"), isPresented: $confirmReset) {
            Button(L("重置", "Reset"), role: .destructive) { sync.resetDevice() }
        } message: {
            Text(L("之后需要重新加入保险库。", "You'll need to join the vault again afterwards."))
        }
        .sheet(isPresented: Binding(get: { phraseFor != nil }, set: { if !$0 { phraseFor = nil } })) {
            VStack(alignment: .leading, spacing: 12) {
                Text(L("需要恢复短语", "Recovery phrase needed")).font(.headline)
                Text(L("这是除本机外的最后一台设备。移除它需要输入恢复短语。", "This is the last other device. Removing it needs your recovery phrase."))
                    .foregroundStyle(.secondary)
                TextEditor(text: $phrase).font(.body.monospaced()).frame(height: 80)
                HStack {
                    Button(L("取消", "Cancel")) { phraseFor = nil; phrase = "" }
                    Spacer()
                    Button(L("移除", "Remove"), role: .destructive) {
                        if let d = phraseFor { sync.revoke(d.deviceId, phrase: phrase) {} }
                        phraseFor = nil
                        phrase = ""
                    }
                }
            }
            .padding(20)
            .frame(width: 440)
        }
    }
}

/// 恢复短语：只显示一次，确认已保存后才能关闭。
private struct PhraseSheet: View {
    let phrase: String
    let done: () -> Void
    @State private var saved = false
    @State private var copied = false

    private func copyPhrase() {
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(phrase, forType: .string)
        copied = true
    }

    private func savePhrase() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = L("Qantrip 恢复短语.txt", "Qantrip recovery phrase.txt")
        panel.allowedContentTypes = [.plainText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let words = phrase.split(separator: " ").enumerated().map { "\($0.offset + 1). \($0.element)" }.joined(separator: "\n")
        let text = L("Nekro Qantrip 同步恢复短语\n所有设备都丢失时，用它找回同步数据。请妥善保管，不要发给任何人。\n\n", "Nekro Qantrip sync recovery phrase\nUse it to recover your synced data if you lose every device. Keep it safe and never share it.\n\n") + words + "\n"
        try? text.write(to: url, atomically: true, encoding: .utf8)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }

    var body: some View {
        let words = phrase.split(separator: " ").map(String.init)
        VStack(alignment: .leading, spacing: 14) {
            Text(L("你的恢复短语", "Your recovery phrase")).font(.title3.weight(.semibold))
            Text(L("把它保存在安全的地方。所有设备都丢失时，只有它能找回你的同步数据；任何人（包括我们）都无法替你恢复。", "Keep it somewhere safe. If you lose every device, it's the only way to get your synced data back. Nobody, including us, can recover it for you."))
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), alignment: .leading), count: 4), spacing: 8) {
                ForEach(Array(words.enumerated()), id: \.offset) { i, w in
                    HStack(spacing: 6) {
                        Text("\(i + 1)").font(.caption.monospacedDigit()).foregroundStyle(.secondary).frame(width: 18, alignment: .trailing)
                        Text(w).font(.body.monospaced())
                    }
                }
            }
            .padding(12)
            .background(RoundedRectangle(cornerRadius: 8).fill(.quaternary.opacity(0.5)))
            .textSelection(.enabled)
            HStack {
                Button { copyPhrase() } label: { Label(copied ? L("已复制", "Copied") : L("复制", "Copy"), systemImage: copied ? "checkmark" : "doc.on.doc") }
                Button { savePhrase() } label: { Label(L("保存为文件…", "Save to File…"), systemImage: "square.and.arrow.down") }
                Spacer()
            }
            Text(L("请把它保存在只有你能访问的地方，例如密码管理器；不要发给任何人。", "Keep it somewhere only you can access, such as a password manager. Never share it."))
                .font(.caption).foregroundStyle(.secondary)
            Toggle(L("我已经保存好了", "I've saved it"), isOn: $saved)
            HStack {
                Spacer()
                Button(L("完成", "Done"), action: done).buttonStyle(.borderedProminent).disabled(!saved)
            }
        }
        .padding(22)
        .frame(width: 520)
        .interactiveDismissDisabled()
    }
}


/// 云同步用量
private struct UsageRow: View {
    let usage: SyncUsage

    var body: some View {
        let quota = max(Double(usage.quotaBytes), 1)
        let ratio = min(Double(usage.bytes) / quota, 1)
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(L("云端用量", "Cloud storage"))
                Spacer()
                Text("\(formatBytes(usage.bytes)) / \(formatBytes(usage.quotaBytes))").monospacedDigit().foregroundStyle(.secondary)
            }
            ProgressView(value: ratio).tint(ratio > 0.9 ? .red : ratio > 0.75 ? .orange : .accentColor)
            if ratio >= 1 {
                Text(L("云端空间已满：新的修改不会上传，本地模板不受影响。可以删除不用的模板腾出空间。", "Cloud storage is full: new changes won't upload; local templates are unaffected. Delete unused templates to free space."))
                    .font(.caption).foregroundStyle(.red)
            }
        }
    }
}

func formatBytes(_ n: UInt64) -> String {
    ByteCountFormatter.string(fromByteCount: Int64(n), countStyle: .file)
}

/// 账号下的一台设备：名称、系统、版本、上次在线
private struct AccountDeviceRow: View {
    let device: AccountDevice
    let onRelease: () -> Void

    var body: some View {
        HStack(alignment: .top) {
            Image(systemName: device.platform.lowercased().hasPrefix("windows") ? "pc" : "laptopcomputer")
                .frame(width: 20)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(device.name.isEmpty ? L("未命名设备", "Unnamed device") : device.name)
                    if device.isSelf { Tag(text: L("本机", "This device")) }
                    if device.limited { Tag(text: L("超出上限", "Over limit"), color: .orange) }
                    // 本机没有保险库状态时无法判断其他设备是否在保险库中
                    else if !device.inVault && SyncService.shared.enrolled { Tag(text: L("未加入保险库", "Not in vault"), color: .secondary) }
                }
                Text(details).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            Button(device.isSelf ? L("退出登录", "Sign Out") : L("解除", "Release"), role: .destructive, action: onRelease)
                .buttonStyle(.borderless)
        }
    }

    private var details: String {
        var parts = [String]()
        if !device.platform.isEmpty { parts.append(device.platform) }
        if !device.appVersion.isEmpty { parts.append("Qantrip \(device.appVersion)") }
        let seen = Date(timeIntervalSince1970: TimeInterval(device.lastSeenAt))
        let f = RelativeDateTimeFormatter()
        f.locale = Foundation.Locale(identifier: AppSettings.shared.uiLocale == .zh ? "zh_CN" : "en_US")
        parts.append(L("上次在线 ", "Last seen ") + f.localizedString(for: seen, relativeTo: Date()))
        return parts.joined(separator: " · ")
    }
}

private struct Tag: View {
    let text: String
    var color: Color = .accentColor

    var body: some View {
        Text(text).font(.system(size: 10, weight: .semibold))
            .padding(.horizontal, 5).padding(.vertical, 1)
            .background(color.opacity(0.18), in: Capsule())
    }
}

/// 本机超出设备上限：列出账号下的设备，解除一台后本机即可同步。可以在新设备上直接解除已丢失的旧设备。
private struct DeviceLimitSection: View {
    let max: UInt32
    private var sync = SyncService.shared
    @State private var target: AccountDevice?

    init(max: UInt32) { self.max = max }

    var body: some View {
        Section {
            VStack(alignment: .leading, spacing: 6) {
                Label(L("这个账号最多同时在 \(max) 台设备上同步", "This account can sync on up to \(max) devices at a time"), systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange).font(.headline)
                Text(L("解除下面任意一台设备后，这台设备就可以同步。旧设备丢失或无法再使用时，也可以在这里直接解除它。", "Release any device below and this one can sync. If an old device is lost or unusable, you can release it right here."))
                    .foregroundStyle(.secondary)
            }
            .padding(.vertical, 4)
            if let account = sync.accountDevices {
                ForEach(account.devices.filter { !$0.isSelf }, id: \.grantId) { d in
                    AccountDeviceRow(device: d, onRelease: { target = d })
                }
            } else {
                ProgressView().controlSize(.small)
            }
            Button(L("退出登录", "Sign out")) { sync.signOut() }
        }
        .onAppear { sync.loadAccountDevices() }
        .confirmationDialog(L("解除这台设备？", "Release this device?"), isPresented: Binding(get: { target != nil }, set: { if !$0 { target = nil } })) {
            Button(L("解除", "Release"), role: .destructive) {
                if let d = target { sync.release(d) }
                target = nil
            }
        } message: {
            Text(L("它会退出登录，无法再上传或下载。", "It will be signed out and can no longer upload or download."))
        }
    }
}
