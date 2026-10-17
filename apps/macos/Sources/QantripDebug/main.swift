import Foundation

// Spike 自动化测试用：向 Qantrip 发送调试通知（需 Qantrip 开启 debugHooks）。
//   qantrip-debug toggle
//   qantrip-debug insert <面板输入>          按面板输入渲染后插入
//   qantrip-debug paste <文本> [expand]      直接插入文本；expand 时展开粘贴
//   qantrip-debug status

let args = CommandLine.arguments.dropFirst()
let center = DistributedNotificationCenter.default()

switch args.first {
case "toggle":
    center.postNotificationName(.init("ai.nekro.qantrip.debug.toggle"), object: nil,
                                userInfo: ["t": ProcessInfo.processInfo.systemUptime], deliverImmediately: true)
case "insert":
    guard let input = args.dropFirst().first else { fputs("用法: qantrip-debug insert <面板输入>\n", stderr); exit(2) }
    center.postNotificationName(.init("ai.nekro.qantrip.debug.insert"), object: nil,
                                userInfo: ["input": input], deliverImmediately: true)
case "paste":
    let rest = Array(args.dropFirst())
    guard let text = rest.first else { fputs("用法: qantrip-debug paste <文本> [expand]\n", stderr); exit(2) }
    center.postNotificationName(.init("ai.nekro.qantrip.debug.insert"), object: nil,
                                userInfo: ["text": text, "expand": rest.dropFirst().first == "expand" ? "1" : "0"],
                                deliverImmediately: true)
case "status":
    center.postNotificationName(.init("ai.nekro.qantrip.debug.status"), object: nil, userInfo: nil,
                                deliverImmediately: true)
default:
    fputs("用法: qantrip-debug toggle | insert <输入> | paste <文本> [expand] | status\n", stderr)
    exit(2)
}
// 给通知投递留出时间
RunLoop.current.run(until: Date().addingTimeInterval(0.1))
