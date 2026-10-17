using System.Runtime.InteropServices;
using static Qantrip.Platform.Win32;

namespace Qantrip.Platform;

/// <summary>
/// 隐藏的顶层窗口（从不显示），接收全局热键、托盘图标回调、资源管理器重启（TaskbarCreated）
/// 与第二个实例发来的"打开面板"消息。在 UI 线程创建，由 WinUI 的消息循环分发。
/// </summary>
internal sealed class MessageWindow : IDisposable
{
    public const string ClassName = "NekroQantrip.MessageWindow";
    public static readonly uint ShowPanelMessage = RegisterWindowMessage("NekroQantrip.ShowPanel");
    public static readonly uint ShowSettingsMessage = RegisterWindowMessage("NekroQantrip.ShowSettings");
    public static readonly uint ShowLibraryMessage = RegisterWindowMessage("NekroQantrip.ShowLibrary");
    private static readonly uint TaskbarCreated = RegisterWindowMessage("TaskbarCreated");
    public const uint TrayMessage = WM_APP + 1;

    public IntPtr Handle { get; }
    /// 热键 id、消息时间（GetMessageTime，毫秒 tick）
    public event Action<int, int>? HotKey;
    public event Action<int>? Tray;
    public event Action? TaskbarRestarted;
    public event Action? ShowRequested;
    public event Action? SettingsRequested;
    public event Action? LibraryRequested;

    private readonly WndProc proc; // 防止委托被回收

    public MessageWindow()
    {
        proc = Proc;
        var wc = new WNDCLASSEX
        {
            cbSize = Marshal.SizeOf<WNDCLASSEX>(),
            lpfnWndProc = Marshal.GetFunctionPointerForDelegate(proc),
            hInstance = GetModuleHandle(null),
            lpszClassName = ClassName,
        };
        RegisterClassEx(ref wc);
        // WS_EX_TOOLWINDOW：不出现在任务栏与 Alt+Tab
        Handle = CreateWindowEx((uint)WS_EX_TOOLWINDOW, ClassName, "Nekro Qantrip", 0, 0, 0, 0, 0, IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
    }

    private IntPtr Proc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        try
        {
            if (msg == WM_HOTKEY) { HotKey?.Invoke((int)wParam, GetMessageTime()); return IntPtr.Zero; }
            if (msg == TrayMessage) { Tray?.Invoke((int)(lParam.ToInt64() & 0xFFFF)); return IntPtr.Zero; }
            if (msg == TaskbarCreated) { TaskbarRestarted?.Invoke(); return IntPtr.Zero; }
            if (msg == ShowPanelMessage) { ShowRequested?.Invoke(); return IntPtr.Zero; }
            if (msg == ShowSettingsMessage) { SettingsRequested?.Invoke(); return IntPtr.Zero; }
            if (msg == ShowLibraryMessage) { LibraryRequested?.Invoke(); return IntPtr.Zero; }
        }
        catch (Exception e) { Log.Error("wndproc", e); }
        return DefWindowProc(hWnd, msg, wParam, lParam);
    }

    public void Dispose() => DestroyWindow(Handle);
}

/// 基于 RegisterHotKey 的全局热键（无需任何权限；被占用时注册失败，不会抢占其他程序）。
internal sealed class HotKey : IDisposable
{
    private readonly MessageWindow window;
    private const int Id = 0x5154; // 'QT'
    public uint Modifiers { get; private set; }
    public uint Key { get; private set; }
    public int LastError { get; private set; }
    public bool IsRegistered { get; private set; }
    public event Action<int>? Pressed;

    public HotKey(MessageWindow window)
    {
        this.window = window;
        window.HotKey += (id, time) => { if (id == Id) Pressed?.Invoke(time); };
    }

    public bool Register(uint mods, uint vk)
    {
        Unregister();
        Modifiers = mods; Key = vk;
        IsRegistered = RegisterHotKey(window.Handle, Id, mods | MOD_NOREPEAT, vk);
        LastError = IsRegistered ? 0 : Marshal.GetLastWin32Error();
        return IsRegistered;
    }

    public void Unregister()
    {
        if (IsRegistered) UnregisterHotKey(window.Handle, Id);
        IsRegistered = false;
    }

    public string Describe() => HotKeySpec.Format(Modifiers, Key);

    public void Dispose() => Unregister();
}

/// 通知区域图标：左键打开面板，右键菜单（打开面板 / 设置 / 退出）。气泡提示用于插入降级等需要停留的消息。
internal sealed class TrayIcon : IDisposable
{
    private readonly MessageWindow window;
    private readonly IntPtr icon;
    private string tip;
    public bool Added { get; private set; }
    public event Action? Open;
    public event Action? OpenSettings;
    public event Action? OpenLibrary;
    public event Action? OpenGuide;
    public event Action? OpenSync;
    public event Action? Quit;
    public Func<string>? OpenLabel, SettingsLabel, LibraryLabel, GuideLabel, QuitLabel;
    /// 有设备等待加入同步时显示的菜单项（null 表示不显示）
    public Func<string?>? SyncLabel;

    public TrayIcon(MessageWindow window, string iconPath, string tip)
    {
        this.window = window;
        this.tip = tip;
        int size = GetSystemMetrics(SM_CXSMICON);
        icon = LoadImage(IntPtr.Zero, iconPath, IMAGE_ICON, size, size, LR_LOADFROMFILE);
        window.Tray += OnTray;
        window.TaskbarRestarted += Add;
        Add();
    }

    private NOTIFYICONDATA Data(uint flags) => new()
    {
        cbSize = Marshal.SizeOf<NOTIFYICONDATA>(), hWnd = window.Handle, uID = 1, uFlags = flags,
        uCallbackMessage = MessageWindow.TrayMessage, hIcon = icon, szTip = tip, szInfo = "", szInfoTitle = "",
    };

    private void Add()
    {
        var d = Data(NIF_MESSAGE | NIF_ICON | NIF_TIP);
        Added = Shell_NotifyIcon(NIM_ADD, ref d);
    }

    public void SetTip(string text)
    {
        tip = text;
        var d = Data(NIF_TIP);
        Shell_NotifyIcon(NIM_MODIFY, ref d);
    }

    /// 气泡通知（例如"已复制到剪贴板"）。
    public void Notify(string title, string text)
    {
        var d = Data(NIF_INFO);
        d.szInfo = text.Length > 255 ? text[..255] : text;
        d.szInfoTitle = title;
        d.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND;
        Shell_NotifyIcon(NIM_MODIFY, ref d);
    }

    private void OnTray(int msg)
    {
        if (msg == WM_LBUTTONUP) { Open?.Invoke(); return; }
        if (msg != WM_RBUTTONUP && msg != WM_CONTEXTMENU) return;
        var menu = CreatePopupMenu();
        AppendMenu(menu, MF_STRING, (UIntPtr)1, OpenLabel?.Invoke() ?? "Open");
        if (SyncLabel?.Invoke() is { } sync) AppendMenu(menu, MF_STRING, (UIntPtr)6, sync);
        AppendMenu(menu, MF_SEPARATOR, UIntPtr.Zero, null);
        AppendMenu(menu, MF_STRING, (UIntPtr)4, LibraryLabel?.Invoke() ?? "Template Library…");
        AppendMenu(menu, MF_STRING, (UIntPtr)3, SettingsLabel?.Invoke() ?? "Settings…");
        AppendMenu(menu, MF_STRING, (UIntPtr)5, GuideLabel?.Invoke() ?? "Welcome Guide…");
        AppendMenu(menu, MF_SEPARATOR, UIntPtr.Zero, null);
        AppendMenu(menu, MF_STRING, (UIntPtr)2, QuitLabel?.Invoke() ?? "Quit");
        GetCursorPos(out var p);
        // 菜单要求宿主窗口在前台，否则点击外部不会关闭
        SetForegroundWindow(window.Handle);
        uint cmd = TrackPopupMenuEx(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, p.X, p.Y, window.Handle, IntPtr.Zero);
        PostMessage(window.Handle, WM_NULL, IntPtr.Zero, IntPtr.Zero);
        DestroyMenu(menu);
        if (cmd == 1) Open?.Invoke();
        if (cmd == 2) Quit?.Invoke();
        if (cmd == 3) OpenSettings?.Invoke();
        if (cmd == 4) OpenLibrary?.Invoke();
        if (cmd == 5) OpenGuide?.Invoke();
        if (cmd == 6) OpenSync?.Invoke();
    }

    public void Dispose()
    {
        var d = Data(0);
        Shell_NotifyIcon(NIM_DELETE, ref d);
    }
}
