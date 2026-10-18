using System.Diagnostics;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Qantrip.Core;
using Qantrip.Platform;
using Qantrip.UI;
using Windows.Graphics;

namespace Qantrip;

/// <summary>
/// 应用入口与控制器：引擎、全局热键、托盘图标、面板的显示 / 隐藏 / 插入（对应 macOS 的 main.swift + LauncherController）。
/// </summary>
public partial class App : Application
{
    private Engine? engine;
    private LauncherModel? model;
    private PanelWindow? panel;
    private MessageWindow? messages;
    private HotKey? hotKey;
    private TrayIcon? tray;
    private Inserter? inserter;
    private LibraryModel? library;
    private LibraryWindow? libraryWindow;
    private OnboardingWindow? onboarding;
    private FileSystemWatcher? watcher;
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? reloadTimer;
    private Hud? hud;
    private ForegroundWatcher? foreground;
    private bool submitting;
    /// 在 Qantrip 自己的窗口（引导页的练习框、模板库的编辑器、设置中的输入框）里唤起面板时，那个窗口与当时的焦点控件
    private OwnTarget? ownTarget;

    private sealed record OwnTarget(Window Window, IntPtr Hwnd, WeakReference<DependencyObject>? Focused);

    public Engine? Engine => engine;
    public static new App? Current => Application.Current as App;

    public App()
    {
        InitializeComponent();
        UnhandledException += (_, e) => { Log.Error("unhandled", e.Exception); e.Handled = true; };
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        var sw = Stopwatch.StartNew();
        var ui = Settings.UiLocale;
        Loc.En = ui == "en";
        Directory.CreateDirectory(Settings.DataDir);
        engine = new Engine(Settings.TemplatesDir, Path.Combine(Settings.DataDir, "usage.json"));
        engine.SetLocale(ui, Settings.TemplateLocale);
        var tEngine = sw.ElapsedMilliseconds;

        model = new LauncherModel(engine);
        panel = new PanelWindow(model);
        model.OnSubmit = (text, expand) => _ = Submit(text, expand);
        model.OnCancel = () => Hide(restoreFocus: true);
        model.OnCommand = FinishCommand;
        model.ReadClipboard = () => messages is null ? null : ClipboardStore.ReadText(messages.Handle);
        model.OnOpenLibrary = () => OpenFromPanel(() => ShowLibrary(null));
        model.OnOpenSettings = () => OpenFromPanel(() => ShowSettings("general"));
        panel.Activated += (_, e) =>
        {
            // 点击面板外部（面板失去激活）时关闭，行为与 Spotlight / PowerToys Run 一致
            if (e.WindowActivationState == WindowActivationState.Deactivated && panel.IsShown && !submitting) Hide(restoreFocus: false);
        };
        var tPanel = sw.ElapsedMilliseconds;

        messages = new MessageWindow();
        inserter = new Inserter(messages.Handle);
        messages.ShowRequested += () => Show("second_instance", -1);
        messages.SettingsRequested += () => ShowSettings("general");
        messages.QuitRequested += Quit;
        messages.LibraryRequested += () => ShowLibrary(null);
        hotKey = new HotKey(messages);
        hotKey.Pressed += time =>
        {
            int delay = Environment.TickCount - time; // GetMessageTime 与 TickCount 同一时间基准
            onboarding?.HotKeyPressed();
            if (panel.IsShown) Hide(restoreFocus: true); else Show("hotkey", delay);
        };
        var (mods, vk) = HotKeySpec.Parse(Settings.HotKey);
        hotKey.Register(mods, vk);

        var icon = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        tray = new TrayIcon(messages, icon, $"Nekro Qantrip · {hotKey.Describe()}")
        {
            OpenLabel = () => Loc.T("打开面板", "Open panel") + (hotKey.IsRegistered ? $"\t{hotKey.Describe()}" : ""),
            SettingsLabel = () => Loc.T("设置…", "Settings…"),
            LibraryLabel = () => Loc.T("模板库…", "Template Library…"),
            GuideLabel = () => Loc.T("使用引导…", "Welcome Guide…"),
            SyncLabel = () => SyncService.Shared.PendingJoinCount > 0 ? Loc.T("有设备请求加入同步…", "A device wants to join sync…") : null,
            UpdateLabel = () => UpdateChecker.Shared.Visible is { } r ? Loc.T($"有新版本 {r.Version}…", $"Update Available: {r.Version}…") : null,
            ExclusionLabel = TrayExclusionLabel,
            QuitLabel = () => Loc.T("退出 Nekro Qantrip", "Quit Nekro Qantrip"),
        };
        tray.Open += () => Show("tray", -1);
        tray.OpenSettings += () => ShowSettings("general");
        tray.OpenUpdate += () => { if (UpdateChecker.Shared.Visible is { } r) UpdateChecker.OpenReleasePage(r); };
        tray.OpenLibrary += () => ShowLibrary(null);
        tray.OpenGuide += ShowOnboarding;
        tray.OpenSync += ActivateSettings;
        tray.ToggleExclusion += ToggleExclusion;
        tray.Quit += Quit;
        Settings.Changed += OnSettingChanged;
        if (!hotKey.IsRegistered)
            tray.Notify("Nekro Qantrip", Loc.T($"快捷键 {hotKey.Describe()} 已被其他程序占用（错误 {hotKey.LastError}），可从托盘图标打开面板",
                $"Shortcut {hotKey.Describe()} is taken by another program (error {hotKey.LastError}). Open the panel from the tray icon."));

        Log.Event("launch", new()
        {
            ["core"] = Engine.CoreVersion,
            ["locale"] = ui,
            ["hotkey"] = hotKey.Describe(),
            ["hotkey_ok"] = hotKey.IsRegistered,
            ["hotkey_error"] = hotKey.LastError,
            ["engine_ms"] = tEngine,
            ["panel_ms"] = tPanel,
            ["total_ms"] = sw.ElapsedMilliseconds,
            ["problems"] = engine.Problems().Count,
            ["elevated"] = Foreground.SelfElevated,
            ["tray"] = tray.Added,
            ["settings"] = Settings.Snapshot(),
            ["motion"] = Motion.Enabled,
        });
        Prewarm();
        WatchTemplates();
        // 排除的程序（D35）：前台切到这些程序时暂停快捷键，让按键原样交给程序
        foreground = new ForegroundWatcher();
        foreground.Changed += _ => ApplyExclusion();
        ApplyExclusion();
        // 同步：拉取到新内容后重新加载模板（D29 / sync-design）；启动只读本地状态，已加入时才联网
        SyncService.Shared.OnTemplatesChanged = ReloadTemplates;
        messages.AsleepChanged += SyncService.Shared.SetAsleep;
        SyncService.Shared.SetAsleep(messages.Asleep);
        SyncService.Shared.Start();
        // 更新提醒：只通知，不自动下载或安装
        UpdateChecker.Shared.Found += r => tray?.Notify("Nekro Qantrip",
            Loc.T($"有新版本 {r.Version} 可以下载。点击这里或从托盘菜单打开下载页面。", $"Version {r.Version} is available. Click here or use the tray menu to open the download page."),
            () => UpdateChecker.OpenReleasePage(r));
        UpdateChecker.Shared.Start(panel.DispatcherQueue);
        if (Program.OpenSettingsOnLaunch) ShowSettings("general");
        if (Program.OpenLibraryOnLaunch) ShowLibrary(null);
        if (!Settings.OnboardingDone && Environment.GetEnvironmentVariable("QANTRIP_SKIP_ONBOARDING") != "1") ShowOnboarding();
    }

    // MARK: 模板目录

    /// 模板目录变化（模板库保存、指令、外部编辑器、同步）时重新加载，并在稍后同步。
    private void WatchTemplates()
    {
        try
        {
            Directory.CreateDirectory(Settings.TemplatesDir);
            watcher = new FileSystemWatcher(Settings.TemplatesDir, "*.md")
            {
                NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite | NotifyFilters.Size,
                IncludeSubdirectories = false,
            };
            var ui = Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();
            void Changed(object? s, FileSystemEventArgs e) => ui.TryEnqueue(ScheduleReload);
            watcher.Changed += Changed;
            watcher.Created += Changed;
            watcher.Deleted += Changed;
            watcher.Renamed += (s, e) => Changed(s, e);
            watcher.EnableRaisingEvents = true;
        }
        catch (Exception e) { Log.Error("watch_templates", e); }
    }

    private void ScheduleReload()
    {
        if (panel is null) return;
        reloadTimer ??= panel.DispatcherQueue.CreateTimer();
        reloadTimer.Stop();
        reloadTimer.Interval = TimeSpan.FromMilliseconds(300);
        reloadTimer.IsRepeating = false;
        reloadTimer.Tick -= OnReloadTick;
        reloadTimer.Tick += OnReloadTick;
        reloadTimer.Start();
    }

    private void OnReloadTick(Microsoft.UI.Dispatching.DispatcherQueueTimer t, object e)
    {
        ReloadTemplates();
        SyncService.Shared.ScheduleSync();
    }

    private void ReloadTemplates()
    {
        engine?.Reload();
        if (library != null) library.Reload();
        model?.Refresh();
    }

    /// 启动后离屏完整走一遍显示流程，消除首次唤起时的冷启动开销（对应 macOS prewarm）。
    private void Prewarm()
    {
        if (panel is null || model is null) return;
        var sw = Stopwatch.StartNew();
        panel.AppWindow.Move(new PointInt32(-32000, -32000));
        panel.AppWindow.Show(false);
        foreach (var input in new[] { "r", "r --scope=st", "r security 示例" }) model.TextDidChange(input);
        model.Begin(null);
        void Done(object? s, object e)
        {
            CompositionTarget.Rendering -= Done;
            var t = panel.DispatcherQueue.CreateTimer();
            t.Interval = TimeSpan.FromMilliseconds(200);
            t.IsRepeating = false;
            t.Tick += (_, _) =>
            {
                if (!panel.IsShown || panel.AppWindow.Position.X < -10000) panel.HidePanel();
                Log.Event("prewarm", new() { ["ms"] = sw.ElapsedMilliseconds });
            };
            t.Start();
        }
        CompositionTarget.Rendering += Done;
    }

    private void Show(string source, int eventDelayMs)
    {
        if (panel is null || model is null) return;
        var sw = Stopwatch.StartNew();
        var target = Foreground.Capture();
        if (target != null && (target.Hwnd == panel.Hwnd || target.Hwnd == messages?.Handle)) target = null;
        ownTarget = CaptureOwn(target);
        panel.PrepareEntrance();
        model.Begin(target);
        var tModel = sw.Elapsed.TotalMilliseconds;
        panel.PlaceOn(target?.Hwnd ?? IntPtr.Zero);
        bool ok = panel.ShowActivated(out var method);
        var tShown = sw.Elapsed.TotalMilliseconds;
        // 距上次检查足够久时在后台检查一次同步（核心按策略决定，面板不等待）
        SyncService.Shared.PanelOpened();
        var fields = new Dictionary<string, object?>
        {
            ["source"] = source,
            ["target"] = target?.Exe,
            ["event_to_handler_ms"] = eventDelayMs,
            ["model_ms"] = Math.Round(tModel, 1),
            ["handler_to_shown_ms"] = Math.Round(tShown, 1),
            ["foreground_ok"] = ok,
            ["foreground_method"] = method,
            ["ime"] = panel.ImeAction,
            ["own_window"] = ownTarget is not null,
        };
        // 第一帧渲染完成的时间（含 XAML 布局与合成提交，不含显示器扫描输出）
        void Frame(object? s, object e)
        {
            CompositionTarget.Rendering -= Frame;
            fields["handler_to_frame_ms"] = Math.Round(sw.Elapsed.TotalMilliseconds, 1);
            fields["foreground_is_panel"] = Win32.GetForegroundWindow() == panel.Hwnd;
            Log.Event("show", fields);
        }
        CompositionTarget.Rendering += Frame;
    }

    /// 关闭面板。`restoreFocus`：把前台还给唤起前的窗口（Esc / 再按快捷键）；点击外部关闭时不切换。
    private void Hide(bool restoreFocus)
    {
        if (panel is null || model is null || !panel.IsShown) return;
        DebugQuery("hide");
        var own = ownTarget;
        ownTarget = null;
        submitting = true; // 切换前台会触发面板失去激活，避免重入
        try
        {
            if (restoreFocus && model.Target is { } t && Win32.IsWindow(t.Hwnd)) Foreground.Activate(t.Hwnd, out _);
            panel.HidePanel();
        }
        finally { submitting = false; }
        // 在自己的窗口里唤起：关闭面板后把键盘焦点还给原来的输入框
        if (restoreFocus && own != null) own.Window.DispatcherQueue.TryEnqueue(() => RestoreFocus(own));
    }

    /// 前台窗口是 Qantrip 自己的窗口（面板与隐藏的消息窗口除外）时，记下它与其中的焦点控件。
    private OwnTarget? CaptureOwn(Foreground.Target? target)
    {
        if (target is null || target.Pid != (uint)Environment.ProcessId) return null;
        Window? window = null;
        foreach (var w in new Window?[] { libraryWindow, onboarding })
            if (w != null && WinRT.Interop.WindowNative.GetWindowHandle(w) == target.Hwnd) window = w;
        if (window?.Content?.XamlRoot is not { } root) return null;
        var focused = Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(root) as DependencyObject;
        return new OwnTarget(window, target.Hwnd, focused is null ? null : new WeakReference<DependencyObject>(focused));
    }

    /// 把焦点还给唤起面板前的控件（窗口已回到前台），返回该控件。
    private static DependencyObject? RestoreFocus(OwnTarget own)
    {
        if (own.Focused is null || !own.Focused.TryGetTarget(out var element)) return null;
        if (element is UIElement { XamlRoot: null }) return null; // 已不在界面中（例如页面重建）
        if (element is Microsoft.UI.Xaml.Controls.Control c) c.Focus(FocusState.Programmatic);
        return element;
    }

    /// <summary>
    /// 插入到 Qantrip 自己的窗口：恢复前台与焦点后直接写入输入框（不模拟粘贴、不经过剪贴板，对应 macOS insertIntoOwnWindow）。
    /// 焦点不在可编辑的输入框上时只复制到剪贴板并提示。
    /// </summary>
    private void InsertIntoOwnWindow(OwnTarget own, string text)
    {
        own.Window.DispatcherQueue.TryEnqueue(() =>
        {
            var element = RestoreFocus(own);
            if (element is null && own.Window.Content?.XamlRoot is { } root)
                element = Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(root) as DependencyObject;
            var normalized = text.Replace("\r\n", "\n");
            string kind;
            switch (element)
            {
                case Microsoft.UI.Xaml.Controls.TextBox { IsReadOnly: false } box:
                {
                    // TextBox 的换行是 \r；单行输入框把换行换成空格
                    var s = box.AcceptsReturn ? normalized.Replace('\n', '\r') : normalized.Replace('\n', ' ');
                    int start = box.SelectionStart;
                    box.SelectedText = s;
                    box.Select(Math.Min(start + s.Length, box.Text.Length), 0);
                    kind = "textbox";
                    break;
                }
                case Microsoft.UI.Xaml.Controls.RichEditBox { IsReadOnly: false } rich:
                    rich.Document.Selection.TypeText(rich.AcceptsReturn ? normalized.Replace('\n', '\r') : normalized.Replace('\n', ' '));
                    kind = "richedit";
                    break;
                default:
                    // 焦点不在可编辑的输入框上：模拟粘贴也无处可去，改为只复制
                    kind = "copy_only";
                    if (messages != null && ClipboardStore.SetText(messages.Handle, text, exclude: false) != 0)
                        (hud ??= new Hud()).Show(Loc.T("已复制到剪贴板，按 Ctrl+V 粘贴", "Copied to clipboard. Press Ctrl+V to paste."));
                    break;
            }
            Log.Event("insert_own", new() { ["control"] = kind, ["element"] = element?.GetType().Name, ["chars"] = text.Length });
            if (Settings.DebugHooks) File.WriteAllText(Path.Combine(Log.Dir, "last-insert.txt"), text);
        });
    }

    /// 面板中的快捷入口（Ctrl+L 模板库、Ctrl+, 设置、顶栏图标）：关闭面板（不把焦点还给原程序）后打开 Qantrip 的窗口。
    /// 先打开窗口再隐藏面板：此时本进程仍在前台，新窗口可以直接取得前台。
    private void OpenFromPanel(Action open)
    {
        if (panel is null) return;
        ownTarget = null;
        submitting = true;
        try
        {
            DebugQuery("open_window");
            open();
            panel.HidePanel();
        }
        finally { submitting = false; }
    }

    private async Task Submit(string text, bool expand)
    {
        if (panel is null || model is null || inserter is null) return;
        var target = model.Target;
        if (ownTarget is { } own)
        {
            ownTarget = null;
            submitting = true;
            try
            {
                DebugQuery("submit");
                Foreground.Activate(own.Hwnd, out _);
                panel.HidePanel();
            }
            finally { submitting = false; }
            InsertIntoOwnWindow(own, text);
            return;
        }
        submitting = true;
        try
        {
            DebugQuery("submit");
            // 趁面板仍在前台时把前台还给目标窗口（前台进程可以自由切换前台），再隐藏面板
            if (target != null && !Settings.CopyOnly) Foreground.Activate(target.Hwnd, out _);
            panel.HidePanel();
        }
        finally { submitting = false; }
        var outcome = await inserter.Insert(text, target, expand);
        if (outcome is Inserter.Outcome.CopiedOnly c)
            tray?.Notify("Nekro Qantrip", Loc.T($"已复制到剪贴板（{c.Reason}），按 Ctrl+V 粘贴", $"Copied to clipboard ({c.Reason}). Press Ctrl+V to paste."));
    }

    /// 调试钩子：记录面板关闭时的输入（回归脚本据此判断输入法是否切换成功）
    private void DebugQuery(string when)
    {
        if (!Settings.DebugHooks || model is null) return;
        Log.Event("debug_query", new() { ["when"] = when, ["query"] = model.Query });
    }

    private void Quit()
    {
        watcher?.Dispose();
        foreground?.Dispose();
        libraryWindow?.Close();
        onboarding?.Close();
        tray?.Dispose();
        hotKey?.Dispose();
        messages?.Dispose();
        hud?.Close();
        panel?.Close();
        engine?.Dispose();
        Exit();
    }

    // MARK: 指令模式（D31）

    /// 指令完成：关闭面板，删除的文件移到回收站，/edit、/dup 在模板库中打开，最后提示。
    private void FinishCommand(CommandOutcome outcome)
    {
        Hide(restoreFocus: true);
        var message = outcome.Message;
        bool trashed = true;
        if (outcome.TrashPath is string path)
        {
            trashed = RecycleBin.Move(path);
            engine?.Reload();
            if (!trashed) message = Loc.T($"无法移到回收站：{Path.GetFileName(path)}", $"Could not move {Path.GetFileName(path)} to the Recycle Bin");
        }
        if (outcome.OpenEditor is string name) ShowLibrary(name);
        switch (outcome.Action)
        {
            case "sync":
                if (SyncService.Shared.Enrolled)
                {
                    SyncService.Shared.SyncNow();
                    message = Loc.T("正在同步…", "Syncing…");
                }
                else ActivateSettings();
                break;
            case "settings":
                ShowSettings("general");
                break;
            case "library":
                ShowLibrary(null);
                break;
            case string a when a.StartsWith("lang:"):
                Settings.TemplateLanguage = a[5..]; // 触发 Settings.Changed → 重新加载内置模板
                break;
        }
        Log.Event("command", new() { ["open_editor"] = outcome.OpenEditor is not null, ["trash"] = outcome.TrashPath is not null, ["trashed"] = trashed });
        if (Settings.DebugHooks) Log.Event("debug_command", new() { ["message"] = message, ["trash_path"] = outcome.TrashPath, ["open_editor"] = outcome.OpenEditor });
        if (!string.IsNullOrEmpty(message)) (hud ??= new Hud()).Show(message);
    }

    // MARK: 窗口

    /// 设置与模板库在同一个窗口中（与 macOS 相同）：打开该窗口的某个设置页（general / insert / sync / privacy / about）。
    public void ShowSettings(string page)
    {
        if (LibraryWindowReady() is not { } w) return;
        w.ShowSettings(page);
        w.ShowAndActivate();
        Log.Event("settings_open", new() { ["page"] = page });
    }

    /// 打开设置 › 同步并置于前台（浏览器登录完成、托盘"有设备请求加入"、超出设备上限、未加入时的 /sync）。
    public void ActivateSettings() => ShowSettings("sync");

    /// 打开模板库；`name` 不为空时选中该模板（/edit、/dup）。窗口正显示设置时回到模板。
    public void ShowLibrary(string? name)
    {
        if (LibraryWindowReady() is not { } w) return;
        if (name != null) w.SelectTemplate(name);
        else w.ShowTemplates();
        w.ShowAndActivate();
        Log.Event("library_open", new() { ["select"] = name != null });
    }

    private LibraryWindow? LibraryWindowReady()
    {
        if (engine is null) return null;
        library ??= new LibraryModel(engine);
        library.Reload();
        if (libraryWindow is null)
        {
            libraryWindow = new LibraryWindow(library, this);
            libraryWindow.Closed += (_, _) => libraryWindow = null;
        }
        return libraryWindow;
    }

    private void ShowOnboarding()
    {
        if (onboarding is null)
        {
            onboarding = new OnboardingWindow(this);
            onboarding.Closed += (_, _) => onboarding = null;
        }
        onboarding.ShowAndActivate();
    }

    private void OnSettingChanged(string key)
    {
        switch (key)
        {
            case "language" or "templateLanguage":
                Loc.En = Settings.UiLocale == "en";
                engine?.SetLocale(Settings.UiLocale, Settings.TemplateLocale);
                model?.Refresh();
                panel?.ApplyLanguage();
                library?.Reload();
                libraryWindow?.ApplyLanguage();
                break;
            case "forceAsciiInput":
                panel?.ApplyInputScope();
                break;
            case "expandPasteEnabled":
                panel?.ApplyLanguage();
                break;
            case "excludedApps":
                ApplyExclusion();
                libraryWindow?.RefreshExcludedApps();
                break;
        }
        Log.Event("setting", new() { ["key"] = key });
    }

    public bool HotKeyRegistered => hotKey?.IsRegistered ?? false;
    public int HotKeyError => hotKey?.LastError ?? 0;
    public string HotKeyText => hotKey?.Describe() ?? Settings.HotKey;

    /// 录制新快捷键期间暂停全局热键（否则按下当前组合会唤起面板）。
    public void SuspendHotKey() => hotKey?.Unregister();

    public void ResumeHotKey()
    {
        if (hotKey is null || hotKey.IsRegistered) return;
        var (mods, vk) = HotKeySpec.Parse(Settings.HotKey);
        hotKey.Register(mods, vk);
        ApplyExclusion();
    }

    // MARK: 排除的程序（D35）

    /// 前台程序在排除列表中时暂停快捷键（本程序的窗口除外）。从托盘显式打开面板不受影响。
    private void ApplyExclusion()
    {
        if (hotKey is null || foreground is null) return;
        bool pause = Settings.IsExcluded(foreground.Current);
        if (pause == hotKey.Paused) return;
        hotKey.SetPaused(pause);
        Log.Event("hotkey_paused", new() { ["paused"] = hotKey.Paused, ["registered"] = hotKey.IsRegistered });
    }

    /// 托盘菜单项：针对打开托盘菜单之前用户正在使用的程序（点击托盘时前台已经变成任务栏）。
    private string? TrayExclusionLabel()
    {
        if (foreground?.LastExternal is not { } path) return null;
        var name = Apps.DisplayName(path).Replace("&", "&&"); // 菜单文字中 & 是助记符
        return Settings.IsExcluded(path)
            ? Loc.T($"在「{name}」中恢复快捷键", $"Re-enable Shortcut in “{name}”")
            : Loc.T($"在「{name}」中停用快捷键", $"Disable Shortcut in “{name}”");
    }

    private void ToggleExclusion()
    {
        if (foreground?.LastExternal is not { } path) return;
        bool exclude = !Settings.IsExcluded(path);
        Settings.SetExcluded(path, Apps.DisplayName(path), exclude);
        Log.Event("exclusion_toggle", new() { ["excluded"] = exclude });
    }

    /// 改用新的快捷键：注册成功才保存；失败（被其他程序占用）时恢复原快捷键并返回错误码。
    public bool TrySetHotKey(uint mods, uint vk, out int error)
    {
        error = 0;
        if (hotKey is null) return false;
        if (hotKey.Register(mods, vk))
        {
            Settings.HotKey = HotKeySpec.Format(mods, vk);
            tray?.SetTip($"Nekro Qantrip · {hotKey.Describe()}");
            return true;
        }
        error = hotKey.LastError;
        var (m, k) = HotKeySpec.Parse(Settings.HotKey);
        hotKey.Register(m, k);
        return false;
    }
}
