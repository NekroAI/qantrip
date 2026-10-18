using System.Diagnostics;
using static Qantrip.Platform.Win32;

namespace Qantrip.Platform;

/// <summary>
/// 程序（按 exe 完整路径识别）：窗口所属程序的路径、显示名称、有可见窗口的正在运行的程序。用于排除的程序（D35）。
/// </summary>
internal static class Apps
{
    private static readonly Dictionary<string, string> names = new(StringComparer.OrdinalIgnoreCase);

    /// 进程的 exe 完整路径；无法查询时为 null。
    public static string? ImagePath(uint pid)
    {
        var p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (p == IntPtr.Zero) return null;
        try
        {
            var buf = new char[1024];
            int n = buf.Length;
            return QueryFullProcessImageName(p, 0, buf, ref n) ? new string(buf, 0, n) : null;
        }
        finally { CloseHandle(p); }
    }

    /// 窗口所属的进程。UWP 应用的顶层窗口属于 ApplicationFrameHost.exe，此时取其中 CoreWindow 所属的应用进程。
    public static uint ProcessOf(IntPtr hwnd)
    {
        GetWindowThreadProcessId(hwnd, out uint pid);
        if (ImagePath(pid) is { } path && Path.GetFileName(path).Equals("ApplicationFrameHost.exe", StringComparison.OrdinalIgnoreCase))
        {
            var core = FindWindowEx(hwnd, IntPtr.Zero, "Windows.UI.Core.CoreWindow", null);
            if (core != IntPtr.Zero)
            {
                GetWindowThreadProcessId(core, out uint inner);
                if (inner != 0) return inner;
            }
        }
        return pid;
    }

    public static string? PathOf(IntPtr hwnd) => hwnd == IntPtr.Zero ? null : ImagePath(ProcessOf(hwnd));

    /// 显示名称：文件说明 → 产品名称 → 文件名（不含扩展名）。
    public static string DisplayName(string path)
    {
        lock (names)
        {
            if (names.TryGetValue(path, out var cached)) return cached;
        }
        string name = Path.GetFileNameWithoutExtension(path);
        try
        {
            var v = FileVersionInfo.GetVersionInfo(path);
            if (!string.IsNullOrWhiteSpace(v.FileDescription)) name = v.FileDescription.Trim();
            else if (!string.IsNullOrWhiteSpace(v.ProductName)) name = v.ProductName.Trim();
        }
        catch { }
        lock (names) names[path] = name;
        return name;
    }

    private static string ClassOf(IntPtr hwnd)
    {
        var buf = new char[256];
        int n = GetClassName(hwnd, buf, buf.Length);
        return n > 0 ? new string(buf, 0, n) : "";
    }

    /// 任务栏、通知区域溢出、桌面等外壳窗口：点击托盘图标时前台会先切到这些窗口，不当作"用户正在使用的程序"。
    public static bool IsShellSurface(IntPtr hwnd) => ClassOf(hwnd) is "Shell_TrayWnd" or "Shell_SecondaryTrayWnd" or "NotifyIconOverflowWindow"
        or "TopLevelWindowForOverflowXamlIsland" or "Progman" or "WorkerW" or "Windows.UI.Core.CoreWindow" or "XamlExplorerHostIslandWindow";

    /// 有可见顶层窗口的程序（不含本程序），按名称排序。
    public static List<(string Path, string Name)> WithVisibleWindows()
    {
        var self = (uint)Environment.ProcessId;
        var paths = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        EnumWindows((h, _) =>
        {
            if (!IsWindowVisible(h) || GetWindow(h, GW_OWNER) != IntPtr.Zero) return true;
            if ((GetWindowLongPtr(h, GWL_EXSTYLE).ToInt64() & WS_EX_TOOLWINDOW) != 0) return true;
            if (DwmGetWindowAttribute(h, DWMWA_CLOAKED, out int cloaked, 4) == 0 && cloaked != 0) return true;
            if (Foreground.Title(h).Length == 0 || IsShellSurface(h)) return true;
            var pid = ProcessOf(h);
            if (pid == self) return true;
            if (ImagePath(pid) is { } p) paths.Add(p);
            return true;
        }, IntPtr.Zero);
        return paths.Select(p => (p, DisplayName(p))).OrderBy(a => a.Item2, StringComparer.CurrentCultureIgnoreCase).ToList();
    }
}

/// <summary>
/// 跟踪前台窗口（SetWinEventHook EVENT_SYSTEM_FOREGROUND，进程外回调，在创建它的 UI 线程上分发）。
/// 记住最后一个"外部程序"（不是本程序、也不是任务栏等外壳窗口），供托盘菜单的"在「X」中停用快捷键"使用。
/// </summary>
internal sealed class ForegroundWatcher : IDisposable
{
    private readonly WinEventProc proc; // 防止委托被回收
    private readonly IntPtr hook;
    private readonly uint self = (uint)Environment.ProcessId;

    /// 前台变化：参数为前台程序的 exe 路径（本程序或无法查询时为 null）
    public event Action<string?>? Changed;
    /// 最后一个外部程序的 exe 路径
    public string? LastExternal { get; private set; }
    /// 当前前台程序的 exe 路径（本程序时为 null）
    public string? Current { get; private set; }

    public ForegroundWatcher()
    {
        proc = OnEvent;
        hook = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, IntPtr.Zero, proc, 0, 0, WINEVENT_OUTOFCONTEXT);
        Update(GetForegroundWindow());
    }

    private void OnEvent(IntPtr h, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time)
    {
        try { Update(hwnd); }
        catch (Exception e) { Log.Error("foreground_hook", e); }
    }

    private void Update(IntPtr hwnd)
    {
        if (hwnd == IntPtr.Zero) return;
        var pid = Apps.ProcessOf(hwnd);
        string? path = pid == self ? null : Apps.ImagePath(pid);
        Current = path;
        if (path != null && !Apps.IsShellSurface(hwnd)) LastExternal = path;
        Changed?.Invoke(path);
    }

    public void Dispose()
    {
        if (hook != IntPtr.Zero) UnhookWinEvent(hook);
    }
}
