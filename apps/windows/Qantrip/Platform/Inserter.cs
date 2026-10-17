using System.Diagnostics;
using System.Runtime.InteropServices;
using static Qantrip.Platform.Win32;

namespace Qantrip.Platform;

/// <summary>
/// 跨应用插入：剪贴板 + 模拟 Ctrl+V（PRD 7.2，对应 macOS 的 Inserter）。
///
/// 流程：把前台还给唤起前的窗口 → 备份剪贴板全部格式 → 写入 Prompt（附加剪贴板历史 / 云剪贴板排除标记）→
/// SendInput Ctrl+V → 延迟后恢复原剪贴板（期间剪贴板被其他程序改动则不覆盖）。
/// 目标窗口无法回到前台、目标以管理员身份运行（UIPI 拦截）或设置为只复制时，降级为"只复制"。
/// </summary>
internal sealed class Inserter
{
    public abstract record Outcome
    {
        public sealed record Pasted : Outcome;
        public sealed record CopiedOnly(string Reason) : Outcome;
    }

    private readonly IntPtr owner;
    public Inserter(IntPtr clipboardOwner) => owner = clipboardOwner;

    public static bool CopyOnlyMode => Settings.CopyOnly;
    private static int PasteDelayMs => Settings.PasteDelayMs;
    private static int RestoreDelayMs => Settings.RestoreDelayMs;
    private static int ExpandIntervalMs => Settings.ExpandPasteIntervalMs;

    /// 在 UI 线程调用；面板应已隐藏。
    public async Task<Outcome> Insert(string text, Foreground.Target? target, bool expand)
    {
        var started = Stopwatch.GetTimestamp();
        var log = new Dictionary<string, object?>
        {
            ["chars"] = text.Length,
            ["lines"] = text.Split('\n').Length,
            ["target"] = target?.Exe,
            ["expand"] = expand,
        };

        Outcome CopyOnly(string reasonKey, string reason)
        {
            ClipboardStore.SetText(owner, text, exclude: false); // 内容就是用户要用的，保留且不标记
            log["mode"] = "copy_only";
            log["reason"] = reasonKey;
            log["total_ms"] = Ms(started);
            Log.Event("insert", log);
            return new Outcome.CopiedOnly(reason);
        }

        if (CopyOnlyMode) return CopyOnly("copy_only_mode", Loc.T("只复制模式", "copy-only mode"));
        if (target is null || !IsWindow(target.Hwnd)) return CopyOnly("no_target", Loc.T("没有可插入的窗口", "no target window"));
        if (Foreground.IsElevated(target.Pid) && !Foreground.SelfElevated)
            return CopyOnly("target_elevated", Loc.T("目标以管理员身份运行", "target runs as administrator"));

        // 把前台还给目标窗口
        bool ok = Foreground.Activate(target.Hwnd, out var method);
        if (!ok) ok = await Foreground.WaitFor(target.Hwnd, 300);
        log["activate"] = method;
        log["activate_ms"] = Ms(started);
        if (!ok)
        {
            var now = Foreground.Capture();
            log["actual"] = now?.Exe;
            return CopyOnly("target_not_foreground", Loc.T("无法切回目标窗口", "could not return to the target window"));
        }
        if (PasteDelayMs > 0) await Task.Delay(PasteDelayMs);

        var t0 = Stopwatch.GetTimestamp();
        var snapshot = ClipboardStore.Snapshot(owner);
        log["snapshot_ms"] = Ms(t0);
        log["snapshot_formats"] = snapshot?.Count ?? -1;
        log["snapshot_bytes"] = snapshot?.Sum(i => (long)i.Data.Length) ?? 0;

        uint ours = ClipboardStore.SetText(owner, text, exclude: true);
        if (ours == 0) return CopyOnly("clipboard_busy", Loc.T("剪贴板被占用", "clipboard busy"));

        // 发送前确认目标仍在前台
        if (GetForegroundWindow() != target.Hwnd)
        {
            if (snapshot != null) ClipboardStore.Restore(owner, snapshot);
            return CopyOnly("target_changed", Loc.T("目标窗口已切换", "target window changed"));
        }
        uint sent = SendCtrlV();
        log["paste_ms"] = Ms(started);
        log["sendinput"] = sent;
        if (expand)
        {
            await Task.Delay(ExpandIntervalMs);
            // 期间剪贴板被改动则不再粘贴第二次，避免插入别的内容
            if (GetClipboardSequenceNumber() == ours) SendCtrlV();
        }

        await Task.Delay(RestoreDelayMs);
        bool untouched = GetClipboardSequenceNumber() == ours;
        bool restored = untouched && snapshot != null && ClipboardStore.Restore(owner, snapshot);
        log["restored"] = restored;
        log["mode"] = "paste";
        log["total_ms"] = Ms(started);
        Log.Event("insert", log);
        if (Settings.DebugHooks) File.WriteAllText(Path.Combine(Log.Dir, "last-insert.txt"), text);
        return new Outcome.Pasted();
    }

    /// <summary>
    /// 发送 Ctrl+V。使用虚拟键码 VK_V：Ctrl 组合键按虚拟键匹配，与键盘布局（含 Dvorak）无关。
    /// 用户仍按着的 Shift / Alt / Win 会和我们的按键组合成别的快捷键，先补发抬起。
    /// </summary>
    private static uint SendCtrlV()
    {
        var inputs = new List<INPUT>();
        foreach (var vk in new ushort[] { VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN })
            if ((GetAsyncKeyState(vk) & 0x8000) != 0) inputs.Add(Key(vk, up: true));
        bool ctrlHeld = (GetAsyncKeyState(VK_CONTROL) & 0x8000) != 0;
        if (!ctrlHeld) inputs.Add(Key(VK_CONTROL, false));
        inputs.Add(Key(VK_V, false));
        inputs.Add(Key(VK_V, true));
        if (!ctrlHeld) inputs.Add(Key(VK_CONTROL, true));
        return SendInput((uint)inputs.Count, inputs.ToArray(), Marshal.SizeOf<INPUT>());
    }

    private static double Ms(long since) => Math.Round(Stopwatch.GetElapsedTime(since).TotalMilliseconds, 1);
}
