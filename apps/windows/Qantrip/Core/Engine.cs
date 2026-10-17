using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Qantrip.Core;

/// <summary>
/// Rust 核心（apps/windows/native，C ABI + JSON）的薄封装。所有文本位置都是 UTF-16 偏移，与 C# string 索引一致。
/// 只在 UI 线程上调用。
/// </summary>
public sealed class Engine : IDisposable
{
    private IntPtr handle;

    public static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public Engine(string? userDir, string? usagePath)
    {
        handle = Native.qt_engine_new(U8(userDir), U8(usagePath));
        if (handle == IntPtr.Zero) throw new InvalidOperationException("qt_engine_new failed");
    }

    public static string CoreVersion => Take(Native.qt_core_version()) ?? "?";

    public void SetLocale(string ui, string templates) => Native.qt_set_locale(handle, U8(ui), U8(templates));
    public void BeginSession(string? app) => Native.qt_begin_session(handle, U8(app));
    public void Reload() => Native.qt_reload(handle);

    public Analysis Analyze(string input, string? selected)
    {
        var json = Take(Native.qt_analyze(handle, U8(input), U8(selected)));
        return json is null ? Analysis.Empty : JsonSerializer.Deserialize<Analysis>(json, Json) ?? Analysis.Empty;
    }

    public string? Render(string input, string? selected) => Take(Native.qt_render(handle, U8(input), U8(selected)));

    public void RecordUse(string input, string? selected, string? app) => Native.qt_record_use(handle, U8(input), U8(selected), U8(app));

    public EditResult? EditParam(string input, string? selected, string name, ParamEdit edit)
    {
        var json = Take(Native.qt_edit_param(handle, U8(input), U8(selected), U8(name), U8(JsonSerializer.Serialize(edit, Json))));
        return json is null ? null : JsonSerializer.Deserialize<EditResult>(json, Json);
    }

    public List<TemplateInfo> Templates()
    {
        var json = Take(Native.qt_templates(handle));
        return json is null ? new() : JsonSerializer.Deserialize<List<TemplateInfo>>(json, Json) ?? new();
    }

    public List<string> Problems()
    {
        var json = Take(Native.qt_problems(handle));
        return json is null ? new() : JsonSerializer.Deserialize<List<string>>(json, Json) ?? new();
    }

    public void Dispose()
    {
        if (handle != IntPtr.Zero) Native.qt_engine_free(handle);
        handle = IntPtr.Zero;
    }

    private static byte[]? U8(string? s) => s is null ? null : Encoding.UTF8.GetBytes(s + "\0");

    private static string? Take(IntPtr p)
    {
        if (p == IntPtr.Zero) return null;
        try { return Marshal.PtrToStringUTF8(p); }
        finally { Native.qt_string_free(p); }
    }

    private static class Native
    {
        private const string Dll = "qantrip_ffi";
        [DllImport(Dll)] public static extern void qt_string_free(IntPtr s);
        [DllImport(Dll)] public static extern IntPtr qt_core_version();
        [DllImport(Dll)] public static extern IntPtr qt_engine_new(byte[]? userDir, byte[]? usagePath);
        [DllImport(Dll)] public static extern void qt_engine_free(IntPtr e);
        [DllImport(Dll)] public static extern void qt_set_locale(IntPtr e, byte[]? ui, byte[]? templates);
        [DllImport(Dll)] public static extern void qt_begin_session(IntPtr e, byte[]? app);
        [DllImport(Dll)] public static extern void qt_reload(IntPtr e);
        [DllImport(Dll)] public static extern IntPtr qt_analyze(IntPtr e, byte[]? input, byte[]? selected);
        [DllImport(Dll)] public static extern IntPtr qt_render(IntPtr e, byte[]? input, byte[]? selected);
        [DllImport(Dll)] public static extern void qt_record_use(IntPtr e, byte[]? input, byte[]? selected, byte[]? app);
        [DllImport(Dll)] public static extern IntPtr qt_edit_param(IntPtr e, byte[]? input, byte[]? selected, byte[]? name, byte[]? edit);
        [DllImport(Dll)] public static extern IntPtr qt_templates(IntPtr e);
        [DllImport(Dll)] public static extern IntPtr qt_problems(IntPtr e);
    }
}
