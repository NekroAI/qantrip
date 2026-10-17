import Foundation

// Spike 自动化测试用：向 Qantrip 发送调试通知（需 Qantrip 开启 debugHooks）。
//   qantrip-debug toggle
//   qantrip-debug insert <模板 id> [输入]
//   qantrip-debug status

let args = CommandLine.arguments.dropFirst()
let center = DistributedNotificationCenter.default()

switch args.first {
case "toggle":
    center.postNotificationName(.init("ai.nekro.qantrip.debug.toggle"), object: nil,
                                userInfo: ["t": ProcessInfo.processInfo.systemUptime], deliverImmediately: true)
case "insert":
    let rest = Array(args.dropFirst())
    guard let id = rest.first else { fputs("用法: qantrip-debug insert <id> [input]\n", stderr); exit(2) }
    let input = rest.count > 1 ? rest[1] : id
    center.postNotificationName(.init("ai.nekro.qantrip.debug.insert"), object: nil,
                                userInfo: ["id": id, "input": input], deliverImmediately: true)
case "status":
    center.postNotificationName(.init("ai.nekro.qantrip.debug.status"), object: nil, userInfo: nil,
                                deliverImmediately: true)
default:
    fputs("用法: qantrip-debug toggle | insert <id> [input] | status\n", stderr)
    exit(2)
}
// 给通知投递留出时间
RunLoop.current.run(until: Date().addingTimeInterval(0.1))
