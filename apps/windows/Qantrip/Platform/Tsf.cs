using System.Runtime.InteropServices;

namespace Qantrip.Platform;

/// <summary>
/// 读取 UI 线程 TSF 线程管理器上的键盘隔间（开关、输入模式）。
/// 实测（微软拼音）：写入这些隔间不会改变 WinUI 输入框的中 / 英模式，但用户在面板中打过字之后，
/// 读数能反映输入法的当前模式（第一次打开前为空）。见 docs/windows-spike.md「输入法」。
/// </summary>
internal static class Tsf
{
    private static readonly Guid KeyboardOpenClose = new("58273aad-01bb-4164-95c6-755ba0b5162d");
    private static readonly Guid InputModeConversion = new("ccf05dd8-4a87-11d7-a6e2-00065b84435c");
    private const int ConversionNative = 0x1;
    private static readonly Guid CLSID_TF_ThreadMgr = new("529a9e6b-6587-4f23-ab9e-9c7d683e3c50");
    private static ITfCompartmentMgr? mgr;

    private static int? Get(Guid id)
    {
        mgr ??= (ITfCompartmentMgr)Activator.CreateInstance(Type.GetTypeFromCLSID(CLSID_TF_ThreadMgr)!)!;
        mgr.GetCompartment(ref id, out var c);
        c.GetValue(out object? v);
        return v is int i ? i : null;
    }

    /// 输入法当前处于中文（本地语言）模式：true / false；无法判断：null
    public static bool? NativeMode()
    {
        try
        {
            int? open = Get(KeyboardOpenClose), conv = Get(InputModeConversion);
            if (open is null || conv is null) return null;
            return open != 0 && (conv & ConversionNative) != 0;
        }
        catch (Exception e) { Log.Error("tsf", e); return null; }
    }

    [ComImport, Guid("7dcf57ac-18ad-438b-824d-979bffb74b7c"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface ITfCompartmentMgr
    {
        void GetCompartment(ref Guid id, out ITfCompartment compartment);
        void ClearCompartment(uint clientId, ref Guid id);
        void EnumCompartments(out IntPtr e);
    }

    [ComImport, Guid("bb08f7a9-607a-4384-8623-056892b64371"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface ITfCompartment
    {
        [PreserveSig] int SetValue(uint clientId, ref object value);
        void GetValue(out object? value);
    }
}
