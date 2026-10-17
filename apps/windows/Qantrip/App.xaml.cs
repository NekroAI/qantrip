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
    private SettingsWindow? settingsWindow;
    private Hud? hud;
    private bool submitting;

    public Engine? Engine => engine;

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
        panel.Activated += (_, e) =>
        {
            // 点击面板外部（面板失去激活）时关闭，行为与 Spotlight / PowerToys Run 一致
            if (e.WindowActivationState == WindowActivationState.Deactivated && panel.IsShown && !submitting) Hide(restoreFocus: false);
        };
        var tPanel = sw.ElapsedMilliseconds;

        messages = new MessageWindow();
        inserter = new Inserter(messages.Handle);
        messages.ShowRequested += () => Show("second_instance", -1);
        messages.SettingsRequested += ShowSettings;
        hotKey = new HotKey(messages);
        hotKey.Pressed += time =>
        {
            int delay = Environment.TickCount - time; // GetMessageTime 与 TickCount 同一时间基准
            if (panel.IsShown) Hide(restoreFocus: true); else Show("hotkey", delay);
        };
        var (mods, vk) = HotKeySpec.Parse(Settings.HotKey);
        hotKey.Register(mods, vk);

        var icon = Path.Combine(AppContext.BaseDirectory, "Assets", "Qantrip.ico");
        tray = new TrayIcon(messages, icon, $"Nekro Qantrip · {hotKey.Describe()}")
        {
            OpenLabel = () => Loc.T("打开面板", "Open panel") + (hotKey.IsRegistered ? $"\t{hotKey.Describe()}" : ""),
            SettingsLabel = () => Loc.T("设置…", "Settings…"),
            QuitLabel = () => Loc.T("退出 Nekro Qantrip", "Quit Nekro Qantrip"),
        };
        tray.Open += () => Show("tray", -1);
        tray.OpenSettings += ShowSettings;
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
        if (Program.OpenSettingsOnLaunch) ShowSettings();
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
        panel.PrepareEntrance();
        model.Begin(target);
        var tModel = sw.Elapsed.TotalMilliseconds;
        panel.PlaceOn(target?.Hwnd ?? IntPtr.Zero);
        bool ok = panel.ShowActivated(out var method);
        var tShown = sw.Elapsed.TotalMilliseconds;
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
        submitting = true; // 切换前台会触发面板失去激活，避免重入
        try
        {
            if (restoreFocus && model.Target is { } t && Win32.IsWindow(t.Hwnd)) Foreground.Activate(t.Hwnd, out _);
            panel.HidePanel();
        }
        finally { submitting = false; }
    }

    private async Task Submit(string text, bool expand)
    {
        if (panel is null || model is null || inserter is null) return;
        var target = model.Target;
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
        tray?.Dispose();
        hotKey?.Dispose();
        messages?.Dispose();
        settingsWindow?.Close();
        hud?.Close();
        panel?.Close();
        engine?.Dispose();
        Exit();
    }

    // MARK: 指令模式（D31）

    /// 指令完成：关闭面板，删除的文件移到回收站，最后提示（模板库窗口尚未实现，/edit 只提示）。
    private void FinishCommand(CommandOutcome outcome)
    {
        Hide(restoreFocus: true);
        // 核心的提示沿用 macOS 的"废纸篓"（与"回收站"等长，替换不影响高亮位置）
        var message = outcome.Message.Replace("废纸篓", "回收站").Replace("the Trash", "the Recycle Bin");
        bool trashed = true;
        if (outcome.TrashPath is string path)
        {
            trashed = RecycleBin.Move(path);
            engine?.Reload();
            if (!trashed) message = Loc.T($"无法移到回收站：{Path.GetFileName(path)}", $"Could not move {Path.GetFileName(path)} to the Recycle Bin");
        }
        if (outcome.OpenEditor is string name && string.IsNullOrEmpty(message))
            message = Loc.T($"Windows 版还没有模板库窗口，{name} 可在模板文件夹中编辑", $"The template library is not available on Windows yet. Edit {name} in the templates folder");
        Log.Event("command", new() { ["open_editor"] = outcome.OpenEditor is not null, ["trash"] = outcome.TrashPath is not null, ["trashed"] = trashed });
        if (Settings.DebugHooks) Log.Event("debug_command", new() { ["message"] = message, ["trash_path"] = outcome.TrashPath, ["open_editor"] = outcome.OpenEditor });
        if (!string.IsNullOrEmpty(message)) (hud ??= new Hud()).Show(message);
    }

    // MARK: 设置

    private void ShowSettings()
    {
        if (settingsWindow is null)
        {
            settingsWindow = new SettingsWindow(this);
            settingsWindow.Closed += (_, _) => settingsWindow = null;
        }
        settingsWindow.ShowAndActivate();
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
                settingsWindow?.Rebuild();
                break;
            case "forceAsciiInput":
                panel?.ApplyInputScope();
                break;
            case "expandPasteEnabled":
                panel?.ApplyLanguage();
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
