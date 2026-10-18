import AppKit
import UserNotifications

/// 新版本提示（不自动更新）：读取 GitHub Releases 的最新正式版本，与当前版本比较。
/// 启动约 10 秒后检查一次，之后每 24 小时一次；自动检查之间至少间隔 12 小时，失败时静默。
@MainActor
@Observable
final class UpdateChecker {
    static let shared = UpdateChecker()

    struct Release: Equatable {
        let version: String
        let url: URL
    }

    enum State: Equatable {
        case idle, checking, upToDate, failed
    }

    /// 比当前版本新的正式版本（被跳过的版本仍会记录，只是不提醒）
    private(set) var available: Release?
    private(set) var state: State = .idle

    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private let d = UserDefaults.standard
    private static let api = URL(string: "https://api.github.com/repos/NekroAI/qantrip/releases/latest")!

    static var currentVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
    }

    var autoCheck: Bool {
        get { d.object(forKey: "autoCheckUpdates") as? Bool ?? true }
        set { d.set(newValue, forKey: "autoCheckUpdates") }
    }

    var skippedVersion: String? {
        get { d.string(forKey: "skippedUpdateVersion") }
        set { d.set(newValue, forKey: "skippedUpdateVersion") }
    }

    /// 需要提醒的新版本（未被跳过）
    var pending: Release? {
        guard let available, available.version != skippedVersion else { return nil }
        return available
    }

    func start() {
        timer?.invalidate()
        DispatchQueue.main.asyncAfter(deadline: .now() + 10) { [weak self] in self?.automaticCheck() }
        timer = Timer.scheduledTimer(withTimeInterval: 3600, repeats: true) { _ in
            MainActor.assumeIsolated { UpdateChecker.shared.automaticCheck() }
        }
    }

    private func automaticCheck() {
        guard autoCheck else { return }
        let last = d.double(forKey: "lastUpdateCheck")
        guard Date().timeIntervalSince1970 - last >= 12 * 3600 else { return }
        check(manual: false)
    }

    /// 检查更新。手动检查会把结果显示在设置 › 关于中。
    func check(manual: Bool) {
        guard state != .checking else { return }
        state = .checking
        var req = URLRequest(url: Self.api, timeoutInterval: 15)
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        req.setValue("Qantrip/\(Self.currentVersion)", forHTTPHeaderField: "User-Agent")
        URLSession.shared.dataTask(with: req) { data, response, _ in
            let ok = (response as? HTTPURLResponse)?.statusCode == 200
            let json = ok ? data.flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] } : nil
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self.finish(json, manual: manual) }
            }
        }.resume()
    }

    private func finish(_ json: [String: Any]?, manual: Bool) {
        guard let json, let tag = json["tag_name"] as? String,
              json["draft"] as? Bool != true, json["prerelease"] as? Bool != true,
              let page = (json["html_url"] as? String).flatMap(URL.init(string:))
        else {
            state = .failed
            return
        }
        d.set(Date().timeIntervalSince1970, forKey: "lastUpdateCheck")
        let version = tag.hasPrefix("v") ? String(tag.dropFirst()) : tag
        SpikeLog.event("update_check", ["latest": version, "current": Self.currentVersion, "manual": manual])
        if Self.isNewer(version, than: Self.currentVersion) {
            let release = Release(version: version, url: page)
            let isNew = available != release
            available = release
            state = .idle
            if isNew, !manual, pending != nil { notify(release) }
        } else {
            available = nil
            state = .upToDate
        }
    }

    /// 按数字逐段比较版本号（如 0.3.10 > 0.3.9）
    static func isNewer(_ a: String, than b: String) -> Bool {
        let pa = a.split(separator: ".").map { Int($0) ?? 0 }
        let pb = b.split(separator: ".").map { Int($0) ?? 0 }
        for i in 0..<max(pa.count, pb.count) {
            let x = i < pa.count ? pa[i] : 0, y = i < pb.count ? pb[i] : 0
            if x != y { return x > y }
        }
        return false
    }

    /// 每个版本只提醒一次；用静默授权（不弹出授权请求），通知出现在通知中心
    private func notify(_ release: Release) {
        guard d.string(forKey: "notifiedUpdateVersion") != release.version else { return }
        d.set(release.version, forKey: "notifiedUpdateVersion")
        let center = UNUserNotificationCenter.current()
        center.requestAuthorization(options: [.provisional, .alert]) { granted, _ in
            guard granted else { return }
            let content = UNMutableNotificationContent()
            content.title = L("Qantrip 有新版本 \(release.version)", "Qantrip \(release.version) is available")
            content.body = L("在菜单栏的 Qantrip 菜单中查看并下载。", "Open the Qantrip menu in the menu bar to download it.")
            center.add(UNNotificationRequest(identifier: "update-\(release.version)", content: content, trigger: nil))
        }
    }

    func openDownload() {
        NSWorkspace.shared.open(available?.url ?? URL(string: "https://github.com/NekroAI/qantrip/releases/latest")!)
    }
}
