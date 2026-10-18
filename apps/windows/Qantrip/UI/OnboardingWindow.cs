using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Qantrip.Platform;
using Windows.Graphics;
using static Qantrip.Platform.Win32;

namespace Qantrip.UI;

/// <summary>
/// 首次启动引导（PRD 11.2 Onboarding，对应 macOS OnboardingView.swift）：
/// Qantrip 做什么 + 确认 / 更换快捷键 → 插入方式（自动粘贴 / 只复制，Windows 不需要授权）→ 施放一次。
/// 只显示一次：完成或关闭窗口后写入 settings.json 的 onboardingDone；托盘菜单"使用引导…"可以再次打开。
/// </summary>
internal sealed class OnboardingWindow : Window
{
    private readonly App app;
    private readonly IntPtr hwnd;
    private readonly Grid root = new();
    private readonly Grid stepHost = new();
    private readonly StackPanel dots = new() { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Stretch };
    private readonly StackPanel footer = new() { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Right };
    private int step;
    private bool hotKeyVerified;
    private HotKeyRecorder? recorder;
    private TextBlock? verifyText;
    private FontIcon? verifyIcon;
    private TextBox? practice;
    private TextBlock? practiceDone;

    public OnboardingWindow(App app)
    {
        this.app = app;
        hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        Title = Loc.T("欢迎使用 Nekro Qantrip", "Welcome to Nekro Qantrip");
        ExtendsContentIntoTitleBar = true;
        if (MicaController.IsSupported()) SystemBackdrop = new MicaBackdrop();
        else root.Background = SettingsView.Res("SolidBackgroundFillColorBaseBrush");
        var p = OverlappedPresenter.Create();
        p.IsMaximizable = false;
        p.IsMinimizable = false;
        p.IsResizable = false;
        AppWindow.SetPresenter(p);
        var icon = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        if (File.Exists(icon)) AppWindow.SetIcon(icon);
        Place();
        Content = root;
        Closed += (_, _) =>
        {
            recorder?.Cancel();
            // 只显示一次：关闭也算看过（托盘菜单可以再次打开）
            Settings.OnboardingDone = true;
            Log.Event("onboarding", new() { ["closed_at_step"] = step });
        };
        Build();
        Log.Event("onboarding", new() { ["shown"] = true });
    }

    private void Place()
    {
        GetCursorPos(out var pt);
        var mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        var mi = new MONITORINFO { cbSize = System.Runtime.InteropServices.Marshal.SizeOf<MONITORINFO>() };
        GetMonitorInfo(mon, ref mi);
        GetDpiForMonitor(mon, 0, out uint dpi, out _);
        double scale = dpi == 0 ? 1 : dpi / 96.0;
        var wa = mi.rcWork;
        int w = (int)Math.Round(640 * scale), h = (int)Math.Round(560 * scale);
        AppWindow.MoveAndResize(new RectInt32(wa.Left + (wa.Right - wa.Left - w) / 2, wa.Top + (wa.Bottom - wa.Top - h) / 2, w, h));
    }

    public void ShowAndActivate()
    {
        AppWindow.Show(true);
        Activate();
        Foreground.Activate(hwnd, out _);
    }

    /// 全局快捷键被按下（第一步用来确认快捷键可用）
    public void HotKeyPressed()
    {
        if (hotKeyVerified) return;
        hotKeyVerified = true;
        UpdateVerify();
    }

    private void Build()
    {
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(36) });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var titleHost = new Grid { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
        var tb = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10, Padding = new Thickness(16, 0, 0, 0), VerticalAlignment = VerticalAlignment.Center };
        var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        if (File.Exists(iconPath)) tb.Children.Add(new Image { Source = new BitmapImage(new Uri(iconPath)), Width = 16, Height = 16 });
        tb.Children.Add(new TextBlock { Text = Title, FontSize = 12, VerticalAlignment = VerticalAlignment.Center });
        titleHost.Children.Add(tb);
        root.Children.Add(titleHost);
        SetTitleBar(titleHost);

        var dotsHost = new Grid { Padding = new Thickness(32, 12, 32, 0) };
        for (int i = 0; i < 3; i++) dotsHost.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        dotsHost.ColumnSpacing = 8;
        for (int i = 0; i < 3; i++)
        {
            var bar = new Border { Height = 4, CornerRadius = new CornerRadius(2) };
            Grid.SetColumn(bar, i);
            dotsHost.Children.Add(bar);
        }
        Grid.SetRow(dotsHost, 1);
        root.Children.Add(dotsHost);
        dotsBars = dotsHost.Children.OfType<Border>().ToList();

        stepHost.Padding = new Thickness(40, 0, 40, 0);
        Grid.SetRow(stepHost, 2);
        root.Children.Add(stepHost);
        var foot = new Border { Padding = new Thickness(20), Child = footer };
        Grid.SetRow(foot, 3);
        root.Children.Add(foot);
        ShowStep(0);
    }

    private List<Border> dotsBars = new();

    private static Brush Accent => new SolidColorBrush(new Theme(Application.Current.RequestedTheme == ApplicationTheme.Dark).Accent);

    private void ShowStep(int s)
    {
        step = s;
        for (int i = 0; i < dotsBars.Count; i++) dotsBars[i].Background = i <= s ? Accent : SettingsView.Res("ControlStrongFillColorDisabledBrush");
        stepHost.Children.Clear();
        var content = s switch { 0 => HotKeyStep(), 1 => InsertStep(), _ => PracticeStep() };
        stepHost.Children.Add(content);
        if (Motion.Enabled) Anim.FadeSlideIn(content, new System.Numerics.Vector3(24, 0, 0), 220);
        BuildFooter();
        Log.Event("onboarding", new() { ["step"] = s });
    }

    private void BuildFooter()
    {
        footer.Children.Clear();
        if (step > 0) footer.Children.Add(Btn("onb_back", Loc.T("上一步", "Back"), () => ShowStep(step - 1)));
        if (step < 2) footer.Children.Add(Btn("onb_next", Loc.T("继续", "Continue"), () => ShowStep(step + 1), accent: true));
        else footer.Children.Add(Btn("onb_done", Loc.T("开始使用", "Get started"), () => { Settings.OnboardingDone = true; Close(); }, accent: true));
    }

    private static Button Btn(string id, string text, Action click, bool accent = false)
    {
        var b = new Button { Content = text, MinWidth = 96 };
        if (accent) b.Style = (Style)Application.Current.Resources["AccentButtonStyle"];
        AutomationProperties.SetAutomationId(b, id);
        b.Click += (_, _) => click();
        return b;
    }

    private static StackPanel Title2(StackPanel host, string title, string sub)
    {
        host.Children.Add(new TextBlock { Text = title, FontFamily = Theme.Display, FontSize = 28, HorizontalAlignment = HorizontalAlignment.Center, TextAlignment = TextAlignment.Center, TextWrapping = TextWrapping.Wrap });
        host.Children.Add(new TextBlock { Text = sub, TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center, Foreground = SettingsView.Res("TextFillColorSecondaryBrush") });
        return host;
    }

    private FrameworkElement HotKeyStep()
    {
        var sp = new StackPanel { Spacing = 18, VerticalAlignment = VerticalAlignment.Center };
        sp.Children.Add(new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 44, Foreground = Accent });
        Title2(sp, Loc.T("按 Q 施放一个戏法", "Press Q to cast a spell"),
            Loc.T("在任何程序里按下唤起快捷键，输入几个字母，就能把常用 Prompt 插入到当前输入框。", "In any app, press the shortcut, type a few letters, and insert a prompt into the current text field."));
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12, HorizontalAlignment = HorizontalAlignment.Center };
        row.Children.Add(new TextBlock { Text = Loc.T("唤起快捷键", "Panel shortcut"), VerticalAlignment = VerticalAlignment.Center });
        recorder?.Cancel();
        recorder = new HotKeyRecorder(app, alignRight: false);
        recorder.Changed += _ => { hotKeyVerified = false; UpdateVerify(); };
        row.Children.Add(recorder.View);
        sp.Children.Add(row);
        verifyIcon = new FontIcon { FontFamily = Theme.Icons, FontSize = 16 };
        verifyText = new TextBlock { VerticalAlignment = VerticalAlignment.Center };
        AutomationProperties.SetAutomationId(verifyText, "onb_verify");
        var v = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Center };
        v.Children.Add(verifyIcon);
        v.Children.Add(verifyText);
        sp.Children.Add(v);
        UpdateVerify();
        return sp;
    }

    private void UpdateVerify()
    {
        if (verifyText is null || verifyIcon is null) return;
        bool dark = root.ActualTheme == ElementTheme.Dark || (root.ActualTheme == ElementTheme.Default && Application.Current.RequestedTheme == ApplicationTheme.Dark);
        var green = new SolidColorBrush(dark ? Windows.UI.Color.FromArgb(255, 0x6C, 0xCB, 0x5F) : Windows.UI.Color.FromArgb(255, 0x0F, 0x7B, 0x0F));
        verifyText.Text = hotKeyVerified ? Loc.T("快捷键可以正常使用", "The shortcut works") : Loc.T($"现在按一次 {app.HotKeyText} 试试", $"Press {app.HotKeyText} now to try it");
        verifyIcon.Glyph = hotKeyVerified ? "" : "";
        verifyText.Foreground = verifyIcon.Foreground = hotKeyVerified ? green : SettingsView.Res("TextFillColorSecondaryBrush");
    }

    private FrameworkElement InsertStep()
    {
        var sp = new StackPanel { Spacing = 16, VerticalAlignment = VerticalAlignment.Center };
        sp.Children.Add(new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 44, Foreground = Accent });
        Title2(sp, Loc.T("插入到当前程序", "Insert into the current app"),
            Loc.T("回车后，Qantrip 把 Prompt 放到剪贴板并替你按下 Ctrl+V，随后恢复原来的剪贴板；Prompt 不会进入剪贴板历史，也不会自动发送。Windows 上不需要任何授权。",
                "Press Enter and Qantrip puts the prompt on the clipboard, presses Ctrl+V for you, then restores your clipboard. The prompt stays out of clipboard history and is never sent automatically. No permissions needed on Windows."));
        var radios = new RadioButtons { HorizontalAlignment = HorizontalAlignment.Center };
        radios.Items.Add(new RadioButton { Content = Loc.T("自动粘贴到当前程序（推荐）", "Auto-paste into the current app (recommended)") });
        radios.Items.Add(new RadioButton { Content = Loc.T("只复制，由我手动粘贴", "Copy only; I'll paste myself") });
        radios.SelectedIndex = Settings.CopyOnly ? 1 : 0;
        AutomationProperties.SetAutomationId(radios, "onb_insert");
        radios.SelectionChanged += (_, _) => { if (radios.SelectedIndex >= 0) Settings.CopyOnly = radios.SelectedIndex == 1; };
        sp.Children.Add(radios);
        sp.Children.Add(new TextBlock
        {
            Text = Loc.T("目标程序以管理员身份运行时，系统不允许模拟按键，Qantrip 会自动改为只复制并提示你。以后可以在设置中更改。",
                "If the target app runs as administrator, Windows blocks simulated keys; Qantrip then copies only and tells you. You can change this later in Settings."),
            FontSize = 12, TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center,
            Foreground = SettingsView.Res("TextFillColorSecondaryBrush"),
        });
        return sp;
    }

    private FrameworkElement PracticeStep()
    {
        var sp = new StackPanel { Spacing = 16, VerticalAlignment = VerticalAlignment.Center };
        Title2(sp, Loc.T("施放一次", "Try it once"),
            Loc.T($"点一下下面的输入框，按 {app.HotKeyText}，输入 zh 后按回车。", $"Click the field below, press {app.HotKeyText}, type zh, then press Enter."));
        practice = new TextBox { AcceptsReturn = true, TextWrapping = TextWrapping.Wrap, Height = 130, FontSize = 14 };
        AutomationProperties.SetAutomationId(practice, "onb_practice");
        AutomationProperties.SetName(practice, Loc.T("练习输入框", "Practice field"));
        practiceDone = new TextBlock
        {
            Text = "✦ " + Loc.T("成功了！之后在任何程序里都可以这样使用。", "It worked! You can use it like this in any app."),
            Foreground = Accent, HorizontalAlignment = HorizontalAlignment.Center, Visibility = Visibility.Collapsed,
        };
        AutomationProperties.SetAutomationId(practiceDone, "onb_success");
        practice.TextChanged += (_, _) =>
        {
            bool done = practice.Text.Length > 0;
            if (done && practiceDone.Visibility != Visibility.Visible && Motion.Enabled) Anim.FadeSlideIn(practiceDone, new System.Numerics.Vector3(0, 6, 0), 200);
            practiceDone.Visibility = done ? Visibility.Visible : Visibility.Collapsed;
        };
        sp.Children.Add(practice);
        sp.Children.Add(practiceDone);
        sp.Children.Add(new TextBlock
        {
            Text = Loc.T("小提示：输入 r 后按空格再输入 --，可以看到审查模板的全部参数；面板里也可以用鼠标点选。托盘菜单里有模板库与设置。",
                "Tip: type r, a space, then -- to see all parameters of the review template. You can also click items in the panel. The tray menu has the template library and settings."),
            FontSize = 12, TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center,
            Foreground = SettingsView.Res("TextFillColorSecondaryBrush"),
        });
        return sp;
    }
}
