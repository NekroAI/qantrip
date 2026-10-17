import QantripCore
import SwiftUI

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
                        LabeledContent(L("本机标识", "This Mac's ID")) { Text(id).font(.caption.monospaced()).textSelection(.enabled) }
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
            sync.start()
            sync.refresh()
        }
        .task {
            // 设置页打开期间，较快地刷新加入请求
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(4))
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
                Text(L("在多台 Mac 之间同步你的模板", "Sync your templates across your Macs")).font(.headline)
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
                    Text(L("把这台 Mac 加入保险库", "Add this Mac to your vault")).font(.headline)
                    Text(L("在一台已经同步的设备上打开 Qantrip 设置 › 同步，输入这里显示的校验码。", "On a Mac that already syncs, open Qantrip Settings › Sync and enter the code shown here."))
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
    @State private var code = ""

    init(join: JoinProgress) { self.join = join }

    var body: some View {
        Section {
            VStack(alignment: .leading, spacing: 12) {
                switch join.stage {
                case .waitingForApprover:
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text(L("等待已同步的设备响应…", "Waiting for a Mac that already syncs…"))
                    }
                    Text(L("在那台设备上打开 Qantrip 设置 › 同步。", "Open Qantrip Settings › Sync on that Mac."))
                        .foregroundStyle(.secondary)
                case .enterCodeOnOtherDevice:
                    Text(L("在另一台设备上输入这个校验码", "Enter this code on your other Mac")).font(.headline)
                    Text(join.sas ?? "")
                        .font(.system(size: 34, weight: .semibold, design: .monospaced))
                        .tracking(4)
                        .textSelection(.enabled)
                    Text(L("这台设备上不需要任何确认。", "Nothing to confirm on this Mac."))
                        .foregroundStyle(.secondary)
                case .enterCodeHere:
                    Text(L("输入另一台设备上显示的确认码", "Enter the code shown on your other Mac")).font(.headline)
                    HStack {
                        TextField(L("确认码", "Code"), text: $code)
                            .font(.title3.monospaced())
                            .textFieldStyle(.roundedBorder)
                            .frame(maxWidth: 220)
                            .onSubmit { sync.confirmJoin(code) }
                        Button(L("加入", "Join")) { sync.confirmJoin(code) }
                            .buttonStyle(.borderedProminent)
                            .disabled(code.count < 4 || sync.working != nil)
                    }
                    Text(L("两边互相输入校验码，确保这台 Mac 加入的是你自己的保险库。", "Entering a code on each side makes sure this Mac joins your own vault."))
                        .font(.caption).foregroundStyle(.secondary)
                case .completed:
                    Label(L("已加入", "Joined"), systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                case .expired:
                    Label(L("请求已过期", "The request expired"), systemImage: "clock.badge.exclamationmark")
                }
                Button(join.stage == .expired ? L("关闭", "Close") : L("取消", "Cancel")) { sync.cancelJoin() }
            }
            .padding(.vertical, 4)
        }
    }
}

private struct Enrolled: View {
    private var sync = SyncService.shared
    @State private var codes: [String: String] = [:]
    @State private var confirmRevoke: DeviceInfo?
    @State private var phraseFor: DeviceInfo?
    @State private var phrase = ""
    @State private var confirmReset = false

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
            if let report = sync.lastReport, report.conflicts > 0 {
                Text(L("有 \(report.conflicts) 个模板在两台设备上同时修改，已保存为冲突副本，请在模板库中合并。", "\(report.conflicts) template(s) changed on two Macs at once. Both versions were kept; merge them in the template library."))
                    .font(.caption).foregroundStyle(.orange)
            }
            HStack {
                Button(L("立即同步", "Sync now")) { sync.syncNow() }.disabled(sync.working != nil)
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

        ForEach(sync.confirmCodes.sorted(by: { $0.key < $1.key }), id: \.key) { id, confirm in
            Section(L("最后一步", "Last step")) {
                VStack(alignment: .leading, spacing: 10) {
                    Text(L("在新的 Mac 上输入这个确认码", "Enter this code on the new Mac"))
                    Text(confirm)
                        .font(.system(size: 30, weight: .semibold, design: .monospaced))
                        .tracking(4)
                        .textSelection(.enabled)
                    Button(L("完成", "Done")) { sync.dismissConfirmCode(id) }
                }
                .padding(.vertical, 4)
            }
        }

        let requests = sync.joinRequests.filter { $0.status != .handledElsewhere && sync.confirmCodes[$0.joinId] == nil }
        if !requests.isEmpty {
            Section(L("新设备请求加入", "A new Mac wants to join")) {
                ForEach(requests, id: \.joinId) { r in
                    VStack(alignment: .leading, spacing: 8) {
                        if r.status == .waitingForNewDevice {
                            HStack(spacing: 8) {
                                ProgressView().controlSize(.small)
                                Text(L("等待新设备显示校验码…", "Waiting for the new Mac to show its code…"))
                            }
                        } else {
                            Text(L("输入新设备上显示的校验码。如果不是你本人在操作，请拒绝。", "Enter the code shown on the new Mac. If this isn't you, reject it."))
                                .font(.callout)
                            HStack {
                                TextField(L("校验码", "Code"), text: Binding(get: { codes[r.joinId] ?? "" }, set: { codes[r.joinId] = $0 }))
                                    .font(.body.monospaced())
                                    .textFieldStyle(.roundedBorder)
                                    .frame(maxWidth: 200)
                                    .onSubmit { sync.approve(r.joinId, code: codes[r.joinId] ?? "") }
                                Button(L("批准", "Approve")) { sync.approve(r.joinId, code: codes[r.joinId] ?? "") }
                                    .buttonStyle(.borderedProminent)
                                    .disabled((codes[r.joinId] ?? "").count < 4 || sync.working != nil)
                                Button(L("拒绝", "Reject"), role: .destructive) { sync.reject(r.joinId) }
                            }
                        }
                    }
                    .padding(.vertical, 2)
                }
            }
        }

        Section(L("设备", "Devices")) {
            ForEach(sync.devices, id: \.deviceId) { d in
                HStack {
                    Image(systemName: "laptopcomputer")
                    Text(d.isSelf ? L("这台 Mac", "This Mac") : L("设备 \(d.rosterIndex)", "Device \(d.rosterIndex)"))
                    Text(String(d.deviceId.prefix(8))).font(.caption.monospaced()).foregroundStyle(.secondary)
                    Spacer()
                    if !d.isSelf {
                        Button(L("移除", "Remove"), role: .destructive) { confirmRevoke = d }
                            .disabled(sync.working != nil)
                    }
                }
            }
            Text(L("要添加设备，在新的 Mac 上登录同一个账号，按提示操作。", "To add a Mac, sign in with the same account on it and follow the steps."))
                .font(.caption).foregroundStyle(.secondary)
        }

        Section {
            HStack {
                Button(L("轮换密钥", "Rotate keys")) { sync.rotateKeys() }.disabled(sync.working != nil)
                Spacer()
                Button(L("重置这台设备的同步", "Reset sync on this Mac"), role: .destructive) { confirmReset = true }
            }
        } footer: {
            Text(L("重置只删除这台 Mac 的同步身份，不影响模板文件，也不影响其他设备。", "Resetting only removes this Mac's sync identity. Your template files and other Macs are not affected."))
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
        .confirmationDialog(L("重置这台设备的同步？", "Reset sync on this Mac?"), isPresented: $confirmReset) {
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

    var body: some View {
        let words = phrase.split(separator: " ").map(String.init)
        VStack(alignment: .leading, spacing: 14) {
            Text(L("你的恢复短语", "Your recovery phrase")).font(.title3.weight(.semibold))
            Text(L("抄写下来，保存在安全的地方。所有设备都丢失时，只有它能找回你的同步数据；任何人（包括我们）都无法替你恢复。", "Write it down and keep it somewhere safe. If you lose every device, it's the only way to get your synced data back. Nobody, including us, can recover it for you."))
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
            Toggle(L("我已经抄写并保存好了", "I've written it down"), isOn: $saved)
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
