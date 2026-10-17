// 测试辅助：剪贴板快照与多格式写入。
//   pbtool dump        打印每个条目的类型与内容哈希
//   pbtool set-rich S  写入 string + html + 自定义类型
import AppKit
import CryptoKit
let pb = NSPasteboard.general
let args = CommandLine.arguments
switch args[1] {
case "dump":
    for (i, item) in (pb.pasteboardItems ?? []).enumerated() {
        for t in item.types.map(\.rawValue).sorted() {
            let d = item.data(forType: .init(t)) ?? Data()
            let h = SHA256.hash(data: d).prefix(6).map { String(format: "%02x", $0) }.joined()
            print("\(i) \(t) \(d.count) \(h)")
        }
    }
case "set-rich":
    pb.clearContents()
    let item = NSPasteboardItem()
    item.setString(args[2], forType: .string)
    item.setString("<b>\(args[2])</b>", forType: .html)
    item.setData(Data([0, 1, 2, 255]), forType: .init("ai.nekro.test.custom"))
    pb.writeObjects([item])
default: exit(2)
}
