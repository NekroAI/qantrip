using System.Globalization;
using System.Text.Json;

namespace Qantrip;

/// 应用设置：%APPDATA%\Qantrip\settings.json（缺省值与 macOS 一致）。设置界面尚未实现，可手动编辑该文件。
internal static class Settings
{
    /// 数据目录（模板、使用记录、设置）。环境变量 QANTRIP_DATA_DIR 可覆盖（测试用）。
    public static string DataDir { get; } = Environment.GetEnvironmentVariable("QANTRIP_DATA_DIR") is { Length: > 0 } d
        ? d
        : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "Qantrip");

    private static readonly Dictionary<string, JsonElement> values = Load();

    private static Dictionary<string, JsonElement> Load()
    {
        try
        {
            var path = Path.Combine(DataDir, "settings.json");
            if (File.Exists(path))
                return JsonSerializer.Deserialize<Dictionary<string, JsonElement>>(File.ReadAllText(path)) ?? new();
        }
        catch (Exception e) { Log.Error("settings", e); }
        return new();
    }

    private static bool Bool(string key, bool fallback) =>
        values.TryGetValue(key, out var v) && v.ValueKind is JsonValueKind.True or JsonValueKind.False ? v.GetBoolean() : fallback;

    private static int Int(string key, int fallback) =>
        values.TryGetValue(key, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetInt32() : fallback;

    private static string? Str(string key) =>
        values.TryGetValue(key, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    /// 唤起快捷键，如 "Alt+Q"（D5：Windows 默认 Alt+Q，Ctrl+Q 在许多程序中是退出）
    public static string HotKey => Str("hotKey") ?? "Alt+Q";
    public static bool ForceAsciiInput => Bool("forceAsciiInput", true);
    /// 再次唤起时输入法仍在中文模式则模拟一次 Shift 切回英文（实验性，见 docs/windows-spike.md）
    public static bool ImeShiftToggle => Bool("imeShiftToggle", true);
    public static int ImeShiftDelayMs => Int("imeShiftDelayMs", 60);
    public static bool CopyOnly => Bool("copyOnly", false);
    public static bool RecordUsage => Bool("recordUsage", true);
    public static bool ExpandPasteEnabled => Bool("expandPasteEnabled", true);
    public static int ExpandPasteIntervalMs => Int("expandPasteIntervalMs", 250);
    public static int PasteDelayMs => Int("pasteDelayMs", 0);
    public static int RestoreDelayMs => Int("restoreDelayMs", 500);
    /// 界面语言：auto / zh / en
    public static string Language => Str("language") ?? "auto";

    /// 调试钩子（仅开发测试）：记录最后一次插入的文本，便于回归脚本比对
    public static bool DebugHooks => Environment.GetEnvironmentVariable("QANTRIP_DEBUG_HOOKS") == "1";

    public static string UiLocale
    {
        get
        {
            var l = Language;
            if (l is "zh" or "en") return l;
            return CultureInfo.CurrentUICulture.Name.StartsWith("zh", StringComparison.OrdinalIgnoreCase) ? "zh" : "en";
        }
    }
}

/// 界面文字：按界面语言选择中文或英文。
internal static class Loc
{
    public static bool En { get; set; }
    public static string T(string zh, string en) => En ? en : zh;
}

/// 埋点日志：%LOCALAPPDATA%\Qantrip\logs\spike.jsonl（只记录时长、字符数等指标，不记录 Prompt 正文）。
internal static class Log
{
    public static string Dir { get; } = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Qantrip", "logs");
    private static readonly object gate = new();

    public static void Event(string name, Dictionary<string, object?> fields)
    {
        try
        {
            var d = new Dictionary<string, object?> { ["ts"] = DateTime.Now.ToString("o"), ["event"] = name };
            foreach (var kv in fields) d[kv.Key] = kv.Value;
            var line = JsonSerializer.Serialize(d);
            lock (gate)
            {
                Directory.CreateDirectory(Dir);
                File.AppendAllText(Path.Combine(Dir, "spike.jsonl"), line + "\n");
            }
        }
        catch { }
    }

    public static void Error(string where, Exception e) =>
        Event("error", new() { ["where"] = where, ["type"] = e.GetType().Name, ["message"] = e.Message, ["stack"] = e.StackTrace });
}
