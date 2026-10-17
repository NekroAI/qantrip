import Foundation

/// Spike 埋点：每个事件一行 JSON，写到 ~/Library/Logs/Qantrip/spike.jsonl。
/// 只记录时间、长度、模式等指标，不记录 Prompt 正文。
enum SpikeLog {
    static let url: URL = {
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Logs/Qantrip", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("spike.jsonl")
    }()

    static func event(_ name: String, _ fields: [String: Any] = [:]) {
        var obj = fields
        obj["event"] = name
        obj["uptime"] = ProcessInfo.processInfo.systemUptime
        guard let data = try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys]),
              var line = String(data: data, encoding: .utf8) else { return }
        line += "\n"
        if let h = try? FileHandle(forWritingTo: url) {
            h.seekToEndOfFile()
            h.write(line.data(using: .utf8)!)
            try? h.close()
        } else {
            try? line.write(to: url, atomically: true, encoding: .utf8)
        }
    }

    static func ms(_ seconds: TimeInterval) -> Double {
        (seconds * 10_000).rounded() / 10
    }
}
