using System.Globalization;
using System.Text.Json;

namespace Qantrip;

/// <summary>
/// 应用设置：%APPDATA%\Qantrip\settings.json（缺省值与 macOS 一致）。由设置窗口修改，也可手动编辑。
/// 写入时保留文件中不认识的键，修改即保存；`Changed` 通知需要即时生效的部分（热键、语言等）。
/// </summary>
internal static class Settings
{
    /// 数据目录（模板、使用记录、设置）。环境变量 QANTRIP_DATA_DIR 可覆盖（测试用）。
    public static string DataDir { get; } = Environment.GetEnvironmentVariable("QANTRIP_DATA_DIR") is { Length: > 0 } d
        ? d
        : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "Qantrip");

    public static string FilePath => Path.Combine(DataDir, "settings.json");
    public static string TemplatesDir => Path.Combine(DataDir, "templates");

    private static readonly Dictionary<string, JsonElement> values = Load();
    private static readonly JsonSerializerOptions WriteOptions = new() { WriteIndented = true, Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

    /// 某项设置被修改（参数为键名）
    public static event Action<string>? Changed;

    private static Dictionary<string, JsonElement> Load()
    {
        try
        {
            if (File.Exists(FilePath))
                return JsonSerializer.Deserialize<Dictionary<string, JsonElement>>(File.ReadAllText(FilePath)) ?? new();
        }
        catch (Exception e) { Log.Error("settings", e); }
        return new();
    }

    private static void Save()
    {
        try
        {
            Directory.CreateDirectory(DataDir);
            var tmp = FilePath + ".tmp";
            File.WriteAllText(tmp, JsonSerializer.Serialize(values, WriteOptions));
            File.Move(tmp, FilePath, overwrite: true);
        }
        catch (Exception e) { Log.Error("settings_save", e); }
    }

    private static void Set<T>(string key, T value)
    {
        var v = JsonSerializer.SerializeToElement(value);
        if (values.TryGetValue(key, out var old) && old.GetRawText() == v.GetRawText()) return;
        values[key] = v;
        Save();
        Changed?.Invoke(key);
    }

    private static bool Bool(string key, bool fallback) =>
        values.TryGetValue(key, out var v) && v.ValueKind is JsonValueKind.True or JsonValueKind.False ? v.GetBoolean() : fallback;

    private static int Int(string key, int fallback) =>
        values.TryGetValue(key, out var v) && v.ValueKind == JsonValueKind.Number && v.TryGetInt32(out var i) ? i : fallback;

    private static string? Str(string key) =>
        values.TryGetValue(key, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    private static string Lang(string key) => Str(key) is "zh" or "en" ? Str(key)! : "auto";

    public const string DefaultHotKey = "Ctrl+Q";

    /// 唤起快捷键，如 "Ctrl+Q"（D33：与 macOS 的 ⌃Q 一致；可在设置中更改）
    public static string HotKey { get => Str("hotKey") ?? DefaultHotKey; set => Set("hotKey", value); }
    /// 唤起时让输入框默认英文（D9）
    public static bool ForceAsciiInput { get => Bool("forceAsciiInput", true); set => Set("forceAsciiInput", value); }
    /// 再次唤起时输入法仍在中文模式则模拟一次 Shift 切回英文（实验性，见 docs/windows-spike.md）
    public static bool ImeShiftToggle { get => Bool("imeShiftToggle", true); set => Set("imeShiftToggle", value); }
    public static int ImeShiftDelayMs => Int("imeShiftDelayMs", 60);
    /// 只复制模式：不模拟粘贴
    public static bool CopyOnly { get => Bool("copyOnly", false); set => Set("copyOnly", value); }
    public static bool RecordUsage { get => Bool("recordUsage", true); set => Set("recordUsage", value); }
    /// Ctrl+↩ 展开粘贴（D13）
    public static bool ExpandPasteEnabled { get => Bool("expandPasteEnabled", true); set => Set("expandPasteEnabled", value); }
    public static int ExpandPasteIntervalMs { get => Int("expandPasteIntervalMs", 250); set => Set("expandPasteIntervalMs", value); }
    public static int PasteDelayMs => Int("pasteDelayMs", 0);
    public static int RestoreDelayMs => Int("restoreDelayMs", 500);
    /// 界面语言：auto / zh / en
    public static string Language { get => Lang("language"); set => Set("language", value); }
    /// 内置模板语言：auto（跟随界面语言）/ zh / en
    public static string TemplateLanguage { get => Lang("templateLanguage"); set => Set("templateLanguage", value); }

    public const string DefaultSyncServer = "https://qantrip.nekro.ai";
    /// 同步服务端地址（可自行部署，登录后不能修改）
    public static string SyncServer
    {
        get => Str("syncServer") is { Length: > 0 } s ? s.TrimEnd('/') : DefaultSyncServer;
        set => Set("syncServer", string.IsNullOrWhiteSpace(value) ? DefaultSyncServer : value.Trim().TrimEnd('/'));
    }
    /// 首次启动引导已完成
    public static bool OnboardingDone { get => Bool("onboardingDone", false); set => Set("onboardingDone", value); }

    // 更新提醒（UpdateChecker）：只提醒，不自动下载或安装
    /// 自动检查更新（启动约 10 秒后、之后每 24 小时；自动检查至多每 12 小时一次）
    public static bool AutoCheckUpdates { get => Bool("checkForUpdates", true); set => Set("checkForUpdates", value); }
    /// 上次检查更新的时间（无论成功与否）
    public static DateTimeOffset UpdateLastCheck
    {
        get => DateTimeOffset.TryParse(Str("updateLastCheck"), CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var t) ? t : DateTimeOffset.MinValue;
        set => Set("updateLastCheck", value.ToString("o", CultureInfo.InvariantCulture));
    }
    /// 上次检查得到的较新版本与发布页（重启后托盘菜单仍可显示）
    public static string? UpdateLatestVersion { get => Str("updateLatestVersion"); set => Set("updateLatestVersion", value); }
    public static string? UpdateLatestUrl { get => Str("updateLatestUrl"); set => Set("updateLatestUrl", value); }
    /// 已经提醒过的版本（每个版本只发一次通知）
    public static string? UpdateNotifiedVersion { get => Str("updateNotifiedVersion"); set => Set("updateNotifiedVersion", value); }
    /// 用户选择跳过的版本（不再通知，托盘菜单不显示）
    public static string? UpdateSkippedVersion { get => Str("updateSkippedVersion"); set => Set("updateSkippedVersion", value); }

    private static List<ExcludedApp>? excluded;

    /// 排除的程序（D35，按 exe 完整路径）：前台是这些程序时暂停唤起快捷键。`excludedApps`: [{"path","name"}]
    public static IReadOnlyList<ExcludedApp> ExcludedApps
    {
        get
        {
            if (excluded != null) return excluded;
            var list = new List<ExcludedApp>();
            if (values.TryGetValue("excludedApps", out var v) && v.ValueKind == JsonValueKind.Array)
                foreach (var e in v.EnumerateArray())
                {
                    if (e.ValueKind != JsonValueKind.Object || !e.TryGetProperty("path", out var p) || p.ValueKind != JsonValueKind.String) continue;
                    var path = p.GetString()!;
                    if (path.Length == 0 || list.Any(a => a.Is(path))) continue;
                    var name = e.TryGetProperty("name", out var n) && n.ValueKind == JsonValueKind.String ? n.GetString()! : Path.GetFileNameWithoutExtension(path);
                    list.Add(new ExcludedApp(path, name));
                }
            return excluded = list;
        }
        set
        {
            excluded = null;
            Set("excludedApps", value.Select(a => new { path = a.Path, name = a.Name }).ToList());
        }
    }

    public static bool IsExcluded(string? path) => path != null && ExcludedApps.Any(a => a.Is(path));

    /// 加入或移出排除列表
    public static void SetExcluded(string path, string name, bool exclude)
    {
        var list = ExcludedApps.Where(a => !a.Is(path)).ToList();
        if (exclude) list.Add(new ExcludedApp(path, name));
        ExcludedApps = list;
    }

    /// 调试钩子（仅开发测试）：记录最后一次插入的文本，便于回归脚本比对
    public static bool DebugHooks => Environment.GetEnvironmentVariable("QANTRIP_DEBUG_HOOKS") == "1";

    /// 实际使用的界面语言（auto 跟随 Windows 显示语言）
    public static string UiLocale
    {
        get
        {
            var l = Language;
            if (l is "zh" or "en") return l;
            return CultureInfo.CurrentUICulture.Name.StartsWith("zh", StringComparison.OrdinalIgnoreCase) ? "zh" : "en";
        }
    }

    /// 实际使用的内置模板语言（auto 跟随界面语言）
    public static string TemplateLocale => TemplateLanguage is "zh" or "en" ? TemplateLanguage : UiLocale;

    /// 当前设置的快照（日志用，不含路径等个人信息）
    public static Dictionary<string, object?> Snapshot() => new()
    {
        ["hotKey"] = HotKey, ["language"] = Language, ["templateLanguage"] = TemplateLanguage, ["copyOnly"] = CopyOnly,
        ["expandPasteEnabled"] = ExpandPasteEnabled, ["forceAsciiInput"] = ForceAsciiInput, ["imeShiftToggle"] = ImeShiftToggle,
        ["recordUsage"] = RecordUsage, ["excludedApps"] = ExcludedApps.Count,
    };
}

/// 排除的程序：exe 完整路径与显示名称（添加时的文件说明）。路径不区分大小写。
internal sealed record ExcludedApp(string Path, string Name)
{
    public bool Is(string path) => string.Equals(Path, path, StringComparison.OrdinalIgnoreCase);
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
