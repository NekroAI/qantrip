using System.Diagnostics;
using System.Reflection;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Input;
using Microsoft.UI.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Qantrip.Core;
using Qantrip.Platform;
using Windows.Graphics;
using Windows.System;
using Windows.UI.Core;
using static Qantrip.Platform.Win32;

namespace Qantrip.UI;

/// <summary>
/// 设置窗口（对应 macOS SettingsView.swift）：Fluent 设置卡片 + Mica 背景，从托盘菜单打开（或 `Qantrip.exe --settings`）。
/// 修改立即保存到 settings.json 并即时生效；界面文字跟随界面语言，切换语言时整页重建。
/// 控件带 AutomationId，回归脚本通过 UI 自动化操作（tests/windows/t-settings.ps1）。
/// </summary>
internal sealed class SettingsWindow : Window
{
    private readonly App app;
    private readonly IntPtr hwnd;
    private readonly Grid root = new();
    private ScrollViewer? scroll;

    // 快捷键录制
    private Button? recorder;
    private TextBlock? recorderText;
    private TextBlock? recorderMessage;
    private bool recording;
    /// 录制完成后吞掉随后的按键抬起（否则空格 / 回车抬起会再次触发按钮）
    private bool swallowKeyUp;

    public SettingsWindow(App app)
    {
        this.app = app;
        hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        ExtendsContentIntoTitleBar = true;
        if (MicaController.IsSupported()) SystemBackdrop = new MicaBackdrop();
        else root.Background = Res("SolidBackgroundFillColorBaseBrush");
        var p = OverlappedPresenter.Create();
        p.IsMaximizable = false;
        AppWindow.SetPresenter(p);
        var icon = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        if (File.Exists(icon)) AppWindow.SetIcon(icon);
        Place();
        Content = root;
        Closed += (_, _) => { if (recording) app.ResumeHotKey(); };
        Build();
    }

    private static Brush Res(string key) => (Brush)Application.Current.Resources[key];

    /// 在鼠标所在显示器居中，按该显示器的缩放计算尺寸。
    private void Place()
    {
        GetCursorPos(out var pt);
        var mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        var mi = new MONITORINFO { cbSize = System.Runtime.InteropServices.Marshal.SizeOf<MONITORINFO>() };
        GetMonitorInfo(mon, ref mi);
        GetDpiForMonitor(mon, 0, out uint dpi, out _);
        double scale = dpi == 0 ? 1 : dpi / 96.0;
        var wa = mi.rcWork;
        int w = (int)Math.Round(720 * scale), h = (int)Math.Min(Math.Round(820 * scale), (wa.Bottom - wa.Top) * 0.9);
        AppWindow.MoveAndResize(new RectInt32(wa.Left + (wa.Right - wa.Left - w) / 2, wa.Top + (wa.Bottom - wa.Top - h) / 2, w, h));
    }

    public void ShowAndActivate()
    {
        AppWindow.Show(true);
        Activate();
        Foreground.Activate(hwnd, out _);
    }

    /// 语言变化后重建（推迟到当前事件处理结束，避免在下拉框的事件中移除它自己）。
    public void Rebuild() => DispatcherQueue.TryEnqueue(() =>
    {
        var offset = scroll?.VerticalOffset ?? 0;
        Build();
        scroll?.UpdateLayout();
        scroll?.ChangeView(null, offset, null, true);
    });

    // MARK: 构建

    private void Build()
    {
        Title = Loc.T("Nekro Qantrip 设置", "Nekro Qantrip Settings");
        root.Children.Clear();
        root.RowDefinitions.Clear();
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(40) });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        // 自定义标题栏（内容延伸到标题栏，Mica 贯穿整个窗口）
        var titleBar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10, Padding = new Thickness(16, 0, 0, 0), VerticalAlignment = VerticalAlignment.Center };
        var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        if (File.Exists(iconPath)) titleBar.Children.Add(new Image { Source = new BitmapImage(new Uri(iconPath)), Width = 16, Height = 16 });
        titleBar.Children.Add(new TextBlock { Text = Title, FontSize = 12, VerticalAlignment = VerticalAlignment.Center });
        var titleHost = new Grid { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
        titleHost.Children.Add(titleBar);
        root.Children.Add(titleHost);
        SetTitleBar(titleHost);

        var page = new StackPanel { Spacing = 4, MaxWidth = 860, Padding = new Thickness(28, 8, 28, 28) };
        page.Children.Add(new TextBlock { Text = Loc.T("设置", "Settings"), FontFamily = Theme.Display, FontSize = 30, Margin = new Thickness(0, 0, 0, 12) });

        // 通用
        page.Children.Add(Section(Loc.T("通用", "General")));
        page.Children.Add(Card("", Loc.T("唤起快捷键", "Panel shortcut"),
            Loc.T("在任意程序中按下即可唤起命令面板。默认 Alt+Q。", "Press it in any app to open the command panel. The default is Alt+Q."), HotKeyRecorder()));
        page.Children.Add(Card("", Loc.T("登录时启动", "Launch at login"), AutostartDescription(), AutostartToggle()));
        page.Children.Add(Card("", Loc.T("界面语言", "Language"), null,
            LanguageBox("uiLanguage", Loc.T("跟随系统", "System default"), Settings.Language, v => Settings.Language = v)));
        page.Children.Add(Card("", Loc.T("内置模板语言", "Built-in templates"), Loc.T("你自己的模板不受影响。", "Your own templates are not affected."),
            LanguageBox("templateLanguage", Loc.T("跟随界面语言", "Same as language"), Settings.TemplateLanguage, v => Settings.TemplateLanguage = v)));

        // 插入
        page.Children.Add(Section(Loc.T("插入", "Insert")));
        page.Children.Add(Card("", Loc.T("插入方式", "Insert method"),
            Loc.T("自动粘贴会把 Prompt 放到剪贴板并模拟 Ctrl+V，随后恢复原剪贴板；Prompt 不进入剪贴板历史。目标程序以管理员身份运行时自动改为只复制。",
                "Auto-paste puts the prompt on the clipboard, simulates Ctrl+V and then restores the clipboard; the prompt stays out of clipboard history. Falls back to copy only when the target app runs as administrator."),
            Combo("insertMode", new[] { Loc.T("自动粘贴到当前程序", "Auto-paste into the current app"), Loc.T("只复制，手动粘贴", "Copy only, paste manually") },
                Settings.CopyOnly ? 1 : 0, i => Settings.CopyOnly = i == 1)));
        var interval = IntervalSlider();
        page.Children.Add(Card("", Loc.T("Ctrl+↩ 展开粘贴（Claude Code）", "Ctrl+↩ expand paste (Claude Code)"),
            Loc.T("Claude Code 会把超过 800 字符或多于 3 行的粘贴折叠为 [Pasted text]，再次粘贴相同内容即可展开。Ctrl+↩ 只在内容会被折叠时才粘贴两次。",
                "Claude Code collapses pastes over 800 characters or more than 3 lines into [Pasted text]; pasting the same content again expands it. Ctrl+↩ pastes twice only when the content would be collapsed."),
            Toggle("expandPaste", Settings.ExpandPasteEnabled, v => { Settings.ExpandPasteEnabled = v; interval.IsEnabled = v; })));
        interval.IsEnabled = Settings.ExpandPasteEnabled;
        page.Children.Add(Card("", Loc.T("两次粘贴间隔", "Interval between pastes"), null, interval));

        // 输入法
        page.Children.Add(Section(Loc.T("输入法", "Input method")));
        ToggleSwitch? shift = null;
        page.Children.Add(Card("", Loc.T("唤起时使用英文输入", "Use English input when opened"),
            Loc.T("中文输入法下唤起面板时默认英文，命令词不会进入拼音组字。输入中文附加要求时按 Shift 切换即可。只影响面板，不改变其他程序的输入法。",
                "With a Chinese IME, the panel starts in English so commands don’t go into pinyin composition. Press Shift to type extra instructions in Chinese. Only affects the panel."),
            Toggle("forceAscii", Settings.ForceAsciiInput, v => { Settings.ForceAsciiInput = v; if (shift != null) shift.IsEnabled = v; })));
        shift = Toggle("imeShift", Settings.ImeShiftToggle, v => Settings.ImeShiftToggle = v);
        shift.IsEnabled = Settings.ForceAsciiInput;
        page.Children.Add(Card("", Loc.T("再次唤起仍是中文时自动切回英文（实验性）", "Switch back to English if still in Chinese mode (experimental)"),
            Loc.T("模拟按一次 Shift（微软拼音等输入法的中英切换键）。若你改过输入法的切换键，可以关闭此项。",
                "Simulates one Shift press (the Chinese/English toggle of Microsoft Pinyin and most IMEs). Turn it off if you changed your IME’s toggle key."),
            shift));

        // 隐私
        page.Children.Add(Section(Loc.T("隐私", "Privacy")));
        page.Children.Add(Card("", Loc.T("记录使用习惯", "Record usage"),
            Loc.T("用于首屏的「最近施放」和「常用」排序，以及按程序区分常用模板。只记录模板名和参数，不记录附加要求和生成的 Prompt，数据只保存在本机。",
                "Used for “Recent” and “Frequent” ordering on the start screen, and to tell which templates you use in which app. Only template names and parameters are recorded, never extra instructions or generated prompts. Data stays on this PC."),
            Toggle("recordUsage", Settings.RecordUsage, v => Settings.RecordUsage = v)));
        page.Children.Add(Card("", Loc.T("清除使用记录", "Clear usage history"), null, ClearUsageButton()));

        // 关于
        page.Children.Add(Section(Loc.T("关于", "About")));
        page.Children.Add(About());

        scroll = new ScrollViewer { Content = page, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
        Grid.SetRow(scroll, 1);
        root.Children.Add(scroll);
    }

    private static TextBlock Section(string title) => new()
    {
        Text = title, FontSize = 14, FontWeight = FontWeights.SemiBold, Margin = new Thickness(2, 18, 0, 6),
    };

    /// 设置卡片：图标 | 标题 + 说明 | 控件（Windows 设置的样式）。
    private static FrameworkElement Card(string glyph, string title, string? description, FrameworkElement control)
    {
        var grid = new Grid { ColumnSpacing = 16, MinHeight = 44 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.Children.Add(new FontIcon { Glyph = glyph, FontSize = 18, FontFamily = Theme.Icons, VerticalAlignment = VerticalAlignment.Center });
        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center, Spacing = 2 };
        text.Children.Add(new TextBlock { Text = title, TextWrapping = TextWrapping.Wrap });
        if (!string.IsNullOrEmpty(description))
            text.Children.Add(new TextBlock { Text = description, FontSize = 12, TextWrapping = TextWrapping.Wrap, Foreground = Res("TextFillColorSecondaryBrush") });
        Grid.SetColumn(text, 1);
        grid.Children.Add(text);
        control.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetColumn(control, 2);
        grid.Children.Add(control);
        AutomationProperties.SetName(control, title);
        return new Border
        {
            Child = grid, Padding = new Thickness(16, 12, 16, 12), CornerRadius = new CornerRadius(6), BorderThickness = new Thickness(1),
            Background = Res("CardBackgroundFillColorDefaultBrush"), BorderBrush = Res("CardStrokeColorDefaultBrush"),
        };
    }

    private static ToggleSwitch Toggle(string id, bool on, Action<bool> changed)
    {
        var t = new ToggleSwitch { IsOn = on, OnContent = Loc.T("开", "On"), OffContent = Loc.T("关", "Off"), MinWidth = 0 };
        AutomationProperties.SetAutomationId(t, id);
        t.Toggled += (_, _) => changed(t.IsOn);
        return t;
    }

    private static ComboBox Combo(string id, string[] items, int selected, Action<int> changed)
    {
        var c = new ComboBox { MinWidth = 200 };
        foreach (var i in items) c.Items.Add(i);
        c.SelectedIndex = selected;
        AutomationProperties.SetAutomationId(c, id);
        c.SelectionChanged += (_, _) => { if (c.SelectedIndex >= 0) changed(c.SelectedIndex); };
        return c;
    }

    private static readonly string[] LanguageCodes = { "auto", "zh", "en" };

    private static ComboBox LanguageBox(string id, string autoLabel, string current, Action<string> changed) =>
        Combo(id, new[] { autoLabel, "中文", "English" }, Math.Max(0, Array.IndexOf(LanguageCodes, current)), i => changed(LanguageCodes[i]));

    private Control IntervalSlider()
    {
        var label = new TextBlock { FontFamily = Theme.Mono, FontSize = 12, Width = 60, TextAlignment = TextAlignment.Right, VerticalAlignment = VerticalAlignment.Center };
        var s = new Slider { Minimum = 100, Maximum = 800, StepFrequency = 50, SnapsTo = Microsoft.UI.Xaml.Controls.Primitives.SliderSnapsTo.StepValues, Width = 180, Value = Settings.ExpandPasteIntervalMs };
        AutomationProperties.SetAutomationId(s, "expandInterval");
        label.Text = $"{(int)s.Value} ms";
        s.ValueChanged += (_, e) => { Settings.ExpandPasteIntervalMs = (int)e.NewValue; label.Text = $"{(int)e.NewValue} ms"; };
        var sp = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        sp.Children.Add(s);
        sp.Children.Add(label);
        // 用 ContentControl 包装，便于整体启用 / 禁用
        return new ContentControl { Content = sp, IsTabStop = false };
    }

    // MARK: 登录时启动

    private static string AutostartDescription() =>
        Autostart.PointsElsewhere
            ? Loc.T("当前登记的是另一个位置的 Qantrip（可能移动过程序目录），重新打开此项即可更正。", "The registered entry points to Qantrip in another folder (the app may have moved). Turn this on again to fix it.")
            : Loc.T("登录 Windows 后在后台启动，常驻通知区域。", "Starts in the background when you sign in to Windows and stays in the notification area.");

    private static ToggleSwitch AutostartToggle()
    {
        var t = Toggle("autostart", Autostart.IsEnabled && !Autostart.PointsElsewhere, v =>
        {
            Autostart.Set(v);
            Log.Event("setting", new() { ["key"] = "launchAtLogin", ["on"] = Autostart.IsEnabled });
        });
        return t;
    }

    private FrameworkElement ClearUsageButton()
    {
        var b = new Button { Content = Loc.T("清除", "Clear") };
        AutomationProperties.SetAutomationId(b, "clearUsage");
        b.Click += (_, _) =>
        {
            app.Engine?.ClearUsage();
            b.Content = Loc.T("已清除", "Cleared");
            b.IsEnabled = false;
            Log.Event("setting", new() { ["key"] = "clearUsage" });
        };
        return b;
    }

    // MARK: 快捷键录制

    private FrameworkElement HotKeyRecorder()
    {
        recording = false;
        recorderText = new TextBlock { FontFamily = Theme.Mono, FontWeight = FontWeights.SemiBold, FontSize = 13 };
        recorder = new Button { Content = recorderText, MinWidth = 140 };
        AutomationProperties.SetAutomationId(recorder, "hotkeyRecorder");
        recorder.Click += (_, _) => { if (recording) StopRecording(); else StartRecording(); };
        recorder.PreviewKeyDown += OnRecorderKeyDown;
        recorder.PreviewKeyUp += (_, e) => { if (recording || swallowKeyUp) { e.Handled = true; if (!IsModifier(e.Key)) swallowKeyUp = false; } };
        recorder.LostFocus += (_, _) => StopRecording();

        var restore = new HyperlinkButton { Content = Loc.T("恢复默认", "Restore default"), Padding = new Thickness(6, 2, 6, 2) };
        AutomationProperties.SetAutomationId(restore, "hotkeyRestore");
        restore.Click += (_, _) =>
        {
            var (m, k) = HotKeySpec.Parse(Settings.DefaultHotKey);
            Apply(m, k);
        };

        recorderMessage = new TextBlock { FontSize = 12, TextWrapping = TextWrapping.Wrap, MaxWidth = 280, TextAlignment = TextAlignment.Right, HorizontalAlignment = HorizontalAlignment.Right };
        AutomationProperties.SetAutomationId(recorderMessage, "hotkeyMessage");
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, HorizontalAlignment = HorizontalAlignment.Right };
        row.Children.Add(restore);
        row.Children.Add(recorder);
        var sp = new StackPanel { Spacing = 4 };
        sp.Children.Add(row);
        sp.Children.Add(recorderMessage);
        UpdateRecorder(restore);
        if (!app.HotKeyRegistered)
            ShowMessage(Loc.T($"当前快捷键 {Settings.HotKey} 注册失败（错误 {app.HotKeyError}），可能已被其他程序占用，请换一个组合",
                $"Could not register {Settings.HotKey} (error {app.HotKeyError}). It may be in use by another app; choose another shortcut"), error: true);
        return sp;
    }

    private HyperlinkButton? restoreButton;

    private void UpdateRecorder(HyperlinkButton? restore = null)
    {
        if (restore != null) restoreButton = restore;
        if (recorderText != null) recorderText.Text = recording ? Loc.T("按下新的组合键…", "Press new shortcut…") : Settings.HotKey;
        if (restoreButton != null)
            restoreButton.Visibility = !recording && !string.Equals(Settings.HotKey, Settings.DefaultHotKey, StringComparison.OrdinalIgnoreCase) ? Visibility.Visible : Visibility.Collapsed;
    }

    private void ShowMessage(string text, bool error)
    {
        if (recorderMessage is null) return;
        recorderMessage.Text = text;
        recorderMessage.Foreground = error ? new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xC4, 0x2B, 0x1C)) : Res("TextFillColorSecondaryBrush");
        if (error && Application.Current.RequestedTheme == ApplicationTheme.Dark) recorderMessage.Foreground = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xFF, 0x99, 0xA4));
    }

    private void StartRecording()
    {
        recording = true;
        app.SuspendHotKey(); // 否则按下当前组合会唤起面板，录制收不到
        UpdateRecorder();
        ShowMessage(Loc.T("按 Esc 取消", "Press Esc to cancel"), error: false);
    }

    private void StopRecording()
    {
        if (!recording) return;
        recording = false;
        app.ResumeHotKey();
        UpdateRecorder();
        if (recorderMessage != null) recorderMessage.Text = "";
    }

    private static bool IsModifier(VirtualKey k) => k is VirtualKey.Control or VirtualKey.LeftControl or VirtualKey.RightControl
        or VirtualKey.Menu or VirtualKey.LeftMenu or VirtualKey.RightMenu or VirtualKey.Shift or VirtualKey.LeftShift or VirtualKey.RightShift
        or VirtualKey.LeftWindows or VirtualKey.RightWindows;

    private static bool Down(VirtualKey k) => InputKeyboardSource.GetKeyStateForCurrentThread(k).HasFlag(CoreVirtualKeyStates.Down);

    private void OnRecorderKeyDown(object sender, KeyRoutedEventArgs e)
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
            // D5：Ctrl+Q 在浏览器和许多程序中是"退出"
            if (mods == MOD_CONTROL && vk == 'Q')
                msg += Loc.T("。注意：Ctrl+Q 在浏览器等许多程序中是“退出”，Qantrip 运行期间这些程序收不到它", ". Note: Ctrl+Q quits many apps such as browsers; they won’t receive it while Qantrip is running");
            ShowMessage(msg, error: false);
            Log.Event("setting", new() { ["key"] = "hotKey", ["value"] = spec });
        }
        else
        {
            ShowMessage(Loc.T($"{spec} 已被其他程序占用（错误 {error}），请换一个组合", $"{spec} is in use by another app (error {error}). Choose another shortcut"), error: true);
            Log.Event("setting", new() { ["key"] = "hotKey", ["value"] = spec, ["conflict"] = error });
        }
        UpdateRecorder();
    }

    // MARK: 关于

    private static string Version
    {
        get
        {
            var v = typeof(App).Assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion ?? "?";
            int plus = v.IndexOf('+');
            return plus > 0 ? v[..plus] : v;
        }
    }

    private static FrameworkElement About()
    {
        var sp = new StackPanel { Spacing = 8, HorizontalAlignment = HorizontalAlignment.Center, Padding = new Thickness(0, 8, 0, 8) };
        var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        if (File.Exists(iconPath))
            sp.Children.Add(new Image { Source = new BitmapImage(new Uri(iconPath)) { DecodePixelWidth = 128 }, Width = 64, Height = 64, HorizontalAlignment = HorizontalAlignment.Center });
        sp.Children.Add(new TextBlock { Text = "Nekro Qantrip", FontFamily = Theme.Display, FontSize = 26, HorizontalAlignment = HorizontalAlignment.Center });
        sp.Children.Add(new TextBlock { Text = Loc.T("按 Q 施放一个戏法", "Press Q to cast a spell"), Foreground = Res("TextFillColorSecondaryBrush"), HorizontalAlignment = HorizontalAlignment.Center });
        var ver = new TextBlock
        {
            Text = Loc.T($"版本 {Version} · core {Engine.CoreVersion}", $"Version {Version} · core {Engine.CoreVersion}"),
            FontFamily = Theme.Mono, FontSize = 11, Foreground = Res("TextFillColorSecondaryBrush"), HorizontalAlignment = HorizontalAlignment.Center,
        };
        AutomationProperties.SetAutomationId(ver, "version");
        sp.Children.Add(ver);
        var links = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Center };
        links.Children.Add(Link(Loc.T("官网", "Website"), "https://qantrip.nekro.ai"));
        links.Children.Add(Link("GitHub", "https://github.com/NekroAI/qantrip"));
        sp.Children.Add(links);
        sp.Children.Add(new TextBlock
        {
            Text = Loc.T("以 GPL-3.0-or-later 许可开源\n内置字体 Monaspace Neon、Fraunces 以 SIL OFL 1.1 许可分发",
                "Open source under GPL-3.0-or-later\nBundled fonts Monaspace Neon and Fraunces are distributed under SIL OFL 1.1"),
            FontSize = 12, TextAlignment = TextAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center, Foreground = Res("TextFillColorSecondaryBrush"),
        });
        return new Border
        {
            Child = sp, Padding = new Thickness(16), CornerRadius = new CornerRadius(6), BorderThickness = new Thickness(1),
            Background = Res("CardBackgroundFillColorDefaultBrush"), BorderBrush = Res("CardStrokeColorDefaultBrush"),
        };
    }

    private static HyperlinkButton Link(string text, string url)
    {
        var b = new HyperlinkButton { Content = text };
        ToolTipService.SetToolTip(b, url);
        b.Click += (_, _) =>
        {
            try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
            catch (Exception e) { Log.Error("open_link", e); }
        };
        return b;
    }
}
