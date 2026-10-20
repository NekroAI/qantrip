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

    public void ClearUsage() => Native.qt_clear_usage(handle);

    /// 指令模式 `/new 名称` 不写正文时使用的剪贴板文字。
    public void SetClipboard(string? text) => Native.qt_set_clipboard(handle, U8(text));

    /// 执行指令（输入以 / 开头）。成功后核心会重新加载模板库。
    public CommandResult RunCommand(string input)
    {
        var json = Take(Native.qt_run_command(handle, U8(input)));
        return (json is null ? null : JsonSerializer.Deserialize<CommandResult>(json, Json)) ?? new CommandResult { Ok = false, Error = "qt_run_command failed" };
    }

    /// 删除模板（模板库的"删除"，与 /rm 相同，D34）。Outcome.TrashPath 由调用方移到回收站。
    public CommandResult DeleteTemplate(string name) => Outcome(Native.qt_delete_template(handle, U8(name)), "qt_delete_template");

    /// 恢复内置模板（撤销删除或修改，与 /restore 相同）。
    public CommandResult RestoreTemplate(string name) => Outcome(Native.qt_restore_template(handle, U8(name)), "qt_restore_template");

    private static CommandResult Outcome(IntPtr p, string fn)
    {
        var json = Take(p);
        return (json is null ? null : JsonSerializer.Deserialize<CommandResult>(json, Json)) ?? new CommandResult { Ok = false, Error = fn + " failed" };
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

    // MARK: 模板库与编辑器

    /// 模板库全部条目（内置、用户模板、公共片段，按模板库顺序）。
    public List<TemplateEntry> Entries()
    {
        var json = Take(Native.qt_entries(handle));
        return json is null ? new() : JsonSerializer.Deserialize<List<TemplateEntry>>(json, Json) ?? new();
    }

    /// 内置模板的原始源码（恢复默认）。
    public string? BuiltinSource(string name) => Take(Native.qt_builtin_source(handle, U8(name)));

    /// 校验编辑中的源码，并以 tryInput（为空时用模板名）试用。
    public SourceCheck CheckSource(string source, string tryInput)
    {
        var json = Take(Native.qt_check_source(handle, U8(source), U8(tryInput)));
        return (json is null ? null : JsonSerializer.Deserialize<SourceCheck>(json, Json)) ?? new SourceCheck { Problems = { "qt_check_source failed" } };
    }

    /// 源码 → 结构化文档；无法解析时返回 null 并给出错误。
    public static TemplateDoc? ParseDoc(string source, out string? error)
    {
        error = null;
        var json = Take(Native.qt_parse_doc(U8(source)));
        if (json is null) { error = "qt_parse_doc failed"; return null; }
        using var d = JsonDocument.Parse(json);
        if (d.RootElement.GetProperty("ok").GetBoolean()) return d.RootElement.GetProperty("doc").Deserialize<TemplateDoc>(Json);
        error = d.RootElement.GetProperty("error").GetString();
        return null;
    }

    public static TemplateDoc? ParseDoc(string source) => ParseDoc(source, out _);

    /// 结构化文档 → 模板源码。
    public static string SerializeDoc(TemplateDoc doc) =>
        Take(Native.qt_serialize_doc(U8(JsonSerializer.Serialize(doc, Json)))) ?? throw new InvalidOperationException("qt_serialize_doc failed");

    /// 模板编辑器的引导：按当前参数可插入的写法与试用示例。
    public static TemplateGuide Guide(TemplateDoc doc, string locale)
    {
        var json = Take(Native.qt_template_guide(U8(JsonSerializer.Serialize(doc, Json)), U8(locale)));
        return (json is null ? null : JsonSerializer.Deserialize<TemplateGuide>(json, Json)) ?? new TemplateGuide { TryExample = doc.Name };
    }

    /// 新模板的初始文档。
    public static TemplateDoc NewDoc(string name, string locale)
    {
        var json = Take(Native.qt_new_doc(U8(name), U8(locale)));
        return (json is null ? null : JsonSerializer.Deserialize<TemplateDoc>(json, Json)) ?? new TemplateDoc { Name = name };
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
        [DllImport(Dll)] public static extern void qt_clear_usage(IntPtr e);
        [DllImport(Dll)] public static extern void qt_set_clipboard(IntPtr e, byte[]? text);
        [DllImport(Dll)] public static extern IntPtr qt_run_command(IntPtr e, byte[]? input);
        [DllImport(Dll)] public static extern IntPtr qt_delete_template(IntPtr e, byte[]? name);
        [DllImport(Dll)] public static extern IntPtr qt_restore_template(IntPtr e, byte[]? name);
        [DllImport(Dll)] public static extern IntPtr qt_problems(IntPtr e);
        [DllImport(Dll)] public static extern IntPtr qt_entries(IntPtr e);
        [DllImport(Dll)] public static extern IntPtr qt_builtin_source(IntPtr e, byte[]? name);
        [DllImport(Dll)] public static extern IntPtr qt_check_source(IntPtr e, byte[]? source, byte[]? tryInput);
        [DllImport(Dll)] public static extern IntPtr qt_parse_doc(byte[]? source);
        [DllImport(Dll)] public static extern IntPtr qt_serialize_doc(byte[]? doc);
        [DllImport(Dll)] public static extern IntPtr qt_new_doc(byte[]? name, byte[]? locale);
        [DllImport(Dll)] public static extern IntPtr qt_template_guide(byte[]? doc, byte[]? locale);
    }
}
