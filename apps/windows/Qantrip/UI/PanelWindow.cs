using Microsoft.UI;
using Microsoft.UI.Input;
using Microsoft.UI.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Qantrip.Core;
using Qantrip.Platform;
using Windows.Foundation;
using Windows.Graphics;
using Windows.System;
using Windows.UI;
using Windows.UI.Core;
using System.Numerics;
using static Qantrip.Platform.Win32;

namespace Qantrip.UI;

/// <summary>
/// 命令面板窗口：无边框、置顶、圆角、亚克力背景，不出现在任务栏与 Alt+Tab。
/// 界面布局与 macOS LauncherView 相同：顶栏命令行，左栏候选，右栏咒文卡（模板、参数、实时预览），底栏状态与按键提示。
/// 界面全部用代码构建（内容几乎都是动态的）。
/// </summary>
internal sealed class PanelWindow : Window
{
    public readonly LauncherModel Model;
    public IntPtr Hwnd { get; }

    private Theme theme = new(true);
    private readonly Grid root = new();
    /// 亚克力之上的一层半透明底色，保证背后内容杂乱时的可读性
    private readonly Grid tint = new();
    private readonly RichEditBox field = new();
    private readonly Grid fieldHost = new();
    private readonly TextBlock ghost = new();
    private readonly TextBlock wand = new();
    private readonly Border targetPill = new();
    private readonly TextBlock targetText = new();
    /// 顶栏右侧的快捷入口：模板库（Ctrl+L）、设置（Ctrl+,）
    private readonly StackPanel quick = new() { Orientation = Orientation.Horizontal, Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
    /// 输入框为空时的占位文字：和灰字补全一样画在光标所在行的基线上（输入框自带的占位文字与光标不在同一基线）
    private string placeholder = "";
    private readonly StackPanel list = new();
    private readonly ScrollViewer listScroll = new();
    private readonly TextBlock listEmpty = new();
    private readonly StackPanel card = new();
    private readonly TextBlock statusText = new();
    private readonly StackPanel keys = new() { Orientation = Orientation.Horizontal, Spacing = 14 };
    private readonly List<Border> dividers = new();
    private readonly List<FrameworkElement> rowElements = new();

    /// 输入框当前文本（模型已知的版本）
    private string fieldText = "";
    private bool composing;

    // 动效状态（D15）
    /// 下一次构建候选列表时播放错落入场（面板刚唤起）
    private bool entering;
    private string listKind = "";
    private HashSet<string> rowIds = new();
    private bool wasLocked;
    private string? previewTemplate;
    private HashSet<string> previewHighlights = new();
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? flashTimer;
    /// 底栏按键提示对应的状态（指令模式、展开粘贴、语言）；变化时重建
    private string keysState = "";

    public PanelWindow(LauncherModel model)
    {
        Model = model;
        Hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        Title = "Nekro Qantrip";
        ConfigureWindow();
        BuildUi();
        model.Changed += Refresh;
        model.SetField += SetFieldText;
        model.Rejected += FlashReject;
        root.ActualThemeChanged += (_, _) => { theme = new Theme(root.ActualTheme == ElementTheme.Dark); ApplyStaticColors(); Refresh(); };
    }

    // MARK: 窗口

    private void ConfigureWindow()
    {
        var aw = AppWindow;
        var p = OverlappedPresenter.Create();
        p.IsAlwaysOnTop = true;
        p.IsResizable = false;
        p.IsMaximizable = false;
        p.IsMinimizable = false;
        // 保留细边框：Windows 11 据此绘制圆角与阴影；不要标题栏
        p.SetBorderAndTitleBar(true, false);
        aw.SetPresenter(p);
        aw.IsShownInSwitchers = false;
        aw.Title = "Nekro Qantrip";
        var ex = GetWindowLongPtr(Hwnd, GWL_EXSTYLE).ToInt64();
        SetWindowLongPtr(Hwnd, GWL_EXSTYLE, new IntPtr((ex | WS_EX_TOOLWINDOW) & ~WS_EX_APPWINDOW));
        int round = DWMWCP_ROUND;
        DwmSetWindowAttribute(Hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, ref round, sizeof(int));
        SystemBackdrop = new DesktopAcrylicBackdrop();
        var icon = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        if (File.Exists(icon)) aw.SetIcon(icon);
    }

    public bool IsShown => AppWindow.IsVisible;

    /// 在目标窗口所在显示器上居中、靠上放置（物理像素，按该显示器的缩放计算尺寸）。
    public void PlaceOn(IntPtr targetHwnd)
    {
        IntPtr mon;
        if (targetHwnd != IntPtr.Zero) mon = MonitorFromWindow(targetHwnd, MONITOR_DEFAULTTONEAREST);
        else { GetCursorPos(out var pt); mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST); }
        var mi = new MONITORINFO { cbSize = System.Runtime.InteropServices.Marshal.SizeOf<MONITORINFO>() };
        GetMonitorInfo(mon, ref mi);
        GetDpiForMonitor(mon, 0, out uint dpi, out _);
        double scale = dpi == 0 ? 1 : dpi / 96.0;
        int w = (int)Math.Round(Theme.PanelWidth * scale), h = (int)Math.Round(Theme.PanelHeight * scale);
        var wa = mi.rcWork;
        int x = wa.Left + (wa.Right - wa.Left - w) / 2;
        int y = (int)(wa.Top + (wa.Bottom - wa.Top) * 0.32 - h / 2.0);
        y = Math.Max(y, wa.Top + (int)(24 * scale));
        AppWindow.MoveAndResize(new RectInt32(x, y, w, h));
    }

    /// 显示并激活，焦点放到输入框。返回是否成功成为前台窗口。
    /// 唤起前调用：下一次刷新时候选错落出现。
    public void PrepareEntrance() => entering = Motion.Enabled;

    public bool ShowActivated(out string method)
    {
        Anim.Entrance(root);
        AppWindow.Show(true);
        bool ok = Foreground.Activate(Hwnd, out method);
        FocusField();
        ImeAction = Settings.ForceAsciiInput ? EnsureAlphanumeric() : "off";
        return ok;
    }

    /// 最近一次唤起时的输入法处理（日志用）
    public string ImeAction { get; private set; } = "";

    /// <summary>
    /// D9：面板默认英文。InputScope=Url 让输入法在输入框第一次获得焦点时使用英文，但之后会记住用户在面板里切到的中文。
    /// 外部写 IMM32 / TSF 状态对 WinUI 输入框无效，因此读到仍是中文模式时，模拟一次 Shift（微软拼音等的默认中英切换键）。
    /// 必须等用户松开所有修饰键（否则 Alt+Shift 会切换系统输入语言），且面板仍在前台。
    /// </summary>
    private string EnsureAlphanumeric()
    {
        var native = Tsf.NativeMode();
        if (native != true || !Settings.ImeShiftToggle) return native is null ? "unknown" : native.Value ? "native(kept)" : "alphanumeric";
        _ = SendShiftWhenSafe();
        return "native -> shift";
    }

    private async Task SendShiftWhenSafe()
    {
        static bool Held(int vk) => (Win32.GetAsyncKeyState(vk) & 0x8000) != 0;
        await Task.Delay(Settings.ImeShiftDelayMs); // 等输入框的输入上下文在输入法中激活（立即发送会被忽略）
        for (int i = 0; i < 75 && (Held(Win32.VK_MENU) || Held(Win32.VK_CONTROL) || Held(Win32.VK_SHIFT) || Held(Win32.VK_LWIN) || Held(Win32.VK_RWIN)); i++)
            await Task.Delay(20);
        if (Win32.GetForegroundWindow() != Hwnd || composing || fieldText.Length > 0) return; // 用户已经开始输入时不再干预
        Win32.SendInput(2, new[] { Win32.Key(Win32.VK_SHIFT, false), Win32.Key(Win32.VK_SHIFT, true) },
            System.Runtime.InteropServices.Marshal.SizeOf<Win32.INPUT>());
    }

    public void HidePanel() => AppWindow.Hide();

    public void FocusField()
    {
        field.Focus(FocusState.Programmatic);
        var len = fieldText.Length;
        field.Document.Selection.SetRange(len, len);
    }

    // MARK: 界面构建

    private Border Divider(bool vertical)
    {
        var d = new Border { Width = vertical ? 1 : double.NaN, Height = vertical ? double.NaN : 1 };
        dividers.Add(d);
        return d;
    }

    private void BuildUi()
    {
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        // 顶栏
        var header = new Grid { Padding = new Thickness(18, 12, 18, 12), ColumnSpacing = 12 };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        wand.Text = ""; // Segoe Fluent Icons: LightningBolt 风格的"施法"图标
        wand.FontFamily = Theme.Icons;
        wand.FontSize = 20;
        wand.VerticalAlignment = VerticalAlignment.Center;
        header.Children.Add(wand);

        ConfigureField();
        fieldHost.Children.Add(field);
        ghost.FontFamily = Theme.Mono;
        ghost.FontSize = Theme.CommandSize;
        ghost.IsHitTestVisible = false;
        ghost.VerticalAlignment = VerticalAlignment.Center;
        ghost.HorizontalAlignment = HorizontalAlignment.Left;
        ghost.Visibility = Visibility.Collapsed;
        // 基线位置在布局完成后才能取得：尺寸变化时重新对齐
        ghost.SizeChanged += (_, _) => UpdateGhost();
        field.SizeChanged += (_, _) => UpdateGhost();
        fieldHost.Children.Add(ghost);
        Grid.SetColumn(fieldHost, 1);
        header.Children.Add(fieldHost);

        targetText.FontSize = 12;
        targetText.MaxWidth = 200;
        targetText.TextTrimming = TextTrimming.CharacterEllipsis;
        targetPill.Child = targetText;
        targetPill.CornerRadius = new CornerRadius(12);
        targetPill.Padding = new Thickness(10, 4, 10, 5);
        targetPill.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetColumn(targetPill, 2);
        header.Children.Add(targetPill);
        Grid.SetColumn(quick, 3);
        header.Children.Add(quick);
        root.Children.Add(header);

        var d1 = Divider(false); Grid.SetRow(d1, 1); root.Children.Add(d1);

        // 主体：左栏候选 + 右栏咒文卡
        var body = new Grid();
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(300) });
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        list.Padding = new Thickness(8);
        list.Spacing = 2;
        listScroll.Content = list;
        listScroll.HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled;
        body.Children.Add(listScroll);
        listEmpty.HorizontalAlignment = HorizontalAlignment.Center;
        listEmpty.VerticalAlignment = VerticalAlignment.Center;
        listEmpty.FontSize = 12;
        listEmpty.TextWrapping = TextWrapping.Wrap;
        listEmpty.TextAlignment = TextAlignment.Center;
        listEmpty.Margin = new Thickness(24, 0, 24, 0);
        body.Children.Add(listEmpty);
        var d2 = Divider(true); Grid.SetColumn(d2, 1); body.Children.Add(d2);
        card.Padding = new Thickness(16);
        card.Spacing = 12;
        var cardScroll = new ScrollViewer { Content = card, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
        Grid.SetColumn(cardScroll, 2);
        body.Children.Add(cardScroll);
        Grid.SetRow(body, 2);
        root.Children.Add(body);

        var d3 = Divider(false); Grid.SetRow(d3, 3); root.Children.Add(d3);

        // 底栏
        var footer = new Grid { Padding = new Thickness(16, 8, 16, 9), ColumnSpacing = 12 };
        footer.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        footer.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        statusText.FontSize = 12;
        statusText.TextTrimming = TextTrimming.CharacterEllipsis;
        statusText.VerticalAlignment = VerticalAlignment.Center;
        footer.Children.Add(statusText);
        Grid.SetColumn(keys, 1);
        footer.Children.Add(keys);
        Grid.SetRow(footer, 4);
        root.Children.Add(footer);

        tint.Children.Add(root);
        Content = tint;
        theme = new Theme(root.ActualTheme == ElementTheme.Dark || Application.Current.RequestedTheme == ApplicationTheme.Dark);
        ApplyStaticColors();
    }

    private void ConfigureField()
    {
        field.FontFamily = Theme.Mono;
        field.FontSize = Theme.CommandSize;
        field.AcceptsReturn = false;
        field.TextWrapping = TextWrapping.NoWrap;
        field.IsSpellCheckEnabled = false;
        field.IsTextPredictionEnabled = false;
        field.DisabledFormattingAccelerators = DisabledFormattingAccelerators.All;
        field.ClipboardCopyFormat = RichEditClipboardFormat.PlainText;
        field.BorderThickness = new Thickness(0);
        // 不用内边距（用外边距留出上下空白）：灰字与占位文字按光标的客户区坐标定位，内边距为 0 时两者原点一致
        field.Padding = new Thickness(0);
        field.Margin = new Thickness(0, 4, 0, 4);
        field.MinHeight = 0;
        field.VerticalAlignment = VerticalAlignment.Center;
        field.SelectionFlyout = null;
        // 去掉 TextBox 默认的背景、边框与焦点下划线（轻量样式资源）
        var clear = new SolidColorBrush(Colors.Transparent);
        foreach (var key in new[] { "TextControlBackground", "TextControlBackgroundPointerOver", "TextControlBackgroundFocused", "TextControlBackgroundDisabled",
                     "TextControlBorderBrush", "TextControlBorderBrushPointerOver", "TextControlBorderBrushFocused", "TextControlBorderBrushDisabled",
                     "TextControlElevationBorderBrush", "TextControlElevationBorderFocusedBrush" })
            field.Resources[key] = clear;
        field.Resources["TextControlBorderThemeThickness"] = new Thickness(0);
        field.Resources["TextControlBorderThemeThicknessFocused"] = new Thickness(0);
        // 模板中边框元素的最小高度（默认 32）会让单行文字靠上：去掉后输入框高度等于一行文字，在顶栏中垂直居中
        field.Resources["TextControlThemeMinHeight"] = 0.0;
        field.Resources["TextControlThemeMinWidth"] = 0.0;
        // 保险起见，模板加载后直接隐藏边框元素（焦点下划线与描边都画在它上面）
        field.Loaded += (_, _) => HideTemplateBorder(field);
        fieldHost.VerticalAlignment = VerticalAlignment.Center;
        ApplyInputScope();
        field.Document.UndoLimit = 0; // 着色会产生撤销记录；暂时关闭撤销（已知问题）

        field.TextChanged += (_, _) => OnFieldChanged();
        field.SelectionChanged += (_, _) => UpdateGhost();
        field.TextCompositionStarted += (_, _) => { composing = true; ghost.Visibility = Visibility.Collapsed; };
        field.TextCompositionEnded += (_, _) => { composing = false; DispatcherQueue.TryEnqueue(OnFieldChanged); };
        field.PreviewKeyDown += OnKey;
        field.Paste += OnPaste;
    }

    /// 隐藏文本控件模板中的 BorderElement（背景、描边与焦点下划线），文字区域不受影响。
    private static void HideTemplateBorder(DependencyObject root)
    {
        int n = VisualTreeHelper.GetChildrenCount(root);
        for (int i = 0; i < n; i++)
        {
            var c = VisualTreeHelper.GetChild(root, i);
            if (c is Border { Name: "BorderElement" } b)
            {
                b.Opacity = 0;
                b.MinHeight = 0;
                return;
            }
            HideTemplateBorder(c);
        }
    }

    /// <summary>
    /// 输入法策略（D9）：通过输入范围（InputScope）提示输入法本控件输入的是字母数字，
    /// 中文输入法据此在面板中默认使用英文模式，用户仍可手动切换。只影响本控件，关闭面板后无需恢复。
    /// </summary>
    public void ApplyInputScope()
    {
        if (!Settings.ForceAsciiInput) { field.InputScope = null; return; }
        var name = Environment.GetEnvironmentVariable("QANTRIP_IME_SCOPE") is { Length: > 0 } s && Enum.TryParse<Microsoft.UI.Xaml.Input.InputScopeNameValue>(s, out var v)
            ? v : Microsoft.UI.Xaml.Input.InputScopeNameValue.Url;
        var scope = new Microsoft.UI.Xaml.Input.InputScope();
        scope.Names.Add(new Microsoft.UI.Xaml.Input.InputScopeName(name));
        field.InputScope = scope;
    }

    private void ApplyStaticColors()
    {
        tint.Background = Theme.B(theme.Base);
        root.Background = new LinearGradientBrush
        {
            StartPoint = new Point(0, 0), EndPoint = new Point(0, 1),
            GradientStops =
            {
                new GradientStop { Color = Theme.WithAlpha(theme.Accent, 0.16), Offset = 0 },
                new GradientStop { Color = Theme.WithAlpha(theme.Accent, 0.0), Offset = 0.3 },
            },
        };
        wand.Foreground = Theme.B(theme.Accent);
        targetPill.Background = Theme.B(theme.Fill);
        targetText.Foreground = Theme.B(theme.Secondary);
        listEmpty.Foreground = Theme.B(theme.Tertiary);
        ApplyFieldColors();
        ghost.Foreground = Theme.B(theme.Tertiary);
        foreach (var d in dividers) d.Background = Theme.B(theme.Divider);
        BuildKeys();
        BuildQuick();
    }

    private void BuildQuick()
    {
        quick.Children.Clear();
        quick.Children.Add(HeaderIcon("\uE8F1", Loc.T("模板库（Ctrl+L）", "Template Library (Ctrl+L)"), "panel_library", () => Model.OnOpenLibrary?.Invoke()));
        quick.Children.Add(HeaderIcon("\uE713", Loc.T("设置（Ctrl+,）", "Settings (Ctrl+,)"), "panel_settings", () => Model.OnOpenSettings?.Invoke()));
    }

    /// 顶栏的小图标按钮（不取得焦点，输入框保持焦点）
    private FrameworkElement HeaderIcon(string glyph, string tip, string id, Action action)
    {
        var icon = new FontIcon { Glyph = glyph, FontFamily = Theme.Icons, FontSize = 14, Foreground = Theme.B(theme.Tertiary) };
        var b = new Border
        {
            Child = icon, Width = 28, Height = 28, CornerRadius = new CornerRadius(7), Background = Theme.B(Colors.Transparent),
        };
        ToolTipService.SetToolTip(b, tip);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(b, tip);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(b, id);
        b.PointerEntered += (_, _) => { b.Background = Theme.B(theme.Hover); icon.Foreground = Theme.B(theme.Text); };
        b.PointerExited += (_, _) => { b.Background = Theme.B(Colors.Transparent); icon.Foreground = Theme.B(theme.Tertiary); };
        b.Tapped += (_, e) => { e.Handled = true; action(); };
        return b;
    }

    private void ApplyFieldColors()
    {
        field.Foreground = Theme.B(theme.Text);
        placeholder = Loc.T("施放一个戏法…  如 r security 重点检查认证", "Cast a spell…  e.g. r security focus on auth");
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(field, placeholder);
        UpdateGhost();
    }

    /// 界面语言或底栏相关设置变化后刷新静态文字。
    public void ApplyLanguage()
    {
        ApplyFieldColors();
        BuildQuick();
        keysState = "";
        Refresh();
    }

    // MARK: 输入框

    private static string Clean(string raw) => raw.Replace("\r", "").Replace("\n", "");

    private string ReadField()
    {
        field.Document.GetText(TextGetOptions.None, out var raw);
        return Clean(raw);
    }

    private void SetFieldText(string text, int caret)
    {
        fieldText = text;
        field.Document.SetText(TextSetOptions.None, text);
        caret = Math.Clamp(caret, 0, text.Length);
        field.Document.Selection.SetRange(caret, caret);
    }

    private void OnFieldChanged()
    {
        if (composing) return; // 组字期间不重新解析，避免拼音字母触发匹配和高亮闪烁
        var text = ReadField();
        if (text == fieldText) return; // 只是格式变化（着色）或程序设置的文本
        fieldText = text;
        Model.TextDidChange(text);
    }

    private async void OnPaste(object sender, TextControlPasteEventArgs e)
    {
        // 只接受纯文本，换行替换为空格（命令行是单行）
        e.Handled = true;
        var content = Windows.ApplicationModel.DataTransfer.Clipboard.GetContent();
        if (!content.Contains(Windows.ApplicationModel.DataTransfer.StandardDataFormats.Text)) return;
        var text = await content.GetTextAsync();
        text = text.Replace("\r\n", " ").Replace('\n', ' ').Replace('\r', ' ');
        field.Document.Selection.SetText(TextSetOptions.None, text);
        field.Document.Selection.Collapse(false);
    }

    private static bool Down(VirtualKey k) =>
        InputKeyboardSource.GetKeyStateForCurrentThread(k).HasFlag(CoreVirtualKeyStates.Down);

    private void OnKey(object sender, KeyRoutedEventArgs e)
    {
        if (composing) return; // 组字期间的 Enter / Esc / 方向键归输入法
        bool ctrl = Down(VirtualKey.Control);
        switch (e.Key)
        {
            case VirtualKey.Enter:
                Model.Confirm(expand: ctrl && Settings.ExpandPasteEnabled);
                break;
            // 快捷入口：Ctrl+L 模板库、Ctrl+, 设置（排除 AltGr = Ctrl+Alt）
            case VirtualKey.L when ctrl && !Down(VirtualKey.Menu):
                Model.OnOpenLibrary?.Invoke();
                break;
            case (VirtualKey)0xBC when ctrl && !Down(VirtualKey.Menu): // VK_OEM_COMMA
                Model.OnOpenSettings?.Invoke();
                break;
            case VirtualKey.Up: Model.Move(-1); break;
            case VirtualKey.Down: Model.Move(1); break;
            case VirtualKey.Tab: Model.Complete(); break;
            case VirtualKey.Escape: Model.OnCancel?.Invoke(); break;
            case VirtualKey.Right or VirtualKey.End when !ctrl:
                // 光标在末尾时 → 接受灰字补全（类似 fish shell）
                var sel = field.Document.Selection;
                if (sel.Length != 0 || sel.StartPosition < fieldText.Length || !Model.AcceptGhost()) return;
                break;
            default: return;
        }
        e.Handled = true;
    }

    private void ApplyHighlighting()
    {
        if (composing) return;
        var a = Model.Analysis;
        var doc = field.Document;
        int len = ReadField().Length;
        doc.BatchDisplayUpdates();
        try
        {
            var all = doc.GetRange(0, len);
            all.CharacterFormat.ForegroundColor = theme.Text;
            all.CharacterFormat.Bold = FormatEffect.Off;
            all.CharacterFormat.Underline = UnderlineType.None;
            foreach (var t in a.Tokens)
            {
                if (t.End > len || t.Start >= t.End) continue;
                var r = doc.GetRange(t.Start, t.End);
                switch (t.Kind)
                {
                    case "command": r.CharacterFormat.ForegroundColor = theme.Accent; r.CharacterFormat.Bold = FormatEffect.On; break;
                    case "command_unknown" or "invalid": r.CharacterFormat.ForegroundColor = theme.Error; break;
                    case "option_name" or "short_flag": r.CharacterFormat.ForegroundColor = theme.Option; break;
                    case "option_value" or "positional": r.CharacterFormat.ForegroundColor = theme.Value; break;
                    case "extra_marker": r.CharacterFormat.ForegroundColor = theme.Tertiary; break;
                }
            }
            // 错误加波浪下划线；正在输入的词不标，避免边打字边报错
            foreach (var d in a.Diagnostics)
            {
                if (d.AtCursor || d.End <= d.Start || d.End > len) continue;
                doc.GetRange(d.Start, d.End).CharacterFormat.Underline = UnderlineType.Wave;
            }
            doc.Selection.CharacterFormat.ForegroundColor = theme.Text;
            doc.Selection.CharacterFormat.Bold = FormatEffect.Off;
        }
        finally { doc.ApplyDisplayUpdates(); }
    }

    /// 灰字补全画在输入文字末尾之后（与输入同一行、同一字体）；输入框为空时在同一位置画占位文字。
    private void UpdateGhost()
    {
        int len = fieldText.Length;
        bool empty = len == 0;
        var g = empty ? placeholder : Model.Analysis.Ghost;
        var sel = field.Document.Selection;
        if (composing || string.IsNullOrEmpty(g) || sel.Length != 0 || sel.StartPosition != len)
        {
            ghost.Visibility = Visibility.Collapsed;
            return;
        }
        double x = 0;
        if (len > 0)
        {
            field.Document.GetRange(len, len).GetRect(PointOptions.ClientCoordinates, out Rect rect, out _);
            x = rect.X;
        }
        if (ghost.Text != g) ghost.Text = g;
        ghost.Margin = new Thickness(x + field.Padding.Left, BaselineTop(len), 0, 0);
        // 超出可见宽度时不显示补全（占位文字总是显示）
        ghost.Visibility = empty || x + 40 < field.ActualWidth ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>
    /// 灰字的上边距：让它的基线与光标所在行的基线重合（字体回退或行高不同时，两个控件各自垂直居中会错开）。
    /// 光标基线取自 ITextRange.GetPoint(Baseline)（输入框客户区坐标），灰字基线取自 TextBlock.BaselineOffset；
    /// 布局尚未完成时退回垂直居中，尺寸变化后会再对齐一次。
    /// </summary>
    private double BaselineTop(int pos)
    {
        double ghostBaseline = ghost.BaselineOffset;
        if (ghostBaseline > 0 && field.ActualHeight > 0 && field.XamlRoot != null)
        {
            try
            {
                field.Document.GetRange(pos, pos).GetPoint(HorizontalCharacterAlignment.Left, VerticalCharacterAlignment.Baseline, PointOptions.ClientCoordinates, out Point caret);
                var origin = field.TransformToVisual(fieldHost).TransformPoint(new Point(0, 0));
                double top = origin.Y + field.BorderThickness.Top + field.Padding.Top + caret.Y - ghostBaseline;
                if (caret.Y > 0 && top > -ghostBaseline && top < fieldHost.ActualHeight)
                {
                    ghost.VerticalAlignment = VerticalAlignment.Top;
                    return Math.Round(top, 1);
                }
            }
            catch (Exception e) { Log.Error("ghost_baseline", e); }
        }
        ghost.VerticalAlignment = VerticalAlignment.Center;
        return 0;
    }

    // MARK: 刷新

    public void Refresh()
    {
        var a = Model.Analysis;
        if (ReadField() == fieldText) ApplyHighlighting();
        UpdateGhost();
        var t = Model.Target;
        targetText.Text = t is null ? "" : "→ " + (string.IsNullOrEmpty(t.Exe) ? t.Title : Path.GetFileNameWithoutExtension(t.Exe));
        targetPill.Visibility = t is null ? Visibility.Collapsed : Visibility.Visible;
        BuildList();
        BuildCard();
        BuildStatus();
        var ks = $"{Model.IsCommandMode}|{Settings.ExpandPasteEnabled}|{Loc.En}";
        if (ks != keysState) { keysState = ks; BuildKeys(); }
        if (a.Locked && !wasLocked) Anim.Pop(wand);
        wasLocked = a.Locked;
    }

    /// 候选列表的"类型"：首屏 / 模板 / 参数 / 取值。类型切换时整组淡入，同类内只有新出现的项淡入（D15）。
    private string ListKind(Analysis a)
    {
        if (a.Candidates.Count == 0) return "empty";
        if (string.IsNullOrWhiteSpace(Model.Query)) return "home";
        return a.Locked ? "locked-" + a.Candidates[0].Kind : "command";
    }

    private void BuildList()
    {
        list.Children.Clear();
        rowElements.Clear();
        var a = Model.Analysis;
        listEmpty.Visibility = a.Candidates.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        // 指令模式下没有候选时，左栏显示这一步的提示（如"输入新模板的名称"）
        listEmpty.Text = a.Phase == "extra" ? Loc.T("正在输入附加要求", "Entering extra instructions")
            : Model.IsCommandMode && !string.IsNullOrEmpty(a.Hint) ? a.Hint
            : Loc.T("没有可补全的内容", "Nothing to complete");
        string? section = null;
        FrameworkElement? selected = null;
        var kind = ListKind(a);
        bool kindChanged = kind != listKind;
        var ids = new HashSet<string>();
        for (int i = 0; i < a.Candidates.Count; i++)
        {
            var c = a.Candidates[i];
            if (!string.IsNullOrEmpty(c.Section) && c.Section != section)
            {
                section = c.Section;
                var header = new TextBlock
                {
                    Text = c.Section, FontSize = 11, FontWeight = FontWeights.SemiBold, Foreground = Theme.B(theme.Tertiary),
                    Margin = new Thickness(10, list.Children.Count == 0 ? 2 : 10, 10, 2),
                };
                if (entering) Anim.FadeSlideIn(header, new Vector3(0, 8, 0), 280, Math.Min(list.Children.Count, 12) * 16);
                list.Children.Add(header);
            }
            var row = CandidateRow(c, i, i == Model.SelectedIndex);
            var id = $"{c.Section}|{c.Kind}|{c.Key}";
            ids.Add(id);
            // 入场：错落出现；同类列表中新出现的项：淡入并轻微上移
            if (entering) Anim.FadeSlideIn(row, new Vector3(0, 8, 0), 280, Math.Min(list.Children.Count, 12) * 16);
            else if (!kindChanged && !rowIds.Contains(id)) Anim.FadeSlideIn(row, new Vector3(0, 6, 0), 160);
            list.Children.Add(row);
            rowElements.Add(row);
            if (i == Model.SelectedIndex) selected = row;
        }
        // 类型切换：整组从左侧淡入（旧列表直接移除，避免新旧两组重叠）
        if (kindChanged && !entering && listKind != "") Anim.FadeSlideIn(list, new Vector3(-6, 0, 0), 140);
        entering = false;
        listKind = kind;
        rowIds = ids;
        if (selected != null)
            DispatcherQueue.TryEnqueue(() => selected.StartBringIntoView(new BringIntoViewOptions { AnimationDesired = false }));
    }

    private FrameworkElement CandidateRow(Candidate c, int index, bool selected)
    {
        var grid = new Grid { ColumnSpacing = 8 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var left = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        if (c.Kind == "invocation")
        {
            bool tryIt = c.Section is "试试看" or "Try these";
            left.Children.Add(new TextBlock
            {
                Text = tryIt ? "" : "", FontFamily = Theme.Icons, FontSize = 12,
                Foreground = Theme.B(theme.Tertiary), VerticalAlignment = VerticalAlignment.Center,
            });
            left.Children.Add(ColorizedInvocation(c.Title));
        }
        else
        {
            left.Children.Add(new TextBlock
            {
                Text = c.Title, FontFamily = Theme.Mono, FontSize = 14,
                FontWeight = c.Kind == "template" ? FontWeights.SemiBold : FontWeights.Normal,
                Foreground = Theme.B(c.Kind switch { "param" => theme.Option, "choice" => theme.Value, _ => theme.Text }),
                VerticalAlignment = VerticalAlignment.Center,
            });
            if (!string.IsNullOrEmpty(c.Subtitle))
                left.Children.Add(new TextBlock
                {
                    Text = c.Subtitle, FontSize = 12, Foreground = Theme.B(theme.Secondary),
                    TextTrimming = TextTrimming.CharacterEllipsis, VerticalAlignment = VerticalAlignment.Center,
                });
        }
        grid.Children.Add(left);
        var right = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        if (c.Active)
            right.Children.Add(new TextBlock { Text = "", FontFamily = Theme.Icons, FontSize = 11, Foreground = Theme.B(theme.Accent), VerticalAlignment = VerticalAlignment.Center });
        if (c.Kind == "template")
            right.Children.Add(new TextBlock
            {
                Text = c.Detail, FontSize = 12, Foreground = Theme.B(theme.Secondary), MaxWidth = 120,
                TextTrimming = TextTrimming.CharacterEllipsis, VerticalAlignment = VerticalAlignment.Center,
            });
        Grid.SetColumn(right, 1);
        grid.Children.Add(right);

        var row = new Border
        {
            Child = grid, Padding = new Thickness(10, 6, 10, 7), CornerRadius = new CornerRadius(8),
            Background = Theme.B(selected ? Theme.WithAlpha(theme.Accent, 0.22) : Colors.Transparent),
        };
        if (!string.IsNullOrEmpty(c.Detail)) ToolTipService.SetToolTip(row, c.Detail);
        var bg = row.Background;
        row.PointerEntered += (_, _) => { if (!selected) row.Background = Theme.B(theme.Hover); };
        row.PointerExited += (_, _) => row.Background = bg;
        var kind = c.Kind;
        row.Tapped += (_, e) => { e.Handled = true; Model.Click(index); FocusField(); };
        // 双击：第一次单击已经接受了该候选（模板 / 首屏调用），这里直接确认插入
        row.DoubleTapped += (_, e) => { e.Handled = true; if (kind is "template" or "invocation") Model.Confirm(false); };
        return row;
    }

    /// 简单着色：命令词、--参数、=取值（用于首屏的调用记录）
    private TextBlock ColorizedInvocation(string s)
    {
        var tb = new TextBlock { FontFamily = Theme.Mono, FontSize = 13, TextTrimming = TextTrimming.CharacterEllipsis, VerticalAlignment = VerticalAlignment.Center };
        var words = s.Split(' ');
        for (int i = 0; i < words.Length; i++)
        {
            if (i > 0) tb.Inlines.Add(new Run { Text = " " });
            var w = words[i];
            if (i == 0) tb.Inlines.Add(new Run { Text = w, Foreground = Theme.B(theme.Accent) });
            else if (w.StartsWith('-') && w.IndexOf('=') is int eq and > 0)
            {
                tb.Inlines.Add(new Run { Text = w[..(eq + 1)], Foreground = Theme.B(theme.Option) });
                tb.Inlines.Add(new Run { Text = w[(eq + 1)..], Foreground = Theme.B(theme.Value) });
            }
            else if (w.StartsWith('-')) tb.Inlines.Add(new Run { Text = w, Foreground = Theme.B(theme.Option) });
            else tb.Inlines.Add(new Run { Text = w, Foreground = Theme.B(theme.Text) });
        }
        return tb;
    }

    private void BuildCard()
    {
        card.Children.Clear();
        var a = Model.Analysis;
        if (a.Template is null && Model.IsCommandMode && a.Preview is Preview help)
        {
            // 指令模式：还没确定指令时显示用法说明
            previewTemplate = null;
            card.Children.Add(new TextBlock { Text = Loc.T("指令", "Commands"), FontFamily = Theme.Display, FontSize = 22, Foreground = Theme.B(theme.Accent) });
            card.Children.Add(new TextBlock
            {
                Text = help.Text, FontFamily = Theme.Mono, FontSize = 12, LineHeight = 20, Foreground = Theme.B(theme.Secondary),
                TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true,
            });
            return;
        }
        if (a.Template is not TemplateInfo t)
        {
            previewTemplate = null;
            card.Children.Add(new TextBlock
            {
                Text = Loc.T("没有匹配的模板", "No matching templates"), Foreground = Theme.B(theme.Tertiary),
                HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 150, 0, 0),
            });
            return;
        }
        var head = new Grid { ColumnSpacing = 8 };
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var title = new TextBlock { Text = t.Name, FontFamily = Theme.Display, FontSize = 22, Foreground = Theme.B(theme.Accent) };
        head.Children.Add(title);
        // 模板切换：标题从左侧滑入
        if (t.Name != previewTemplate) Anim.FadeSlideIn(head, new Vector3(10, 0, 0), 180);
        var desc = new TextBlock { Text = t.Description, FontSize = 13, Foreground = Theme.B(theme.Secondary), TextTrimming = TextTrimming.CharacterEllipsis, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(desc, 1);
        head.Children.Add(desc);
        if (!string.IsNullOrEmpty(a.Hint))
        {
            var hint = new TextBlock { Text = a.Hint, FontSize = 11, Foreground = Theme.B(theme.Secondary), VerticalAlignment = VerticalAlignment.Center };
            Grid.SetColumn(hint, 2);
            head.Children.Add(hint);
        }
        card.Children.Add(head);

        if (a.Params.Count > 0)
        {
            var chips = new WrapPanel();
            foreach (var p in a.Params) chips.Children.Add(Chip(p));
            card.Children.Add(chips);
        }
        if (a.Preview is Preview pv) card.Children.Add(PreviewBox(pv, t.Name));
        else previewTemplate = t.Name;
    }

    /// 参数标签：显示当前取值，点击可修改（D16）。
    private FrameworkElement Chip(ParamState p)
    {
        var sp = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
        sp.Children.Add(new TextBlock { Text = p.Name, FontFamily = Theme.Mono, FontSize = 11, Foreground = Theme.B(p.IsSet ? theme.Option : theme.Secondary) });
        sp.Children.Add(new TextBlock
        {
            Text = p.Display, FontSize = 11, FontWeight = p.IsSet ? FontWeights.SemiBold : FontWeights.Normal,
            Foreground = Theme.B(p.IsSet ? theme.Text : theme.Tertiary),
        });
        var chip = new Border
        {
            Child = sp, Padding = new Thickness(8, 3, 8, 4), CornerRadius = new CornerRadius(11), BorderThickness = new Thickness(1),
            BorderBrush = Theme.B(p.IsSet ? Theme.WithAlpha(theme.Accent, 0.45) : theme.Divider),
            Background = Theme.B(p.IsSet ? Theme.WithAlpha(theme.Accent, 0.16) : Colors.Transparent),
        };
        if (!string.IsNullOrEmpty(p.Description)) ToolTipService.SetToolTip(chip, p.Description);
        chip.Tapped += (_, e) =>
        {
            e.Handled = true;
            switch (p.Kind)
            {
                case "bool": Model.EditParam(p.Name, ParamEdit.Toggle()); FocusField(); break;
                case "enum" or "multi":
                    var menu = new MenuFlyout();
                    foreach (var c in p.Choices)
                    {
                        var item = new ToggleMenuFlyoutItem { Text = $"{c.Label}  {c.Value}", IsChecked = c.On };
                        var value = c.Value;
                        item.Click += (_, _) => { Model.EditParam(p.Name, p.Kind == "multi" ? ParamEdit.ToggleChoice(value) : ParamEdit.Set(value)); FocusField(); };
                        menu.Items.Add(item);
                    }
                    if (p.IsSet)
                    {
                        menu.Items.Add(new MenuFlyoutSeparator());
                        var reset = new MenuFlyoutItem { Text = Loc.T("恢复默认", "Restore default") };
                        reset.Click += (_, _) => { Model.EditParam(p.Name, ParamEdit.Clear()); FocusField(); };
                        menu.Items.Add(reset);
                    }
                    menu.Closed += (_, _) => FocusField();
                    menu.ShowAt(chip);
                    break;
                default:
                    Model.EditParam(p.Name, ParamEdit.Begin());
                    field.Focus(FocusState.Programmatic);
                    break;
            }
        };
        return chip;
    }

    private FrameworkElement PreviewBox(Preview p, string template)
    {
        var box = new StackPanel { Spacing = 6 };
        var meta = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        meta.Children.Add(new TextBlock { Text = Loc.T("预览", "Preview"), FontSize = 11, FontWeight = FontWeights.SemiBold, Foreground = Theme.B(theme.Secondary) });
        meta.Children.Add(new TextBlock { Text = Loc.T($"{p.Chars} 字 · {p.Lines} 行", $"{p.Chars} chars · {p.Lines} lines"), FontSize = 11, Foreground = Theme.B(theme.Tertiary) });
        if (p.CollapsesInClaudeCode && Settings.ExpandPasteEnabled)
            meta.Children.Add(new TextBlock { Text = Loc.T("Claude Code 中会折叠 · Ctrl+↩ 展开粘贴", "Collapsed in Claude Code · Ctrl+↩ to expand"), FontSize = 11, Foreground = Theme.B(theme.Secondary) });
        box.Children.Add(meta);

        var text = new TextBlock
        {
            Text = p.Text, FontSize = 13, LineHeight = 20, TextWrapping = TextWrapping.Wrap,
            IsTextSelectionEnabled = true, Foreground = Theme.B(theme.Text),
        };
        var param = new TextHighlighter { Background = Theme.B(Theme.WithAlpha(theme.Accent, 0.28)) };
        var extra = new TextHighlighter { Background = Theme.B(Theme.WithAlpha(theme.Value, 0.28)) };
        foreach (var h in p.Highlights)
        {
            if (h.End > p.Text.Length || h.End <= h.Start) continue;
            (h.Kind == "extra" ? extra : param).Ranges.Add(new TextRange { StartIndex = h.Start, Length = h.End - h.Start });
        }
        text.TextHighlighters.Add(param);
        text.TextHighlighters.Add(extra);
        // 预览闪光：同一模板下新出现的高亮片段先亮起，再淡成常驻高亮（D15）
        var keys = p.Highlights.Select(h => $"{h.Kind}-{h.Start}-{h.End}").ToHashSet();
        if (template == previewTemplate && Motion.Enabled)
        {
            var fresh = p.Highlights.Where(h => !previewHighlights.Contains($"{h.Kind}-{h.Start}-{h.End}") && h.End <= p.Text.Length && h.End > h.Start).ToList();
            if (fresh.Count > 0)
            {
                var brush = new SolidColorBrush(Theme.WithAlpha(theme.Accent, 0.76));
                var flash = new TextHighlighter { Background = brush };
                foreach (var h in fresh) flash.Ranges.Add(new TextRange { StartIndex = h.Start, Length = h.End - h.Start });
                text.TextHighlighters.Add(flash);
                StartFlash(brush, theme.Accent);
            }
        }
        previewTemplate = template;
        previewHighlights = keys;
        box.Children.Add(new Border { Child = text, Padding = new Thickness(12), CornerRadius = new CornerRadius(10), Background = Theme.B(theme.Panel) });
        return box;
    }

    /// 闪光强度 1 → 0（450ms，先慢后快），叠加在常驻高亮（0.26）之上。
    private void StartFlash(SolidColorBrush brush, Color accent)
    {
        flashTimer?.Stop();
        var start = DateTime.UtcNow;
        var timer = DispatcherQueue.CreateTimer();
        timer.Interval = TimeSpan.FromMilliseconds(16);
        timer.Tick += (_, _) =>
        {
            double p = (DateTime.UtcNow - start).TotalMilliseconds / 450;
            double level = Math.Max(0, 1 - p * p);
            // 闪光层叠在常驻高亮之上：只画超出常驻高亮的部分
            brush.Color = Theme.WithAlpha(accent, 0.5 * level);
            if (p >= 1) timer.Stop();
        };
        flashTimer = timer;
        timer.Start();
    }

    private void BuildStatus()
    {
        var a = Model.Analysis;
        if (Model.ArmedCommand != null)
        {
            statusText.Text = "⚠ " + Loc.T("再按一次 ↩ 确认", "Press Enter again to confirm");
            statusText.Foreground = Theme.B(theme.Warning);
        }
        else if (Model.FirstError is Diagnostic e)
        {
            statusText.Text = "ⓘ " + e.Message;
            statusText.Foreground = Theme.B(e.AtCursor ? theme.Secondary : theme.Error);
        }
        else if (a.Diagnostics.FirstOrDefault(d => !d.IsError) is Diagnostic w)
        {
            statusText.Text = "⚠ " + w.Message;
            statusText.Foreground = Theme.B(theme.Warning);
        }
        else if (a.Locked && a.Canonical is string c)
        {
            statusText.Text = c;
            statusText.FontFamily = Theme.Mono;
            statusText.Foreground = Theme.B(theme.Tertiary);
            return;
        }
        else statusText.Text = "";
        statusText.FontFamily = Theme.Ui;
    }

    private void BuildKeys()
    {
        keys.Children.Clear();
        bool command = Model.IsCommandMode;
        keys.Children.Add(Keycap("↩", command ? Loc.T("执行", "Run") : Loc.T("插入", "Insert"), () => Model.Confirm(false)));
        if (Settings.ExpandPasteEnabled && !command) keys.Children.Add(Keycap("Ctrl+↩", Loc.T("展开粘贴", "Expand paste"), () => Model.Confirm(true)));
        keys.Children.Add(Keycap("Tab", Loc.T("补全", "Complete"), null));
        keys.Children.Add(Keycap("↑↓", Loc.T("选择", "Select"), null));
        keys.Children.Add(Keycap("Esc", Loc.T("关闭", "Close"), () => Model.OnCancel?.Invoke()));
    }

    private FrameworkElement Keycap(string key, string label, Action? action)
    {
        var sp = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
        sp.Children.Add(new Border
        {
            Child = new TextBlock { Text = key, FontSize = 10, Foreground = Theme.B(theme.Secondary) },
            Padding = new Thickness(5, 1, 5, 2), CornerRadius = new CornerRadius(4), Background = Theme.B(theme.Fill),
        });
        sp.Children.Add(new TextBlock { Text = label, FontSize = 11, Foreground = Theme.B(theme.Secondary), VerticalAlignment = VerticalAlignment.Center });
        var b = new Border { Child = sp, Padding = new Thickness(4, 2, 4, 2), CornerRadius = new CornerRadius(6), Background = Theme.B(Colors.Transparent) };
        if (action != null)
        {
            b.PointerEntered += (_, _) => b.Background = Theme.B(theme.Hover);
            b.PointerExited += (_, _) => b.Background = Theme.B(Colors.Transparent);
            b.Tapped += (_, e) => { e.Handled = true; action(); };
        }
        return b;
    }

    /// 回车被阻止：面板抖动、图标短暂变红（只做视觉反馈，不发声）。
    private void FlashReject()
    {
        Anim.Shake(root);
        wand.Foreground = Theme.B(theme.Error);
        var timer = DispatcherQueue.CreateTimer();
        timer.Interval = TimeSpan.FromMilliseconds(320);
        timer.IsRepeating = false;
        timer.Tick += (_, _) => wand.Foreground = Theme.B(theme.Accent);
        timer.Start();
    }
}
