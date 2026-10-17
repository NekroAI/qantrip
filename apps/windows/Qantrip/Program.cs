using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Qantrip.Platform;

namespace Qantrip;

public static class Program
{
    /// 自定义入口（DISABLE_XAML_GENERATED_MAIN）：单实例。再次启动时通知已运行的实例打开面板后退出。
    [STAThread]
    public static int Main(string[] args)
    {
        using var mutex = new Mutex(true, @"Local\NekroQantrip.Instance", out bool first);
        if (!first)
        {
            var h = Win32.FindWindow(MessageWindow.ClassName, null);
            if (h != IntPtr.Zero)
            {
                Win32.AllowSetForegroundWindowAny();
                Win32.PostMessage(h, MessageWindow.ShowPanelMessage, IntPtr.Zero, IntPtr.Zero);
            }
            return 0;
        }
        WinRT.ComWrappersSupport.InitializeComWrappers();
        Application.Start(p =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new App();
        });
        return 0;
    }
}
