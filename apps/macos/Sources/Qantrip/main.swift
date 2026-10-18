import AppKit
import Carbon
import QantripCore

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var hotKey: HotKey!
    private var launcher: LauncherController!
    private var engine: Engine!
    private var statusItem: NSStatusItem!
    private var watcher: DispatchSourceFileSystemObject?
    private var reloadWork: DispatchWorkItem?

    /// 用户模板目录（docs/decisions.md D11）
    static let templatesDir: URL = {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Qantrip/templates", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()

    func applicationDidFinishLaunching(_ notification: Notification) {
        Theme.registerFonts()
        let usage = Self.templatesDir.deletingLastPathComponent().appendingPathComponent("usage.json")
        engine = Engine(userDir: Self.templatesDir.path, usagePath: usage.path)
        launcher = LauncherController(engine: engine)

        let settings = AppSettings.shared
        engine.setLocale(ui: settings.uiLocale, templates: settings.templateLocale)
        settings.onLanguageChange = { [weak self] in
            guard let self else { return }
            self.engine.setLocale(ui: settings.uiLocale, templates: settings.templateLocale)
            LibraryWindow.model.reload()
            NotificationCenter.default.post(name: AppSettings.languageChanged, object: nil)
        }
        hotKey = HotKey(settings.hotKey)
        hotKey.onPress = { [weak self] t in
            NotificationCenter.default.post(name: Onboarding.hotKeyPressed, object: nil)
            self?.launcher.toggle(eventTime: t, source: "hotkey")
        }
        AppServices.shared = AppServices(engine: engine, hotKey: hotKey, launcher: launcher, templatesDir: Self.templatesDir)

        setupStatusItem()
        watchTemplates()
        SyncService.shared.onChange = { [weak self] in
            self?.engine.reload()
            LibraryWindow.model.reload()
        }
        SyncService.shared.start()
        // 排除的应用（D35）：前台切到这些应用时暂停快捷键，让按键原样交给应用
        NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main) { [weak self] note in
            MainActor.assumeIsolated { self?.applyExclusion(note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication) }
        }
        settings.onExcludedAppsChange = { [weak self] in self?.applyExclusion(NSWorkspace.shared.frontmostApplication) }
        applyExclusion(NSWorkspace.shared.frontmostApplication)
        #if QANTRIP_DEBUG_HOOKS
        setupDebugHooks()
        #endif

        SpikeLog.event("launch", [
            "core_version": coreVersion(),
            "hotkey_status": Int(hotKey.status),
            "ax_trusted": Inserter.isTrusted,
            "templates": engine.templates().count,
            "template_problems": engine.problems(),
        ])

        if !settings.onboardingDone {
            Onboarding.show()
        }
    }

    private func setupStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.image = NSImage(systemSymbolName: "wand.and.stars", accessibilityDescription: "Qantrip")
        let menu = NSMenu()
        menu.delegate = self
        statusItem.menu = menu
    }

    /// 模板目录变化（包括外部编辑器修改）时自动重新加载。
    private func watchTemplates() {
        let fd = open(Self.templatesDir.path, O_EVTONLY)
        guard fd >= 0 else { return }
        let src = DispatchSource.makeFileSystemObjectSource(fileDescriptor: fd, eventMask: [.write, .rename, .delete], queue: .main)
        src.setEventHandler { [weak self] in self?.scheduleReload() }
        src.setCancelHandler { close(fd) }
        src.resume()
        watcher = src
    }

    private func scheduleReload() {
        reloadWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            self?.engine.reload()
            LibraryWindow.model.reload()
            MainActor.assumeIsolated { SyncService.shared.scheduleSync() }
        }
        reloadWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
    }

    #if QANTRIP_DEBUG_HOOKS
    /// 仅调试构建（QANTRIP_DEBUG_HOOKS=1 scripts/build-macos.sh）且 `defaults write ai.nekro.qantrip debugHooks -bool true` 时生效。
    /// 它允许本机其他进程借用本应用的辅助功能权限插入任意文本，正式构建不包含。
    private func setupDebugHooks() {
        guard UserDefaults.standard.bool(forKey: "debugHooks") else { return }
        let center = DistributedNotificationCenter.default()
        let hooks: [(String, Selector)] = [
            ("ai.nekro.qantrip.debug.toggle", #selector(debugToggle(_:))),
            ("ai.nekro.qantrip.debug.insert", #selector(debugInsert(_:))),
            ("ai.nekro.qantrip.debug.status", #selector(debugStatus(_:))),
            ("ai.nekro.qantrip.debug.type", #selector(debugType(_:))),
            ("ai.nekro.qantrip.debug.confirm", #selector(debugConfirm(_:))),
            ("ai.nekro.qantrip.debug.param", #selector(debugParam(_:))),
            ("ai.nekro.qantrip.debug.window", #selector(debugWindow(_:))),
            ("ai.nekro.qantrip.debug.editor", #selector(debugEditor(_:))),
            ("ai.nekro.qantrip.debug.sync", #selector(debugSync(_:))),
        ]
        for (name, sel) in hooks {
            center.addObserver(self, selector: sel, name: .init(name), object: nil, suspensionBehavior: .deliverImmediately)
        }
    }

    @objc private func debugToggle(_ note: Notification) {
        let t = (note.userInfo?["t"] as? Double) ?? ProcessInfo.processInfo.systemUptime
        launcher.toggle(eventTime: t, source: "debug")
    }

    /// userInfo: text（直接插入的文本）或 input（按面板输入渲染）；expand 为 "1" 时展开粘贴
    @objc private func debugInsert(_ note: Notification) {
        let info = note.userInfo ?? [:]
        let text = (info["text"] as? String) ?? (info["input"] as? String).flatMap { engine.render(input: $0, selected: nil) }
        guard let text else {
            SpikeLog.event("debug_insert_error", ["reason": "render failed"])
            return
        }
        launcher.insert(text, target: NSWorkspace.shared.frontmostApplication, expand: info["expand"] as? String == "1")
    }

    /// 打开面板并把输入框设置为指定文本（用于截图检查各种输入状态）
    @objc private func debugType(_ note: Notification) {
        let text = (note.userInfo?["text"] as? String) ?? ""
        if !launcher.isVisible { launcher.show(eventTime: ProcessInfo.processInfo.systemUptime, source: "debug") }
        guard !text.isEmpty else { return }
        launcher.model.field.setText(text)
        launcher.model.textDidChange(text)
        if let n = Int((note.userInfo?["down"] as? String) ?? "0"), n > 0 {
            for _ in 0..<n { launcher.model.move(1) }
        }
    }

    @objc private func debugConfirm(_ note: Notification) {
        launcher.model.confirm(expand: false)
    }

    /// 模拟点击参数标签：name + op（toggle / set:<v> / choice:<v> / clear / begin）
    @objc private func debugParam(_ note: Notification) {
        guard let name = note.userInfo?["name"] as? String, let op = note.userInfo?["op"] as? String else { return }
        let edit: ParamEdit
        if op == "toggle" { edit = .toggle }
        else if op == "clear" { edit = .clear }
        else if op == "begin" { edit = .begin }
        else if op.hasPrefix("set:") { edit = .set(value: String(op.dropFirst(4))) }
        else if op.hasPrefix("choice:") { edit = .toggleChoice(value: String(op.dropFirst(7))) }
        else { return }
        launcher.model.editParam(name, edit)
    }

    /// 打开窗口：name = settings / library[:模板名] / onboarding
    /// 驱动同步界面：op = signin / create / join / confirm:<码> / approve:<码> / sync / reverify
    @objc private func debugSync(_ note: Notification) {
        let op = (note.userInfo?["op"] as? String) ?? ""
        MainActor.assumeIsolated { Self.runDebugSync(op) }
    }

    @MainActor private static func runDebugSync(_ op: String) {
        let s = SyncService.shared
        let arg = op.split(separator: ":", maxSplits: 1).dropFirst().first.map(String.init) ?? ""
        switch op.split(separator: ":").first.map(String.init) ?? "" {
        case "signin": s.signIn()
        case "create": s.createVault()
        case "join": s.startJoin()
        case "confirm": s.confirmJoin(arg)
        case "approve": if let r = s.joinRequests.first(where: { $0.status == .readyForCode }) { s.approve(r.joinId, code: arg) }
        case "sync": s.syncNow()
        case "reverify": s.reverify()
        case "refresh": s.refresh()
        case "status":
            let st = s.status
            SpikeLog.event("sync_status", [
                "signed_in": st?.signedIn ?? false, "has_vault": st?.hasVault ?? false, "enrolled": st?.enrolled ?? false,
                "remote_vault": s.remoteVault.map { "\($0)" } ?? "nil", "join_stage": s.join.map { "\($0.stage)" } ?? "nil",
                "sas": s.join?.sas ?? "", "confirm_codes": s.confirmCodes.values.joined(separator: ","),
                "requests": s.joinRequests.map { "\($0.status)" }.joined(separator: ","), "devices": s.devices.count,
                "working": s.working ?? "", "error": s.error ?? "", "last_sync": st?.lastSync ?? 0,
                "pulled": s.lastReport?.pulled ?? 0, "alerts": st?.alerts.count ?? 0,
            ])
        default: break
        }
    }

    @objc private func debugWindow(_ note: Notification) {
        let name = (note.userInfo?["name"] as? String) ?? ""
        if name == "settings" { openSettings() }
        else if name == "settings:sync" { openSyncSettings() }
        else if name == "onboarding" { Onboarding.show() }
        else if name.hasPrefix("library") { LibraryWindow.show(select: name.split(separator: ":").dropFirst().first.map(String.init)) }
    }

    /// 驱动模板编辑器：op = new / customize / desc:<文本> / add-param / mode:source / save / delete
    @objc private func debugEditor(_ note: Notification) {
        let m = LibraryWindow.model
        let op = (note.userInfo?["op"] as? String) ?? ""
        switch op {
        case "new": m.newTemplate()
        case "customize": if let e = m.editor { m.customize(e.entry) }
        case "add-param":
            m.editor?.updateDoc { d in
                d.params.append(ParamDoc(name: "lang", kind: .enum, description: "输出语言",
                                         choices: [Choice(value: "zh", label: "中文", aliases: []), Choice(value: "en", label: "英文", aliases: [])],
                                         defaultValue: "zh", positional: false, short: nil, required: false, requires: [], conflicts: [], min: nil, max: nil))
            }
        case "save": m.editor?.save()
        case "mode:source": m.editor?.mode = .source
        case "focus":
            // 把键窗口中第一个可编辑的文本视图设为焦点（模拟用户点进输入框）
            func find(_ v: NSView) -> NSTextView? {
                if let t = v as? NSTextView, t.isEditable { return t }
                for c in v.subviews { if let t = find(c) { return t } }
                return nil
            }
            if let w = NSApp.keyWindow ?? NSApp.windows.first(where: { $0.isVisible && $0.canBecomeKey }), let v = w.contentView, let t = find(v) {
                w.makeFirstResponder(t)
                t.setSelectedRange(NSRange(location: (t.string as NSString).length, length: 0))
            }
        case "mode:form": m.editor?.mode = .form
        case "delete":
            if let e = m.editor?.entry, let path = e.path {
                try? FileManager.default.trashItem(at: URL(fileURLWithPath: path), resultingItemURL: nil)
                m.reload()
            }
        default:
            if op.hasPrefix("desc:") { m.editor?.updateDoc { $0.description = String(op.dropFirst(5)) } }
            if op.hasPrefix("try:") { m.editor?.tryInput = String(op.dropFirst(4)) }
        }
        SpikeLog.event("editor", ["op": op, "dirty": m.editor?.dirty ?? false, "selection": m.selection ?? "",
                                  "problems": m.editor?.check.problems ?? [], "path": m.editor?.entry.path ?? ""])
    }

    @objc private func debugStatus(_ note: Notification) {
        SpikeLog.event("status", [
            "ax_trusted": Inserter.isTrusted,
            "hotkey_status": Int(hotKey.status),
            "hotkey_paused": hotKey.paused,
            "rss_mb": Self.residentMB(),
        ])
    }
    #endif

    static func residentMB() -> Double {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return kr == KERN_SUCCESS ? (Double(info.phys_footprint) / 1_048_576 * 10).rounded() / 10 : -1
    }

    /// 前台应用在排除列表中时暂停快捷键（Qantrip 自己的窗口除外）。
    private func applyExclusion(_ app: NSRunningApplication?) {
        let bid = app?.bundleIdentifier
        hotKey.setPaused(bid != Bundle.main.bundleIdentifier && AppSettings.shared.isExcluded(bid))
    }

    @objc private func toggleExclusion(_ sender: NSMenuItem) {
        guard let app = sender.representedObject as? ExcludedApp else { return }
        let settings = AppSettings.shared
        if settings.isExcluded(app.bundleId) {
            settings.excludedApps.removeAll { $0.bundleId == app.bundleId }
        } else {
            settings.excludedApps.append(app)
        }
    }

    @objc func openSyncSettings() {
        UserDefaults.standard.set("sync", forKey: "settingsTab")
        openSettings()
    }

    @objc func openSettings() {
        WindowManager.shared.show("settings", title: L("Qantrip 设置", "Qantrip Settings"), size: NSSize(width: 660, height: 540)) { SettingsView() }
    }
}

extension AppDelegate: NSMenuDelegate {
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        if SyncService.shared.pendingJoinCount > 0 {
            menu.addItem(withTitle: L("有设备请求加入同步…", "A Mac wants to join sync…"), action: #selector(openSyncSettings), keyEquivalent: "").target = self
            menu.addItem(.separator())
        }
        menu.addItem(withTitle: L("打开面板（\(hotKey.spec.display)）", "Open Panel (\(hotKey.spec.display))"), action: #selector(openPanel), keyEquivalent: "").target = self
        // 打开菜单栏菜单时前台仍是用户刚才使用的应用：一键把它加入或移出排除列表
        if let app = NSWorkspace.shared.frontmostApplication, let bid = app.bundleIdentifier, bid != Bundle.main.bundleIdentifier {
            let name = app.localizedName ?? bid
            let excluded = AppSettings.shared.isExcluded(bid)
            let item = NSMenuItem(
                title: excluded ? L("在「\(name)」中恢复快捷键", "Re-enable Shortcut in “\(name)”") : L("在「\(name)」中停用快捷键", "Disable Shortcut in “\(name)”"),
                action: #selector(toggleExclusion(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = ExcludedApp(bundleId: bid, name: name)
            menu.addItem(item)
        }
        menu.addItem(withTitle: L("模板库…", "Template Library…"), action: #selector(openLibrary), keyEquivalent: "l").target = self
        menu.addItem(withTitle: L("设置…", "Settings…"), action: #selector(openSettings), keyEquivalent: ",").target = self
        menu.addItem(.separator())

        if !hotKey.isRegistered {
            menu.addItem(withTitle: L("⚠︎ 快捷键 \(hotKey.spec.display) 注册失败，请在设置中更换", "⚠︎ Shortcut \(hotKey.spec.display) could not be registered. Change it in Settings"), action: #selector(openSettings), keyEquivalent: "").target = self
        }
        if !Inserter.isTrusted && !AppSettings.shared.copyOnly {
            menu.addItem(withTitle: L("⚠︎ 未授予辅助功能权限（只复制）", "⚠︎ Accessibility permission not granted (copy only)"), action: #selector(requestTrust), keyEquivalent: "").target = self
        }
        let problems = engine.problems()
        if !problems.isEmpty {
            menu.addItem(withTitle: L("⚠︎ \(problems.count) 个模板加载问题", problems.count == 1 ? "⚠︎ 1 template problem" : "⚠︎ \(problems.count) template problems"), action: #selector(openSettings), keyEquivalent: "").target = self
        }
        menu.addItem(withTitle: L("使用引导…", "Welcome Guide…"), action: #selector(openOnboarding), keyEquivalent: "").target = self
        menu.addItem(.separator())
        menu.addItem(withTitle: L("退出 Qantrip", "Quit Qantrip"), action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
    }

    @objc private func openPanel() {
        launcher.show(eventTime: ProcessInfo.processInfo.systemUptime, source: "menu")
    }

    @objc private func openLibrary() {
        LibraryWindow.show()
    }

    @objc private func openOnboarding() {
        Onboarding.show()
    }

    @objc private func requestTrust() {
        Inserter.requestTrust()
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
