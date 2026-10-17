using static Qantrip.Platform.Win32;

namespace Qantrip.Platform;

/// 前台窗口的记录与切换。
internal static class Foreground
{
    public sealed record Target(IntPtr Hwnd, uint Pid, string Exe, string Title);

    public static Target? Capture()
    {
        var h = GetForegroundWindow();
        if (h == IntPtr.Zero) return null;
        GetWindowThreadProcessId(h, out uint pid);
        return new Target(h, pid, ExeName(pid), Title(h));
    }

    public static string ExeName(uint pid)
    {
        var p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (p == IntPtr.Zero) return "";
        try
        {
            var buf = new char[1024];
            int n = buf.Length;
            return QueryFullProcessImageName(p, 0, buf, ref n) ? Path.GetFileName(new string(buf, 0, n)) : "";
        }
        finally { CloseHandle(p); }
    }

    public static string Title(IntPtr h)
    {
        var buf = new char[256];
        int n = GetWindowText(h, buf, buf.Length);
        return new string(buf, 0, Math.Max(n, 0));
    }

    /// 目标进程是否以管理员身份运行（UIPI 会静默拦截我们发给它的 SendInput）。查询失败按"是"处理。
    public static bool IsElevated(uint pid)
    {
        var p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (p == IntPtr.Zero) return true;
        try
        {
            if (!OpenProcessToken(p, TOKEN_QUERY, out var token)) return true;
            try { return GetTokenInformation(token, TokenElevation, out int elevated, 4, out _) && elevated != 0; }
            finally { CloseHandle(token); }
        }
        finally { CloseHandle(p); }
    }

    public static bool SelfElevated => IsElevated((uint)Environment.ProcessId);

    /// <summary>
    /// 把窗口切到前台。先直接调用（我们刚收到热键 / 自己在前台时通常允许）；
    /// 失败时临时附加到当前前台线程的输入队列再试。返回最终是否成功。
    /// </summary>
    public static bool Activate(IntPtr hwnd, out string method)
    {
        method = "direct";
        if (IsIconic(hwnd)) ShowWindow(hwnd, SW_RESTORE);
        if (SetForegroundWindow(hwnd) && GetForegroundWindow() == hwnd) return true;
        method = "attach";
        var fg = GetForegroundWindow();
        uint fgThread = GetWindowThreadProcessId(fg, out _);
        uint me = GetCurrentThreadId();
        bool attached = fgThread != 0 && fgThread != me && AttachThreadInput(me, fgThread, true);
        try
        {
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
        }
        finally { if (attached) AttachThreadInput(me, fgThread, false); }
        return GetForegroundWindow() == hwnd;
    }

    /// 等待某窗口成为前台，最多 `timeoutMs`。
    public static async Task<bool> WaitFor(IntPtr hwnd, int timeoutMs)
    {
        var sw = System.Diagnostics.Stopwatch.StartNew();
        while (sw.ElapsedMilliseconds < timeoutMs)
        {
            if (GetForegroundWindow() == hwnd) return true;
            await Task.Delay(5);
        }
        return GetForegroundWindow() == hwnd;
    }
}
