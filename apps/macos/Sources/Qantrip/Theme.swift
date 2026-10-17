import AppKit
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

    static let commandFont = NSFont.monospacedSystemFont(ofSize: 20, weight: .regular)
    static let commandFontBold = NSFont.monospacedSystemFont(ofSize: 20, weight: .semibold)

    static let panelSize = NSSize(width: 840, height: 500)
}

extension Color {
    static let qAccent = Color(nsColor: Theme.accent)
    static let qOption = Color(nsColor: Theme.option)
    static let qValue = Color(nsColor: Theme.value)
}
