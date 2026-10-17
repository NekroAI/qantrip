using System.Runtime.InteropServices;
using static Qantrip.Platform.Win32;

namespace Qantrip.Platform;

/// <summary>
/// 剪贴板的完整快照与恢复（Win32 API，可读写任意格式）。
/// 只保存基于 HGLOBAL 的格式；GDI 句柄类格式（CF_BITMAP 等）由系统从 CF_DIB 等自动合成，无需保存。
/// </summary>
internal static class ClipboardStore
{
    /// Windows 剪贴板历史 / 云剪贴板 / 第三方剪贴板管理器的排除标记
    /// （https://learn.microsoft.com/windows/win32/dataxchg/clipboard-formats#cloud-clipboard-and-clipboard-history-formats）
    private static readonly uint ExcludeFromMonitor = RegisterClipboardFormat("ExcludeClipboardContentFromMonitorProcessing");
    private static readonly uint CanIncludeInHistory = RegisterClipboardFormat("CanIncludeInClipboardHistory");
    private static readonly uint CanUploadToCloud = RegisterClipboardFormat("CanUploadToCloudClipboard");

    public sealed record Item(uint Format, byte[] Data);

    private static bool IsHandleFormat(uint f) =>
        f is 2 /*CF_BITMAP*/ or 3 /*CF_METAFILEPICT*/ or 9 /*CF_PALETTE*/ or 14 /*CF_ENHMETAFILE*/
            or 0x80 /*CF_OWNERDISPLAY*/ or 0x82 /*CF_DSPBITMAP*/ or 0x83 /*CF_DSPMETAFILEPICT*/ or 0x8E /*CF_DSPENHMETAFILE*/
        || (f >= 0x300 && f <= 0x3FF); // CF_GDIOBJFIRST..LAST

    /// 打开剪贴板，被其他进程占用时重试（最多约 200ms）。
    private static bool Open(IntPtr owner)
    {
        for (int i = 0; i < 20; i++)
        {
            if (OpenClipboard(owner)) return true;
            Thread.Sleep(10);
        }
        return false;
    }

    /// 读取剪贴板中的纯文本（CF_UNICODETEXT）；没有文本或剪贴板被占用时返回 null。
    public static string? ReadText(IntPtr owner)
    {
        if (!Open(owner)) return null;
        try
        {
            var h = GetClipboardData(CF_UNICODETEXT);
            if (h == IntPtr.Zero) return null;
            var p = GlobalLock(h);
            if (p == IntPtr.Zero) return null;
            try { return Marshal.PtrToStringUni(p)?.Replace("\r\n", "\n"); }
            finally { GlobalUnlock(h); }
        }
        finally { CloseClipboard(); }
    }

    public static List<Item>? Snapshot(IntPtr owner)
    {
        if (!Open(owner)) return null;
        var items = new List<Item>();
        try
        {
            uint f = 0;
            while ((f = EnumClipboardFormats(f)) != 0)
            {
                if (IsHandleFormat(f)) continue;
                var h = GetClipboardData(f);
                if (h == IntPtr.Zero) continue;
                var size = (long)GlobalSize(h);
                if (size <= 0) continue;
                var p = GlobalLock(h);
                if (p == IntPtr.Zero) continue;
                try
                {
                    var data = new byte[size];
                    Marshal.Copy(p, data, 0, (int)size);
                    items.Add(new Item(f, data));
                }
                finally { GlobalUnlock(h); }
            }
        }
        finally { CloseClipboard(); }
        return items;
    }

    /// 写入 Prompt 文本。`exclude`：附加剪贴板历史 / 云同步 / 监视程序的排除标记。返回写入后的序列号（失败为 0）。
    public static uint SetText(IntPtr owner, string text, bool exclude)
    {
        if (!Open(owner)) return 0;
        try
        {
            EmptyClipboard();
            // Windows 程序约定剪贴板文本使用 CRLF
            var normalized = text.Replace("\r\n", "\n").Replace("\n", "\r\n");
            var bytes = System.Text.Encoding.Unicode.GetBytes(normalized + "\0");
            Put(CF_UNICODETEXT, bytes);
            if (exclude) PutMarkers();
        }
        finally { CloseClipboard(); }
        return GetClipboardSequenceNumber();
    }

    /// 恢复快照。恢复只是"还原"而非用户的新复制，因此同样附加排除标记，避免剪贴板历史出现重复条目（对应 macOS 的 D18）。
    public static bool Restore(IntPtr owner, List<Item> items)
    {
        if (!Open(owner)) return false;
        try
        {
            EmptyClipboard();
            if (items.Count == 0) return true;
            foreach (var it in items)
            {
                if (it.Format == ExcludeFromMonitor || it.Format == CanIncludeInHistory || it.Format == CanUploadToCloud) continue;
                Put(it.Format, it.Data);
            }
            PutMarkers();
            return true;
        }
        finally { CloseClipboard(); }
    }

    private static void PutMarkers()
    {
        Put(ExcludeFromMonitor, BitConverter.GetBytes(0));
        Put(CanIncludeInHistory, BitConverter.GetBytes(0));
        Put(CanUploadToCloud, BitConverter.GetBytes(0));
    }

    private static void Put(uint format, byte[] data)
    {
        var h = GlobalAlloc(GMEM_MOVEABLE, (UIntPtr)Math.Max(data.Length, 1));
        if (h == IntPtr.Zero) return;
        var p = GlobalLock(h);
        Marshal.Copy(data, 0, p, data.Length);
        GlobalUnlock(h);
        // 成功后内存归系统所有；失败时需要自己释放
        if (SetClipboardData(format, h) == IntPtr.Zero) GlobalFree(h);
    }

    public static string FormatName(uint f)
    {
        var buf = new char[128];
        int n = GetClipboardFormatName(f, buf, buf.Length);
        return n > 0 ? new string(buf, 0, n) : $"#{f}";
    }
}
