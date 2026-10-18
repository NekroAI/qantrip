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
using Windows.Storage.Pickers;
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
    private NavigationView? nav;
    private HotKeyRecorder? recorder;
    private SyncPage? syncPage;
    /// 通用页中"排除的程序"列表的容器（列表变化时只重建它）
    private StackPanel? excludedHost;
    /// 当前页面：general / sync
    private string page = "general";

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
        Closed += (_, _) =>
        {
            recorder?.Cancel();
            syncPage?.Detach();
        };
        Build();
    }

    internal static Brush Res(string key) => (Brush)Application.Current.Resources[key];

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
        int w = (int)Math.Round(900 * scale), h = (int)Math.Min(Math.Round(820 * scale), (wa.Bottom - wa.Top) * 0.9);
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

    /// 切换到指定页面（general / sync）。
    public void ShowPage(string name)
    {
        if (nav is null) return;
        foreach (var item in nav.MenuItems.OfType<NavigationViewItem>())
            if ((string)item.Tag == name) { nav.SelectedItem = item; break; }
    }

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

        scroll = new ScrollViewer { HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
        AutomationProperties.SetAutomationId(scroll, "settings_scroll");
        nav = new NavigationView
        {
            PaneDisplayMode = NavigationViewPaneDisplayMode.Left, IsSettingsVisible = false, IsBackButtonVisible = NavigationViewBackButtonVisible.Collapsed,
            IsPaneToggleButtonVisible = false, OpenPaneLength = 200, Content = scroll, IsTitleBarAutoPaddingEnabled = false,
        };
        AutomationProperties.SetAutomationId(nav, "settingsNav");
        nav.MenuItems.Add(NavItem("general", Loc.T("通用", "General"), "\uE713"));
        nav.MenuItems.Add(NavItem("sync", Loc.T("同步", "Sync"), "\uE895"));
        nav.SelectionChanged += (_, e) =>
        {
            if (e.SelectedItem is NavigationViewItem { Tag: string tag }) ShowContent(tag);
        };
        Grid.SetRow(nav, 1);
        root.Children.Add(nav);
        ShowPage(page);
    }

    private static NavigationViewItem NavItem(string tag, string text, string glyph)
    {
        var item = new NavigationViewItem { Content = text, Tag = tag, Icon = new FontIcon { Glyph = glyph, FontFamily = Theme.Icons } };
        AutomationProperties.SetAutomationId(item, "nav_" + tag);
        return item;
    }

    private void ShowContent(string name)
    {
        if (scroll is null) return;
        page = name;
        recorder?.Cancel();
        syncPage?.Detach();
        syncPage = null;
        if (name == "sync")
        {
            syncPage = new SyncPage(root);
            scroll.Content = syncPage.View;
            syncPage.Attach();
        }
        else scroll.Content = General();
        Log.Event("settings_page", new() { ["page"] = name });
    }

    private FrameworkElement General()
    {
        var page = new StackPanel { Spacing = 4, MaxWidth = 860, Padding = new Thickness(28, 8, 28, 28) };
        page.Children.Add(new TextBlock { Text = Loc.T("通用", "General"), FontFamily = Theme.Display, FontSize = 30, Margin = new Thickness(0, 0, 0, 12) });

        // 通用
        page.Children.Add(Section(Loc.T("通用", "General")));
        page.Children.Add(Card("", Loc.T("唤起快捷键", "Panel shortcut"),
            Loc.T("在任意程序中按下即可唤起命令面板。默认 Ctrl+Q。", "Press it in any app to open the command panel. The default is Ctrl+Q."), (recorder = new HotKeyRecorder(app)).View));
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

        // 排除的程序（D35）
        page.Children.Add(Section(Loc.T("排除的程序", "Excluded apps")));
        excludedHost = new StackPanel { Spacing = 4 };
        AutomationProperties.SetAutomationId(excludedHost, "excludedApps");
        page.Children.Add(excludedHost);
        BuildExcludedApps();

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
        return page;
    }

    // MARK: 排除的程序（D35）

    /// 重建排除列表（设置或托盘菜单修改 excludedApps 后调用）。推迟到当前事件处理结束，避免在按钮 / 菜单的事件中移除它自己。
    public void RefreshExcludedApps() => DispatcherQueue.TryEnqueue(BuildExcludedApps);

    private void BuildExcludedApps()
    {
        if (excludedHost is null) return;
        excludedHost.Children.Clear();
        foreach (var app in Settings.ExcludedApps) excludedHost.Children.Add(ExcludedRow(app));

        var menu = new MenuFlyout();
        menu.Opening += (_, _) => FillAddMenu(menu);
        var add = new DropDownButton { Content = Loc.T("添加程序…", "Add app…"), Flyout = menu };
        AutomationProperties.SetAutomationId(add, "excludedAdd");
        var text = new TextBlock
        {
            Text = Loc.T("在这些程序里按快捷键不会唤起 Qantrip，按键照常交给程序本身（例如 Outlook 的 Ctrl+Q）。也可以在托盘菜单中一键停用当前程序；从托盘打开面板不受影响。",
                "In these apps the shortcut doesn't open Qantrip and goes to the app as usual (for example Ctrl+Q in Outlook). You can also disable the current app from the tray menu; opening the panel from the tray still works."),
            FontSize = 12, TextWrapping = TextWrapping.Wrap, Foreground = Res("TextFillColorSecondaryBrush"), VerticalAlignment = VerticalAlignment.Center,
        };
        var grid = new Grid { ColumnSpacing = 16 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.Children.Add(text);
        Grid.SetColumn(add, 1);
        add.VerticalAlignment = VerticalAlignment.Center;
        grid.Children.Add(add);
        excludedHost.Children.Add(CardBorder(grid));
    }

    private static Border CardBorder(UIElement child) => new()
    {
        Child = child, Padding = new Thickness(16, 12, 16, 12), CornerRadius = new CornerRadius(6), BorderThickness = new Thickness(1),
        Background = Res("CardBackgroundFillColorDefaultBrush"), BorderBrush = Res("CardStrokeColorDefaultBrush"),
    };

    /// 一个排除的程序：图标 | 名称 + 路径 | 移除
    private FrameworkElement ExcludedRow(ExcludedApp app)
    {
        var grid = new Grid { ColumnSpacing = 16 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var icon = new Grid { Width = 24, Height = 24, VerticalAlignment = VerticalAlignment.Center };
        icon.Children.Add(new FontIcon { Glyph = "\uECAA", FontSize = 18, FontFamily = Theme.Icons });
        var image = new Image { Width = 24, Height = 24 };
        icon.Children.Add(image);
        LoadIcon(image, app.Path);
        grid.Children.Add(icon);
        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center, Spacing = 2 };
        text.Children.Add(new TextBlock { Text = app.Name, TextTrimming = TextTrimming.CharacterEllipsis });
        var path = new TextBlock { Text = app.Path, FontSize = 11, TextTrimming = TextTrimming.CharacterEllipsis, Foreground = Res("TextFillColorSecondaryBrush") };
        ToolTipService.SetToolTip(path, app.Path);
        text.Children.Add(path);
        Grid.SetColumn(text, 1);
        grid.Children.Add(text);
        var remove = new Button { Content = Loc.T("移除", "Remove"), VerticalAlignment = VerticalAlignment.Center };
        AutomationProperties.SetAutomationId(remove, "excludedRemove_" + Path.GetFileNameWithoutExtension(app.Path));
        AutomationProperties.SetName(remove, Loc.T($"移除 {app.Name}", $"Remove {app.Name}"));
        remove.Click += (_, _) => Settings.SetExcluded(app.Path, app.Name, false); // Settings.Changed → RefreshExcludedApps
        Grid.SetColumn(remove, 2);
        grid.Children.Add(remove);
        return CardBorder(grid);
    }

    /// 程序图标（外壳缩略图）；文件不存在或读取失败时保留默认图标。
    private static async void LoadIcon(Image image, string path)
    {
        try
        {
            var file = await Windows.Storage.StorageFile.GetFileFromPathAsync(path);
            using var thumb = await file.GetThumbnailAsync(Windows.Storage.FileProperties.ThumbnailMode.SingleItem, 32);
            if (thumb is null) return;
            var bmp = new BitmapImage();
            await bmp.SetSourceAsync(thumb);
            image.Source = bmp;
        }
        catch { }
    }

    /// "添加程序"菜单：有可见窗口的正在运行的程序 + 浏览 .exe
    private void FillAddMenu(MenuFlyout menu)
    {
        menu.Items.Clear();
        List<(string Path, string Name)> running;
        try { running = Apps.WithVisibleWindows().Where(a => !Settings.IsExcluded(a.Path)).ToList(); }
        catch (Exception e) { Log.Error("running_apps", e); running = new(); }
        if (running.Count == 0)
            menu.Items.Add(new MenuFlyoutItem { Text = Loc.T("没有其他正在运行的程序", "No other running apps"), IsEnabled = false });
        foreach (var (path, name) in running)
        {
            var item = new MenuFlyoutItem { Text = name };
            ToolTipService.SetToolTip(item, path);
            item.Click += (_, _) => Settings.SetExcluded(path, name, true);
            menu.Items.Add(item);
        }
        menu.Items.Add(new MenuFlyoutSeparator());
        var browse = new MenuFlyoutItem { Text = Loc.T("浏览程序文件…", "Browse for a program…"), Icon = new FontIcon { Glyph = "\uE8DA", FontFamily = Theme.Icons } };
        AutomationProperties.SetAutomationId(browse, "excludedBrowse");
        browse.Click += async (_, _) =>
        {
            try
            {
                var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.ComputerFolder };
                picker.FileTypeFilter.Add(".exe");
                WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);
                var file = await picker.PickSingleFileAsync();
                if (file is null) return;
                Settings.SetExcluded(file.Path, Apps.DisplayName(file.Path), true);
            }
            catch (Exception e) { Log.Error("exclude_browse", e); }
        };
        menu.Items.Add(browse);
    }

    internal static TextBlock Section(string title) => new()
    {
        Text = title, FontSize = 14, FontWeight = FontWeights.SemiBold, Margin = new Thickness(2, 18, 0, 6),
    };

    /// 设置卡片：图标 | 标题 + 说明 | 控件（Windows 设置的样式）。
    internal static FrameworkElement Card(string glyph, string title, string? description, FrameworkElement control)
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

    // MARK: 关于

    internal static string Version
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
