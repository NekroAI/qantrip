using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace Qantrip.Platform;

/// 回收站：/rm 删除的模板移到回收站，可以还原（对应 macOS 的 trashItem）。
internal static class RecycleBin
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct SHFILEOPSTRUCT
    {
        public IntPtr hwnd;
        public uint wFunc;
        public string pFrom;
        public string? pTo;
        public ushort fFlags;
        public int fAnyOperationsAborted;
        public IntPtr hNameMappings;
        public string? lpszProgressTitle;
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] private static extern int SHFileOperation(ref SHFILEOPSTRUCT op);

    private const uint FO_DELETE = 3;
    private const ushort FOF_SILENT = 0x4, FOF_NOCONFIRMATION = 0x10, FOF_ALLOWUNDO = 0x40, FOF_NOERRORUI = 0x400, FOF_WANTNUKEWARNING = 0x4000;

    /// 把文件移到回收站（不弹确认框）。返回是否成功。
    public static bool Move(string path)
    {
        if (!File.Exists(path)) return false;
        var op = new SHFILEOPSTRUCT
        {
            wFunc = FO_DELETE,
            pFrom = Path.GetFullPath(path) + "\0\0", // 以两个 NUL 结尾的列表
            // FOF_WANTNUKEWARNING：回收站不可用（如超出容量）时不会静默永久删除
            fFlags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_WANTNUKEWARNING,
        };
        int r = SHFileOperation(ref op);
        return r == 0 && op.fAnyOperationsAborted == 0 && !File.Exists(path);
    }
}

/// 登录时启动：HKCU\Software\Microsoft\Windows\CurrentVersion\Run（无需管理员权限）。
internal static class Autostart
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    /// 任务管理器"启动应用"里的启用 / 禁用状态（首字节 2 = 启用，3 = 禁用）
    private const string ApprovedKey = @"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    private const string Name = "NekroQantrip";

    private static string Command => $"\"{Environment.ProcessPath}\"";

    public static bool IsEnabled
    {
        get
        {
            try
            {
                using var run = Registry.CurrentUser.OpenSubKey(RunKey);
                if (run?.GetValue(Name) is not string) return false;
                using var approved = Registry.CurrentUser.OpenSubKey(ApprovedKey);
                return approved?.GetValue(Name) is not byte[] b || b.Length == 0 || b[0] % 2 == 0;
            }
            catch { return false; }
        }
    }

    /// 已启用但指向的不是当前程序（例如移动了程序目录）
    public static bool PointsElsewhere
    {
        get
        {
            try
            {
                using var run = Registry.CurrentUser.OpenSubKey(RunKey);
                return run?.GetValue(Name) is string s && !string.Equals(s, Command, StringComparison.OrdinalIgnoreCase);
            }
            catch { return false; }
        }
    }

    public static void Set(bool enabled)
    {
        try
        {
            using var run = Registry.CurrentUser.CreateSubKey(RunKey);
            if (enabled) run.SetValue(Name, Command);
            else run.DeleteValue(Name, false);
            // 用户在设置中明确开启时，清除任务管理器中的"已禁用"标记
            using var approved = Registry.CurrentUser.OpenSubKey(ApprovedKey, writable: true);
            approved?.DeleteValue(Name, false);
        }
        catch (Exception e) { Log.Error("autostart", e); }
    }
}

/// 系统"动画效果"设置（设置 › 辅助功能 › 视觉效果）。关闭时所有动效降级为直接切换（D15）。
internal static class Motion
{
    [DllImport("user32.dll")] private static extern bool SystemParametersInfo(uint action, uint param, out int value, uint ini);
    private const uint SPI_GETCLIENTAREAANIMATION = 0x1042;

    public static bool Enabled
    {
        get
        {
            if (Environment.GetEnvironmentVariable("QANTRIP_REDUCE_MOTION") == "1") return false;
            return !SystemParametersInfo(SPI_GETCLIENTAREAANIMATION, 0, out int on, 0) || on != 0;
        }
    }
}

/// 热键文本与 (修饰键, 虚拟键码) 之间的转换："Alt+Q" / "Ctrl+Shift+Space" / "Win+F2"。
internal static class HotKeySpec
{
    public static (uint mods, uint vk) Parse(string spec)
    {
        uint mods = 0, vk = 'Q';
        foreach (var raw in spec.Split('+', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            var p = raw.ToLowerInvariant();
            switch (p)
            {
                case "ctrl" or "control": mods |= Win32.MOD_CONTROL; break;
                case "alt": mods |= Win32.MOD_ALT; break;
                case "shift": mods |= Win32.MOD_SHIFT; break;
                case "win": mods |= Win32.MOD_WIN; break;
                case "space": vk = 0x20; break;
                default:
                    if (p.Length == 1 && char.IsAsciiLetterOrDigit(p[0])) vk = char.ToUpperInvariant(p[0]);
                    else if (p.StartsWith('f') && int.TryParse(p[1..], out int f) && f is >= 1 and <= 24) vk = (uint)(0x70 + f - 1);
                    break;
            }
        }
        return (mods == 0 && vk is < 0x70 or > 0x87 ? Win32.MOD_ALT : mods, vk);
    }

    public static string KeyName(uint vk) => vk switch
    {
        0x20 => "Space",
        >= 0x70 and <= 0x87 => "F" + (vk - 0x70 + 1),
        >= '0' and <= '9' or >= 'A' and <= 'Z' => ((char)vk).ToString(),
        _ => "?",
    };

    /// 设置界面可以录制的按键：字母、数字、空格、F1–F24
    public static bool IsSupportedKey(uint vk) => KeyName(vk) != "?";

    public static string Format(uint mods, uint vk)
    {
        var parts = new List<string>();
        if ((mods & Win32.MOD_CONTROL) != 0) parts.Add("Ctrl");
        if ((mods & Win32.MOD_ALT) != 0) parts.Add("Alt");
        if ((mods & Win32.MOD_SHIFT) != 0) parts.Add("Shift");
        if ((mods & Win32.MOD_WIN) != 0) parts.Add("Win");
        parts.Add(KeyName(vk));
        return string.Join("+", parts);
    }
}
