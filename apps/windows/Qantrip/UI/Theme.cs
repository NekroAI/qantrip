using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.Foundation;
using Windows.UI;

namespace Qantrip.UI;

/// 视觉常量（与 macOS Theme.swift 相同的配色）。
internal sealed class Theme
{
    public bool Dark { get; }
    public Theme(bool dark) => Dark = dark;

    private static Color Hex(uint v, byte a = 255) => Color.FromArgb(a, (byte)(v >> 16), (byte)(v >> 8), (byte)v);

    public Color Accent => Dark ? Hex(0xA78BFA) : Hex(0x7C3AED);
    public Color Option => Dark ? Hex(0x5EEAD4) : Hex(0x0F766E);
    public Color Value => Dark ? Hex(0xFBBF24) : Hex(0xB45309);
    public Color Error => Dark ? Hex(0xFF7B72) : Hex(0xC42B1C);
    public Color Warning => Dark ? Hex(0xF0A030) : Hex(0xB86000);
    public Color Text => Dark ? Hex(0xF2F2F2) : Hex(0x1A1A1A);
    public Color Secondary => Dark ? Hex(0xFFFFFF, 0xA8) : Hex(0x000000, 0x99);
    public Color Tertiary => Dark ? Hex(0xFFFFFF, 0x6A) : Hex(0x000000, 0x66);
    public Color Divider => Dark ? Hex(0xFFFFFF, 0x18) : Hex(0x000000, 0x14);
    public Color Hover => Dark ? Hex(0xFFFFFF, 0x0F) : Hex(0x000000, 0x0A);
    public Color Fill => Dark ? Hex(0xFFFFFF, 0x10) : Hex(0x000000, 0x08);
    public Color Base => Dark ? Hex(0x1C1C1E, 0xA0) : Hex(0xF5F5F7, 0xA8);
    public Color Panel => Dark ? Hex(0x202020, 0x60) : Hex(0xF8F8F8, 0x50);

    public static Color WithAlpha(Color c, double a) => Color.FromArgb((byte)Math.Clamp(a * 255, 0, 255), c.R, c.G, c.B);
    public static SolidColorBrush B(Color c) => new(c);

    // 字体（D17）：命令行与代码元素用 Monaspace Neon，展示性大字用 Fraunces，正文与界面小字用系统字体。
    // 两款字体随应用分发（Assets/Fonts，SIL OFL 1.1）；Fraunces 为固定 SOFT=100 / WONK=1 / wght=600 / opsz=24 的静态实例
    // （XAML 无法设置自定义变量轴，取值与 macOS Theme.display 一致），中文等字形由系统字体回退。
    public static readonly FontFamily Mono = new("ms-appx:///Assets/Fonts/MonaspaceNeonVar.ttf#Monaspace Neon Var, Cascadia Mono, Consolas, Microsoft YaHei UI");
    public static readonly FontFamily Ui = new("Segoe UI Variable Text, Segoe UI, Microsoft YaHei UI");
    public static readonly FontFamily Display = new("ms-appx:///Assets/Fonts/FrauncesDisplay.ttf#Fraunces Display, Segoe UI Variable Display, Microsoft YaHei UI");
    public static readonly FontFamily Icons = new("Segoe Fluent Icons, Segoe MDL2 Assets");

    public const double CommandSize = 19;
    public const double PanelWidth = 840, PanelHeight = 500;
}

/// 自动换行的横向排列（参数标签）。
internal sealed class WrapPanel : Panel
{
    public double Spacing { get; set; } = 6;

    protected override Size MeasureOverride(Size available)
    {
        double x = 0, y = 0, rowH = 0, maxW = 0;
        foreach (var c in Children)
        {
            c.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
            var s = c.DesiredSize;
            if (x > 0 && x + s.Width > available.Width) { y += rowH + Spacing; x = 0; rowH = 0; }
            x += s.Width + Spacing;
            rowH = Math.Max(rowH, s.Height);
            maxW = Math.Max(maxW, x - Spacing);
        }
        return new Size(double.IsInfinity(available.Width) ? maxW : available.Width, y + rowH);
    }

    protected override Size ArrangeOverride(Size final)
    {
        double x = 0, y = 0, rowH = 0;
        foreach (var c in Children)
        {
            var s = c.DesiredSize;
            if (x > 0 && x + s.Width > final.Width) { y += rowH + Spacing; x = 0; rowH = 0; }
            c.Arrange(new Rect(x, y, s.Width, s.Height));
            x += s.Width + Spacing;
            rowH = Math.Max(rowH, s.Height);
        }
        return final;
    }
}
