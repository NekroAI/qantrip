import Foundation

// 自动化测试用：向 Qantrip 发送调试通知（需调试构建 QANTRIP_DEBUG_HOOKS=1，且开启 debugHooks）。
//   qantrip-debug toggle
//   qantrip-debug insert <面板输入>          按面板输入渲染后插入
//   qantrip-debug paste <文本> [expand]      直接插入文本；expand 时展开粘贴
//   qantrip-debug type <文本> [下移次数]     打开面板并填入输入（截图检查用）
//   qantrip-debug confirm                    模拟回车
//   qantrip-debug param <参数名> <操作>       模拟点击参数标签
//   qantrip-debug window <settings|library[:名称]|onboarding>
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
case "type":
    let rest = Array(args.dropFirst())
    center.postNotificationName(.init("ai.nekro.qantrip.debug.type"), object: nil,
                                userInfo: ["text": rest.first ?? "", "down": rest.dropFirst().first ?? "0"],
                                deliverImmediately: true)
case "confirm":
    center.postNotificationName(.init("ai.nekro.qantrip.debug.confirm"), object: nil, userInfo: nil, deliverImmediately: true)
case "param":
    let rest = Array(args.dropFirst())
    guard rest.count == 2 else { fputs("用法: qantrip-debug param <参数名> <toggle|set:v|choice:v|clear|begin>\n", stderr); exit(2) }
    center.postNotificationName(.init("ai.nekro.qantrip.debug.param"), object: nil,
                                userInfo: ["name": rest[0], "op": rest[1]], deliverImmediately: true)
case "window":
    center.postNotificationName(.init("ai.nekro.qantrip.debug.window"), object: nil,
                                userInfo: ["name": args.dropFirst().first ?? ""], deliverImmediately: true)
case "editor":
    center.postNotificationName(.init("ai.nekro.qantrip.debug.editor"), object: nil,
                                userInfo: ["op": args.dropFirst().first ?? ""], deliverImmediately: true)
case "status":
    center.postNotificationName(.init("ai.nekro.qantrip.debug.status"), object: nil, userInfo: nil,
                                deliverImmediately: true)
default:
    fputs("用法: qantrip-debug toggle | insert <输入> | paste <文本> [expand] | status\n", stderr)
    exit(2)
}
// 给通知投递留出时间
RunLoop.current.run(until: Date().addingTimeInterval(0.1))
