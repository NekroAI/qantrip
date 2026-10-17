using System.Numerics;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Qantrip.Platform;
using Windows.Graphics;
using static Qantrip.Platform.Win32;

namespace Qantrip.UI;

/// <summary>
/// 简单的提示浮层（对应 macOS LauncherPanel.swift 的 HUD）：屏幕下方居中，不抢焦点，2 秒后消失。
/// 用于指令完成的提示；不经过系统通知（勿扰模式下也能看到）。
/// </summary>
internal sealed class Hud : Window
{
    private readonly TextBlock text = new() { FontSize = 14, FontWeight = Microsoft.UI.Text.FontWeights.Medium, TextTrimming = TextTrimming.CharacterEllipsis };
    private readonly Grid content = new();
    private readonly IntPtr hwnd;
    private DispatcherQueueTimer? timer;

    public Hud()
    {
        hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        var p = OverlappedPresenter.Create();
        p.IsAlwaysOnTop = true;
        p.IsResizable = false;
        p.IsMaximizable = false;
        p.IsMinimizable = false;
        p.SetBorderAndTitleBar(true, false);
        AppWindow.SetPresenter(p);
        AppWindow.IsShownInSwitchers = false;
        var ex = GetWindowLongPtr(hwnd, GWL_EXSTYLE).ToInt64();
        const long WS_EX_NOACTIVATE = 0x08000000;
        SetWindowLongPtr(hwnd, GWL_EXSTYLE, new IntPtr((ex | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) & ~WS_EX_APPWINDOW));
        int round = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, ref round, sizeof(int));
        SystemBackdrop = new DesktopAcrylicBackdrop();
        text.HorizontalAlignment = HorizontalAlignment.Center;
        text.VerticalAlignment = VerticalAlignment.Center;
        content.Children.Add(text);
        content.Padding = new Thickness(20, 0, 20, 0);
        Content = content;
        content.ActualThemeChanged += (_, _) => ApplyColors();
        ApplyColors();
    }

    private void ApplyColors()
    {
        var theme = new Theme(content.ActualTheme == ElementTheme.Dark || Application.Current.RequestedTheme == ApplicationTheme.Dark);
        content.Background = Theme.B(theme.Base);
        text.Foreground = Theme.B(theme.Text);
    }

    /// 估算文字宽度（DIP）：全角字符约一个字号宽，其余约 0.55 个字号。
    private static double Estimate(string s) => s.Sum(c => c > 0x2E80 ? 15.0 : 8.0);

    public void Show(string message)
    {
        text.Text = message;
        GetCursorPos(out var pt);
        var mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        var mi = new MONITORINFO { cbSize = System.Runtime.InteropServices.Marshal.SizeOf<MONITORINFO>() };
        GetMonitorInfo(mon, ref mi);
        GetDpiForMonitor(mon, 0, out uint dpi, out _);
        double scale = dpi == 0 ? 1 : dpi / 96.0;
        var wa = mi.rcWork;
        double maxW = (wa.Right - wa.Left) / scale * 0.8;
        int w = (int)Math.Round(Math.Min(Estimate(message) + 48, maxW) * scale), h = (int)Math.Round(44 * scale);
        int x = wa.Left + (wa.Right - wa.Left - w) / 2;
        int y = (int)(wa.Bottom - (wa.Bottom - wa.Top) * 0.2 - h / 2.0);
        AppWindow.MoveAndResize(new RectInt32(x, y, w, h));
        AppWindow.Show(false);
        Anim.FadeSlideIn(text, new Vector3(0, 6, 0), 180);
        timer?.Stop();
        timer = DispatcherQueue.CreateTimer();
        timer.Interval = TimeSpan.FromSeconds(2);
        timer.IsRepeating = false;
        timer.Tick += (_, _) => AppWindow.Hide();
        timer.Start();
        if (Settings.DebugHooks) Log.Event("hud", new() { ["message"] = message });
    }
}
