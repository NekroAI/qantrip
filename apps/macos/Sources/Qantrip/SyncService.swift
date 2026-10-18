import AppKit
import Network
import QantripCore
import Security

// 端到端加密同步的 macOS 外壳（docs/sync-design.md，C3）。加密、花名册与协议都在 Rust 核心里；
// 这里只负责：钥匙串存储、浏览器登录回调、在后台线程调用核心、定时同步、把状态交给界面。

/// 核心要求的安全存储。所有同步密钥合在一个钥匙串条目里（属性列表），每个服务端一个条目。
/// 没有 Developer ID 时，系统按每次构建的代码哈希授权钥匙串访问，更新后首次读取会请求一次授权；
/// 合成一个条目让每次更新最多只询问一次。
final class KeychainSecretStore: SecretStore, @unchecked Sendable {
    private let service: String
    private let lock = NSLock()
    private var cache: [String: Data]?

    init(service: String) { self.service = service }

    private var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "secrets"]
    }

    private func load() throws -> [String: Data] {
        if let cache { return cache }
        var q = query
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        var map: [String: Data] = [:]
        if status == errSecSuccess, let data = out as? Data {
            map = (try? PropertyListDecoder().decode([String: Data].self, from: data)) ?? [:]
        } else if status != errSecItemNotFound {
            throw SecretStoreError.Failed(message: "Keychain \(status)")
        }
        cache = map
        return map
    }

    private func save(_ map: [String: Data]) throws {
        let data = try PropertyListEncoder().encode(map)
        let update = SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if update == errSecItemNotFound {
            var add = query
            add[kSecValueData as String] = data
            add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            add[kSecAttrLabel as String] = "Qantrip sync"
            let status = SecItemAdd(add as CFDictionary, nil)
            guard status == errSecSuccess else { throw SecretStoreError.Failed(message: "Keychain \(status)") }
        } else if update != errSecSuccess {
            throw SecretStoreError.Failed(message: "Keychain \(update)")
        }
        cache = map
    }

    func get(key: String) throws -> Data? {
        lock.lock(); defer { lock.unlock() }
        return try load()[key]
    }

    func set(key: String, value: Data) throws {
        lock.lock(); defer { lock.unlock() }
        var map = try load()
        map[key] = value
        try save(map)
    }

    func delete(key: String) throws {
        lock.lock(); defer { lock.unlock() }
        var map = try load()
        guard map.removeValue(forKey: key) != nil else { return }
        try save(map)
    }
}

/// 浏览器登录的本机回调（PKCE，RFC 8252）：在 127.0.0.1 的随机端口上接收一次 `/callback?code=…&state=…`。
final class LoopbackReceiver: @unchecked Sendable {
    private var listener: NWListener?
    private var completion: ((Result<(code: String, state: String), Error>) -> Void)?
    private let queue = DispatchQueue(label: "ai.nekro.qantrip.loopback")

    struct Failed: LocalizedError {
        let errorDescription: String?
    }

    /// 开始监听，返回回调地址。
    func start(completion: @escaping (Result<(code: String, state: String), Error>) -> Void) throws -> String {
        let params = NWParameters.tcp
        params.requiredLocalEndpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: .any)
        let listener = try NWListener(using: params)
        self.listener = listener
        self.completion = completion
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { state in
            if case .ready = state { ready.signal() }
            if case .failed = state { ready.signal() }
        }
        listener.newConnectionHandler = { [weak self] conn in self?.handle(conn) }
        listener.start(queue: queue)
        _ = ready.wait(timeout: .now() + 3)
        guard let port = listener.port?.rawValue else { throw Failed(errorDescription: L("无法监听本机端口", "Couldn't open a local port")) }
        return "http://127.0.0.1:\(port)/callback"
    }

    func stop() {
        listener?.cancel()
        listener = nil
        completion = nil
    }

    private func handle(_ conn: NWConnection) {
        conn.start(queue: queue)
        conn.receive(minimumIncompleteLength: 1, maximumLength: 16 * 1024) { [weak self] data, _, _, _ in
            guard let self else { return }
            let request = data.flatMap { String(data: $0, encoding: .utf8) } ?? ""
            let target = request.split(separator: "\r\n").first?.split(separator: " ").dropFirst().first.map(String.init) ?? ""
            let items = URLComponents(string: "http://127.0.0.1\(target)")?.queryItems ?? []
            let value = { (n: String) in items.first { $0.name == n }?.value }
            let ok = target.hasPrefix("/callback") && value("code") != nil && value("state") != nil
            let denied = target.hasPrefix("/callback") && value("error") != nil
            let page = ok ? L("登录完成，可以关闭此页面并回到 Qantrip。", "Signed in. You can close this tab and return to Qantrip.")
                : L("登录没有完成，请回到 Qantrip 重试。", "Sign-in didn't complete. Return to Qantrip and try again.")
            let body = "<!doctype html><meta charset=utf-8><title>Qantrip</title><body style=\"font:16px -apple-system,sans-serif;background:#0b0b0c;color:#ececee;display:grid;place-items:center;height:90vh\"><p>\(page)</p>"
            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n\(body)"
            conn.send(content: Data(response.utf8), completion: .contentProcessed { _ in conn.cancel() })
            guard ok || denied, let done = self.completion else { return }
            self.completion = nil
            DispatchQueue.main.async {
                if ok { done(.success((value("code")!, value("state")!))) }
                else { done(.failure(Failed(errorDescription: L("已取消登录", "Sign-in was cancelled")))) }
            }
            self.listener?.cancel()
        }
    }
}

/// 同步错误的界面文字（核心的错误信息是中文，这里按界面语言重新表述）。
func describe(_ error: Error) -> String {
    guard let e = error as? SyncError else { return error.localizedDescription }
    switch e {
    case .NotSignedIn: return L("需要重新登录", "Please sign in again")
    case .Network: return L("无法连接同步服务器，请检查网络", "Can't reach the sync server. Check your connection.")
    case let .Server(status, code, message): return AppSettings.shared.uiLocale == .zh ? message : "Server error \(status) (\(code))"
    case .NoVault: return L("还没有保险库", "There is no vault yet")
    case .NotEnrolled: return L("这台设备还没有加入保险库", "This device hasn't joined the vault")
    case let .Security(message): return AppSettings.shared.uiLocale == .zh ? message : "Security check failed. Sync has been paused for safety."
    case let .SasMismatch(remaining): return L("校验码不正确，还可以再试 \(remaining) 次", "Wrong code. \(remaining) attempts left.")
    case let .Cooldown(seconds): return L("错误次数过多，请 \((seconds + 59) / 60) 分钟后再试", "Too many attempts. Try again in \((seconds + 59) / 60) min.")
    case .RecoveryRequired: return L("这个操作需要恢复短语", "This needs your recovery phrase")
    case .Conflict: return L("其他设备同时在操作，请稍后重试", "Another device is busy. Try again shortly.")
    case let .InvalidInput(message): return AppSettings.shared.uiLocale == .zh ? message : "Invalid input"
    case let .InvalidState(message): return AppSettings.shared.uiLocale == .zh ? message : "Not possible right now"
    case let .Storage(message): return AppSettings.shared.uiLocale == .zh ? message : "Couldn't read or write the keychain"
    case let .DeviceLimit(maxDevices): return L("此账号最多同时在 \(maxDevices) 台设备上同步", "This account can sync on up to \(maxDevices) devices at a time")
    }
}

/// 同步服务：在后台串行调用核心，状态发布给界面。定时同步，模板目录变化后也会同步。
@MainActor
@Observable
final class SyncService {
    static let shared = SyncService()

    private(set) var status: SyncStatus?
    private(set) var remoteVault: Bool?
    private(set) var joinRequests: [JoinRequest] = []
    private(set) var devices: [DeviceInfo] = []
    private(set) var join: JoinProgress?
    /// 已登录设备批准后要显示给用户、在新设备上输入的确认码（按 join_id）
    private(set) var confirmCodes: [String: String] = [:]
    private(set) var working: String?
    var error: String?
    /// 刚创建保险库时的恢复短语，只显示一次
    var phrase: String?
    private(set) var lastReport: SyncReport?
    /// 账号下已登录的设备（含未加入保险库、超出设备上限的）
    private(set) var accountDevices: AccountDevices?
    /// 设置窗口打开时为真：此时才轮询加入请求、刷新设备列表
    var settingsVisible = false

    @ObservationIgnored private var client: SyncClient?
    @ObservationIgnored private var server = ""
    @ObservationIgnored private let queue = DispatchQueue(label: "ai.nekro.qantrip.sync")
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var pushTimer: Timer?
    /// 屏幕锁定或休眠时暂停定时检查
    @ObservationIgnored private var asleep = false
    @ObservationIgnored private var observersInstalled = false
    @ObservationIgnored private var joinTimer: Timer?
    @ObservationIgnored private var pending: DispatchWorkItem?
    @ObservationIgnored private let receiver = LoopbackReceiver()
    /// 同步拉取到新内容后调用（重新加载模板）
    @ObservationIgnored var onChange: (() -> Void)?

    var signedIn: Bool { status?.signedIn ?? false }
    var enrolled: Bool { (status?.hasVault ?? false) && (status?.enrolled ?? false) }

    private init() {}

    /// 启动或切换服务端时调用。
    func start() {
        let url = AppSettings.shared.syncServer
        guard url != server || client == nil else { return }
        server = url
        let host = URL(string: url)?.host ?? "default"
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let dataDir = support.appendingPathComponent("Qantrip/sync/\(host)", isDirectory: true)
        try? FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        let store = KeychainSecretStore(service: "ai.nekro.qantrip.sync.\(host)")
        let os = ProcessInfo.processInfo.operatingSystemVersion
        let device = DeviceLabel(
            name: Host.current().localizedName ?? "Mac",
            platform: "macOS \(os.majorVersion).\(os.minorVersion)",
            appVersion: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
        )
        client = SyncClient(serverUrl: url, dataDir: dataDir.path, templateDir: AppServices.shared.templatesDir.path, secrets: store, device: device)
        status = client?.status()
        remoteVault = nil
        joinRequests = []
        devices = []
        join = nil
        accountDevices = nil
        timer?.invalidate()
        // 每分钟让核心判断一次是否到期；没到期时不发任何请求（间隔由服务端下发的同步策略决定）
        timer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { _ in
            MainActor.assumeIsolated { SyncService.shared.trigger(.timer) }
        }
        installObservers()
        refresh()
        trigger(.launch)
    }

    /// 锁屏 / 休眠时暂停，解锁 / 唤醒后补一次检查
    private func installObservers() {
        guard !observersInstalled else { return }
        observersInstalled = true
        let ws = NSWorkspace.shared.notificationCenter
        let dist = DistributedNotificationCenter.default()
        let sleep: (Notification) -> Void = { _ in MainActor.assumeIsolated { SyncService.shared.asleep = true } }
        let wake: (Notification) -> Void = { _ in
            MainActor.assumeIsolated {
                SyncService.shared.asleep = false
                SyncService.shared.trigger(.wake)
            }
        }
        ws.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main, using: sleep)
        ws.addObserver(forName: NSWorkspace.screensDidSleepNotification, object: nil, queue: .main, using: sleep)
        ws.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main, using: wake)
        ws.addObserver(forName: NSWorkspace.screensDidWakeNotification, object: nil, queue: .main, using: wake)
        dist.addObserver(forName: .init("com.apple.screenIsLocked"), object: nil, queue: .main, using: sleep)
        dist.addObserver(forName: .init("com.apple.screenIsUnlocked"), object: nil, queue: .main, using: wake)
    }

    /// 按触发原因交给核心判断是否同步（docs/sync-design.md v0.4）；没到期时核心不发请求。
    func trigger(_ trigger: SyncTrigger) {
        guard signedIn, enrolled || status?.deviceLimit != nil else { return }
        if asleep && trigger == .timer { return }
        run(nil, { try $0.maybeSync(trigger: trigger) }) { report in
            if let report {
                self.lastReport = report
                if report.pulled > 0 || report.conflicts > 0 { self.onChange?() }
            }
            self.schedulePush()
            if (self.status?.pendingJoins ?? 0) > 0 || self.settingsVisible { self.pollJoinRequests() }
        }
    }

    /// 本地修改被节流时，在核心给出的时间点再试一次自动上传
    private func schedulePush() {
        pushTimer?.invalidate()
        guard let at = status?.nextPushAt else { return }
        let delay = max(1, TimeInterval(at) - Date().timeIntervalSince1970 + 1)
        pushTimer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) { _ in
            MainActor.assumeIsolated { SyncService.shared.trigger(.localChange) }
        }
    }

    /// 唤出面板时调用：距上次检查足够久时在后台检查一次，面板不等待
    func panelOpened() {
        trigger(.panelOpened)
    }

    /// 在后台线程执行一个核心调用。`label` 显示为进行中的操作。
    private func run<T>(_ label: String?, _ work: @escaping @Sendable (SyncClient) throws -> T, done: @escaping @MainActor (T) -> Void = { _ in }) {
        guard let client else { return }
        if let label { working = label }
        error = nil
        queue.async {
            let result = Result { try work(client) }
            let status = client.status()
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    if label != nil { self.working = nil }
                    self.status = status
                    switch result {
                    case let .success(v): done(v)
                    case let .failure(e): if label != nil { self.error = describe(e) }
                    }
                }
            }
        }
    }

    /// 读取服务端信息：是否有保险库、在册设备；设置窗口打开时再读账号设备与加入请求。
    func refresh() {
        guard let status = client?.status(), status.signedIn else { return }
        if !status.hasVault {
            run(nil, { try $0.remoteVaultExists() }) { self.remoteVault = $0 }
        } else {
            run(nil, { try $0.devices() }) { self.devices = $0 }
        }
        if settingsVisible || status.deviceLimit != nil { loadAccountDevices() }
    }

    func loadAccountDevices() {
        run(nil, { try $0.accountDevices() }) { self.accountDevices = $0 }
    }

    /// 解除一台设备的登录（超出设备上限时，或旧设备已丢失）
    func release(_ device: AccountDevice) {
        run(L("正在解除…", "Releasing…"), { try $0.releaseDevice(grantId: device.grantId) }) { _ in
            self.refresh()
            if !device.isSelf { self.trigger(.manual) }
        }
    }

    /// 模板目录变化后稍等片刻，合并连续的保存，再交给核心按上传间隔决定是否上传。
    func scheduleSync() {
        guard enrolled else { return }
        pending?.cancel()
        let work = DispatchWorkItem { MainActor.assumeIsolated { SyncService.shared.trigger(.localChange) } }
        pending = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 8, execute: work)
    }

    func syncNow() {
        run(L("正在同步…", "Syncing…"), { try $0.syncNow() }) { report in
            self.lastReport = report
            if report.pulled > 0 || report.conflicts > 0 { self.onChange?() }
            self.refresh()
        }
    }

    // MARK: 登录

    func signIn() {
        guard let client else { return }
        error = nil
        do {
            let redirect = try receiver.start { [weak self] result in
                guard let self else { return }
                switch result {
                case let .success((code, state)):
                    self.run(L("正在登录…", "Signing in…"), { try $0.finishSignIn(code: code, state: state) }) { _ in
                        NSApp.activate()
                        self.refresh()
                        self.trigger(.launch)
                    }
                case let .failure(e):
                    self.working = nil
                    self.error = e.localizedDescription
                }
            }
            working = L("请在浏览器中完成登录…", "Finish signing in in your browser…")
            let url = redirect
            queue.async {
                let result = Result { try client.beginSignIn(redirectUri: url) }
                DispatchQueue.main.async {
                    MainActor.assumeIsolated {
                        switch result {
                        case let .success(authorize):
                            #if QANTRIP_DEBUG_HOOKS
                            if let cookie = UserDefaults.standard.string(forKey: "debugSyncCookie") {
                                Self.debugAuthorize(authorize, cookie: cookie)
                                return
                            }
                            #endif
                            if let u = URL(string: authorize) { NSWorkspace.shared.open(u) }
                        case let .failure(e):
                            self.receiver.stop()
                            self.working = nil
                            self.error = describe(e)
                        }
                    }
                }
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    #if QANTRIP_DEBUG_HOOKS
    /// 仅调试构建：对本地测试服务端，用预置的网页会话 Cookie 代替浏览器完成授权（`defaults write ai.nekro.qantrip debugSyncCookie …`）。
    nonisolated private static func debugAuthorize(_ authorize: String, cookie: String) {
        guard let url = URL(string: authorize) else { return }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.setValue("qt_session=\(cookie)", forHTTPHeaderField: "cookie")
        req.setValue("\(url.scheme!)://\(url.host!):\(url.port ?? 80)", forHTTPHeaderField: "origin")
        req.httpBody = Data("intent=approve".utf8)
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "content-type")
        final class NoRedirect: NSObject, URLSessionTaskDelegate {
            func urlSession(_ s: URLSession, task: URLSessionTask, willPerformHTTPRedirection r: HTTPURLResponse, newRequest: URLRequest) async -> URLRequest? { nil }
        }
        let session = URLSession(configuration: .ephemeral, delegate: NoRedirect(), delegateQueue: nil)
        session.dataTask(with: req) { _, resp, _ in
            guard let loc = (resp as? HTTPURLResponse)?.value(forHTTPHeaderField: "location"), let cb = URL(string: loc) else { return }
            URLSession.shared.dataTask(with: cb).resume()
        }.resume()
    }
    #endif

    func cancelSignIn() {
        receiver.stop()
        working = nil
    }

    func signOut() {
        run(L("正在退出…", "Signing out…"), { try $0.signOut() }) { _ in
            self.remoteVault = nil
            self.devices = []
            self.joinRequests = []
        }
    }

    func resetDevice() {
        run(L("正在重置…", "Resetting…"), { try $0.resetDevice() }) { _ in
            self.remoteVault = nil
            self.devices = []
            self.joinRequests = []
            self.join = nil
        }
    }

    // MARK: 保险库

    func createVault() {
        let language: PhraseLanguage = AppSettings.shared.uiLocale == .zh ? .chineseSimplified : .english
        run(L("正在创建保险库…", "Creating vault…"), { try $0.createVault(language: language) }) { phrase in
            self.phrase = phrase
            self.syncNow()
        }
    }

    func recover(_ phrase: String) {
        run(L("正在恢复…", "Recovering…"), { try $0.recover(phrase: phrase) }) { _ in
            self.syncNow()
        }
    }

    // MARK: 新设备加入

    func startJoin() {
        run(L("正在发起加入请求…", "Requesting to join…"), { try $0.startJoin() }) { progress in
            self.join = progress
            self.joinTimer?.invalidate()
            self.joinTimer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { _ in
                MainActor.assumeIsolated { SyncService.shared.pollJoin() }
            }
        }
    }

    private func pollJoin() {
        run(nil, { try $0.pollJoin() }) { progress in
            self.join = progress
            switch progress.stage {
            case .completed:
                self.joinTimer?.invalidate()
                self.join = nil
                self.syncNow()
            case .expired, .enterCodeHere:
                self.joinTimer?.invalidate()
            default: break
            }
        }
    }

    /// 新设备：输入已登录设备上显示的确认码，完成双向确认后才加入。
    func confirmJoin(_ code: String) {
        run(L("正在验证…", "Verifying…"), { try $0.confirmJoin(code: code) }) { _ in
            self.join = nil
            self.syncNow()
        }
    }

    func cancelJoin() {
        joinTimer?.invalidate()
        join = nil
        run(nil, { $0.cancelJoin() })
    }

    // MARK: 已登录设备批准加入

    func pollJoinRequests() {
        guard enrolled else { return }
        run(nil, { try $0.listJoinRequests() }) { self.joinRequests = $0 }
    }

    func approve(_ joinId: String, code: String) {
        run(L("正在验证…", "Verifying…"), { try $0.approveJoin(joinId: joinId, sas: code) }) { confirm in
            self.confirmCodes[joinId] = confirm
            self.refresh()
        }
    }

    func reject(_ joinId: String) {
        run(nil, { try $0.rejectJoin(joinId: joinId) }) { _ in self.refresh() }
    }

    // MARK: 设备管理

    /// 撤销设备。需要恢复短语时 `needsPhrase` 回调为真。
    func revoke(_ deviceId: String, phrase: String?, needsPhrase: @escaping @MainActor () -> Void) {
        guard let client else { return }
        working = L("正在撤销并轮换密钥…", "Removing and rotating keys…")
        error = nil
        queue.async {
            let result = Result { try client.revokeDevice(deviceId: deviceId, recoveryPhrase: phrase) }
            let status = client.status()
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self.working = nil
                    self.status = status
                    switch result {
                    case .success: self.refresh()
                    case let .failure(e):
                        if case .RecoveryRequired = e as? SyncError { needsPhrase() } else { self.error = describe(e) }
                    }
                }
            }
        }
    }

    func rotateKeys() {
        run(L("正在轮换密钥…", "Rotating keys…"), { try $0.rotateKeys() }) { _ in self.refresh() }
    }

    /// 重新验证：服务端恢复正常后解除停止写入。
    func reverify() {
        run(L("正在重新验证…", "Re-verifying…"), { try $0.reverify() }) { _ in self.refresh() }
    }

    func dismissConfirmCode(_ joinId: String) {
        confirmCodes[joinId] = nil
    }

    func dismissAlerts() {
        run(nil, { $0.dismissAlerts() })
    }

    /// 菜单栏提示：有设备等待加入
    var pendingJoinCount: Int { max(Int(status?.pendingJoins ?? 0), joinRequests.filter { $0.status != .handledElsewhere }.count) }
}
