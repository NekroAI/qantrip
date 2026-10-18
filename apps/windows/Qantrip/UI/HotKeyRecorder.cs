using Microsoft.UI.Input;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Qantrip.Platform;
using Windows.System;
using Windows.UI.Core;
using static Qantrip.Platform.Win32;

namespace Qantrip.UI;

/// <summary>
/// 唤起快捷键录制控件（设置窗口与首次启动引导共用）：点击后按下新组合键，注册成功才保存；
/// 被占用时提示并保留原快捷键。录制期间暂停全局热键。
/// </summary>
internal sealed class HotKeyRecorder
{
    private readonly App app;
    private readonly Button recorder;
    private readonly TextBlock recorderText;
    private readonly TextBlock message;
    private readonly HyperlinkButton restore;
    private bool recording;
    /// 录制完成后吞掉随后的按键抬起（否则空格 / 回车抬起会再次触发按钮）
    private bool swallowKeyUp;

    public FrameworkElement View { get; }
    /// 快捷键已更改（新的快捷键文本）
    public event Action<string>? Changed;

    public HotKeyRecorder(App app, bool alignRight = true)
    {
        this.app = app;
        recorderText = new TextBlock { FontFamily = Theme.Mono, FontWeight = FontWeights.SemiBold, FontSize = 13 };
        recorder = new Button { Content = recorderText, MinWidth = 140 };
        AutomationProperties.SetAutomationId(recorder, "hotkeyRecorder");
        recorder.Click += (_, _) => { if (recording) StopRecording(); else StartRecording(); };
        recorder.PreviewKeyDown += OnKeyDown;
        recorder.PreviewKeyUp += (_, e) => { if (recording || swallowKeyUp) { e.Handled = true; if (!IsModifier(e.Key)) swallowKeyUp = false; } };
        recorder.LostFocus += (_, _) => StopRecording();

        restore = new HyperlinkButton { Content = Loc.T("恢复默认", "Restore default"), Padding = new Thickness(6, 2, 6, 2) };
        AutomationProperties.SetAutomationId(restore, "hotkeyRestore");
        restore.Click += (_, _) =>
        {
            var (m, k) = HotKeySpec.Parse(Settings.DefaultHotKey);
            Apply(m, k);
        };

        var align = alignRight ? HorizontalAlignment.Right : HorizontalAlignment.Center;
        message = new TextBlock { FontSize = 12, TextWrapping = TextWrapping.Wrap, MaxWidth = 300, TextAlignment = alignRight ? TextAlignment.Right : TextAlignment.Center, HorizontalAlignment = align };
        AutomationProperties.SetAutomationId(message, "hotkeyMessage");
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, HorizontalAlignment = align };
        row.Children.Add(restore);
        row.Children.Add(recorder);
        var sp = new StackPanel { Spacing = 4 };
        sp.Children.Add(row);
        sp.Children.Add(message);
        View = sp;
        Update();
        if (!app.HotKeyRegistered)
            ShowMessage(Loc.T($"当前快捷键 {Settings.HotKey} 注册失败（错误 {app.HotKeyError}），可能已被其他程序占用，请换一个组合",
                $"Could not register {Settings.HotKey} (error {app.HotKeyError}). It may be in use by another app; choose another shortcut"), error: true);
    }

    /// 窗口关闭时调用：录制中则恢复全局热键。
    public void Cancel()
    {
        if (recording) { recording = false; app.ResumeHotKey(); }
    }

    private void Update()
    {
        recorderText.Text = recording ? Loc.T("按下新的组合键…", "Press new shortcut…") : Settings.HotKey;
        restore.Visibility = !recording && !string.Equals(Settings.HotKey, Settings.DefaultHotKey, StringComparison.OrdinalIgnoreCase) ? Visibility.Visible : Visibility.Collapsed;
    }

    private void ShowMessage(string text, bool error)
    {
        message.Text = text;
        message.Foreground = error ? new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xC4, 0x2B, 0x1C)) : (Brush)Application.Current.Resources["TextFillColorSecondaryBrush"];
        if (error && Application.Current.RequestedTheme == ApplicationTheme.Dark) message.Foreground = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xFF, 0x99, 0xA4));
    }

    private void StartRecording()
    {
        recording = true;
        app.SuspendHotKey(); // 否则按下当前组合会唤起面板，录制收不到
        Update();
        ShowMessage(Loc.T("按 Esc 取消", "Press Esc to cancel"), error: false);
    }

    private void StopRecording()
    {
        if (!recording) return;
        recording = false;
        app.ResumeHotKey();
        Update();
        message.Text = "";
    }

    private static bool IsModifier(VirtualKey k) => k is VirtualKey.Control or VirtualKey.LeftControl or VirtualKey.RightControl
        or VirtualKey.Menu or VirtualKey.LeftMenu or VirtualKey.RightMenu or VirtualKey.Shift or VirtualKey.LeftShift or VirtualKey.RightShift
        or VirtualKey.LeftWindows or VirtualKey.RightWindows;

    private static bool Down(VirtualKey k) => InputKeyboardSource.GetKeyStateForCurrentThread(k).HasFlag(CoreVirtualKeyStates.Down);

    private void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (!recording) return;
        e.Handled = true;
        if (e.Key == VirtualKey.Escape) { StopRecording(); swallowKeyUp = true; return; }
        if (IsModifier(e.Key)) return; // 等待主键
        uint vk = (uint)e.Key;
        uint mods = 0;
        if (Down(VirtualKey.Control)) mods |= MOD_CONTROL;
        if (Down(VirtualKey.Menu)) mods |= MOD_ALT;
        if (Down(VirtualKey.Shift)) mods |= MOD_SHIFT;
        if (Down(VirtualKey.LeftWindows) || Down(VirtualKey.RightWindows)) mods |= MOD_WIN;
        if (!HotKeySpec.IsSupportedKey(vk))
        {
            ShowMessage(Loc.T("请使用字母、数字、空格或 F1–F24", "Use a letter, digit, Space or F1–F24"), error: true);
            return;
        }
        bool function = vk is >= 0x70 and <= 0x87;
        if (mods == 0 && !function)
        {
            ShowMessage(Loc.T("需要包含至少一个修饰键（Ctrl / Alt / Shift / Win）", "Include at least one modifier key (Ctrl / Alt / Shift / Win)"), error: true);
            return;
        }
        swallowKeyUp = true;
        Apply(mods, vk);
    }

    private void Apply(uint mods, uint vk)
    {
        recording = false;
        var spec = HotKeySpec.Format(mods, vk);
        if (app.TrySetHotKey(mods, vk, out int error))
        {
            var msg = Loc.T($"已设置为 {spec}", $"Set to {spec}");
            ShowMessage(msg, error: false);
            Log.Event("setting", new() { ["key"] = "hotKey", ["value"] = spec });
            Changed?.Invoke(spec);
        }
        else
        {
            ShowMessage(Loc.T($"{spec} 已被其他程序占用（错误 {error}），请换一个组合", $"{spec} is in use by another app (error {error}). Choose another shortcut"), error: true);
            Log.Event("setting", new() { ["key"] = "hotKey", ["value"] = spec, ["conflict"] = error });
        }
        Update();
    }
}
