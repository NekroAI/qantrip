import AppKit
import CoreText
import SwiftUI

/// 视觉常量。品牌色取紫色（"戏法"），语法高亮各类 token 使用固定配色。
enum Theme {
    static func dynamic(light: NSColor, dark: NSColor) -> NSColor {
        NSColor(name: nil) { $0.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light }
    }

    static func hex(_ v: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((v >> 16) & 0xFF) / 255, green: CGFloat((v >> 8) & 0xFF) / 255,
                blue: CGFloat(v & 0xFF) / 255, alpha: 1)
    }

    static let accent = dynamic(light: hex(0x7C3AED), dark: hex(0xA78BFA))
    static let option = dynamic(light: hex(0x0F766E), dark: hex(0x5EEAD4))
    static let value = dynamic(light: hex(0xB45309), dark: hex(0xFBBF24))
    static let extra = NSColor.labelColor
    static let error = NSColor.systemRed
    static let warning = NSColor.systemOrange

    // MARK: 字体（docs/decisions.md D17）
    // 命令行与代码元素：Monaspace Neon；展示性大字：Fraunces；正文与界面小字：系统字体。

    /// 注册应用内置字体（仅本进程可见）。需在使用 Theme 字体前调用。
    static func registerFonts() {
        guard let dir = Bundle.main.resourceURL?.appendingPathComponent("Fonts"),
              let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else { return }
        for url in files where url.pathExtension == "ttf" {
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
    }

    /// 按变量字体轴取字体；内置字体缺失时退回系统字体。
    private static func variable(_ name: String, _ size: CGFloat, _ axes: [String: CGFloat], fallback: NSFont) -> NSFont {
        guard let base = NSFont(name: name, size: size) else { return fallback }
        var variations: [NSNumber: CGFloat] = [:]
        for (tag, v) in axes {
            variations[NSNumber(value: tag.unicodeScalars.reduce(0) { ($0 << 8) | Int($1.value) })] = v
        }
        let key = NSFontDescriptor.AttributeName(rawValue: kCTFontVariationAttribute as String)
        return NSFont(descriptor: base.fontDescriptor.addingAttributes([key: variations]), size: size) ?? base
    }

    static func mono(_ size: CGFloat, weight: CGFloat = 400) -> NSFont {
        variable("MonaspaceNeonVar-Regular", size, ["wght": weight],
                 fallback: .monospacedSystemFont(ofSize: size, weight: weight >= 600 ? .semibold : .regular))
    }

    static func display(_ size: CGFloat, weight: CGFloat = 600) -> NSFont {
        variable("Fraunces", size, ["wght": weight, "SOFT": 100, "WONK": 1, "opsz": min(max(size, 9), 144)],
                 fallback: .systemFont(ofSize: size, weight: .semibold))
    }

    static let commandFont = mono(19)
    static let commandFontBold = mono(19, weight: 700)

    static let panelSize = NSSize(width: 840, height: 500)
}

extension Font {
    static func qMono(_ size: CGFloat, weight: CGFloat = 400) -> Font { Font(Theme.mono(size, weight: weight)) }
    static func qDisplay(_ size: CGFloat, weight: CGFloat = 600) -> Font { Font(Theme.display(size, weight: weight)) }
}

extension Color {
    static let qAccent = Color(nsColor: Theme.accent)
    static let qOption = Color(nsColor: Theme.option)
    static let qValue = Color(nsColor: Theme.value)
}
