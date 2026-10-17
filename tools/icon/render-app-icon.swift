// 应用图标生成脚本（方案 A「魔杖 Q」，docs/decisions.md D21）。
//
// 依赖字体 Fraunces 斜体变量字体（Google Fonts，SIL OFL 1.1）：
//   curl -LO "https://raw.githubusercontent.com/google/fonts/main/ofl/fraunces/Fraunces-Italic%5BSOFT,WONK,opsz,wght%5D.ttf"
// 用法：
//   swiftc -O render-app-icon.swift -o render-app-icon
//   ./render-app-icon <Fraunces-Italic 字体路径> AppIcon-1024.png
// 再按 README 中的步骤生成 AppIcon.icns。
import AppKit
import CoreText

let fontPath = CommandLine.arguments[1], out = CommandLine.arguments[2]
CTFontManagerRegisterFontsForURL(URL(fileURLWithPath: fontPath) as CFURL, .process, nil)
func vfont(_ name: String, _ size: CGFloat, _ axes: [String: CGFloat]) -> NSFont {
    let base = NSFont(name: name, size: size)!
    var v: [NSNumber: CGFloat] = [:]
    for (t, x) in axes { v[NSNumber(value: t.unicodeScalars.reduce(0) { ($0 << 8) | Int($1.value) })] = x }
    let key = NSFontDescriptor.AttributeName(rawValue: kCTFontVariationAttribute as String)
    return NSFont(descriptor: base.fontDescriptor.addingAttributes([key: v]), size: size)!
}
func c(_ hex: UInt32, _ a: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: CGFloat((hex >> 16) & 255) / 255, green: CGFloat((hex >> 8) & 255) / 255, blue: CGFloat(hex & 255) / 255, alpha: a)
}
// macOS 图标网格：1024 画布，主体 824×824，圆角约 185
let body = NSRect(x: 100, y: 100, width: 824, height: 824)
func squircle() -> NSBezierPath { NSBezierPath(roundedRect: body, xRadius: 185, yRadius: 185) }

func sparkle(_ center: NSPoint, _ r: CGFloat, _ color: NSColor) {
    let p = NSBezierPath()
    let k: CGFloat = 0.22
    p.move(to: NSPoint(x: center.x, y: center.y + r))
    p.curve(to: NSPoint(x: center.x + r, y: center.y), controlPoint1: NSPoint(x: center.x + r * k, y: center.y + r * k), controlPoint2: NSPoint(x: center.x + r * k, y: center.y + r * k))
    p.curve(to: NSPoint(x: center.x, y: center.y - r), controlPoint1: NSPoint(x: center.x + r * k, y: center.y - r * k), controlPoint2: NSPoint(x: center.x + r * k, y: center.y - r * k))
    p.curve(to: NSPoint(x: center.x - r, y: center.y), controlPoint1: NSPoint(x: center.x - r * k, y: center.y - r * k), controlPoint2: NSPoint(x: center.x - r * k, y: center.y - r * k))
    p.curve(to: NSPoint(x: center.x, y: center.y + r), controlPoint1: NSPoint(x: center.x - r * k, y: center.y + r * k), controlPoint2: NSPoint(x: center.x - r * k, y: center.y + r * k))
    color.setFill(); p.fill()
}

func shadowed(_ blur: CGFloat, _ color: NSColor, _ draw: () -> Void) {
    NSGraphicsContext.saveGraphicsState()
    let s = NSShadow(); s.shadowBlurRadius = blur; s.shadowColor = color; s.shadowOffset = .zero; s.set()
    draw()
    NSGraphicsContext.restoreGraphicsState()
}

func drawText(_ str: String, _ font: NSFont, _ color: NSColor, center: NSPoint) {
    let a = NSAttributedString(string: str, attributes: [.font: font, .foregroundColor: color])
    let line = CTLineCreateWithAttributedString(a)
    let b = CTLineGetBoundsWithOptions(line, .useGlyphPathBounds)
    let ctx = NSGraphicsContext.current!.cgContext
    ctx.textPosition = CGPoint(x: center.x - b.midX, y: center.y - b.midY)
    CTLineDraw(line, ctx)
}

// A：紫色渐变 + Fraunces 斜体 Q，尾巴化作魔杖，杖尖星光
func iconA() {
    squircle().addClip()
    NSGradient(colors: [c(0x8B5CF6), c(0x5B21B6), c(0x2E1065)])!.draw(in: body, angle: -60)
    NSGradient(colors: [c(0xFFFFFF, 0.25), c(0xFFFFFF, 0)])!.draw(in: NSRect(x: 100, y: 600, width: 824, height: 324), angle: -90)
    shadowed(40, c(0x1E0B4B, 0.6)) {
        drawText("Q", vfont("Fraunces-Italic", 600, ["wght": 700, "SOFT": 100, "WONK": 1, "opsz": 144]), .white, center: NSPoint(x: 492, y: 540))
    }
    // 魔杖：从 Q 的尾部斜向右下延伸
    let wand = NSBezierPath()
    wand.move(to: NSPoint(x: 600, y: 330)); wand.line(to: NSPoint(x: 790, y: 190))
    wand.lineWidth = 34; wand.lineCapStyle = .round
    c(0xFDE68A).setStroke(); wand.stroke()
    shadowed(30, c(0xFDE68A, 0.9)) { sparkle(NSPoint(x: 735, y: 760), 70, c(0xFEF3C7)) }
    shadowed(16, c(0xFDE68A, 0.8)) { sparkle(NSPoint(x: 820, y: 660), 34, c(0xFEF3C7)) }
}


func render(_ draw: @escaping () -> Void) -> NSImage {
    NSImage(size: NSSize(width: 1024, height: 1024), flipped: false) { _ in
        shadowed(30, NSColor(white: 0, alpha: 0.35)) { NSColor.black.setFill(); squircle().fill() }
        draw(); return true
    }
}

let rep = NSBitmapImageRep(data: render(iconA).tiffRepresentation!)!
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
