using System.Diagnostics;
using System.Net;
using System.Text.Json;
using Microsoft.UI.Dispatching;

namespace Qantrip;

/// <summary>
/// 更新提醒（不自动更新，与 macOS 行为一致）：查询 GitHub 上最新的正式版本（忽略草稿与预发布），
/// 比当前版本新时在托盘发一次通知（每个版本一次）、托盘菜单显示"有新版本"，设置 › 关于中可手动检查、下载或跳过此版本。
/// 自动检查：启动约 10 秒后（距上次检查不足 12 小时则跳过）、之后每 24 小时；失败时不打扰。只在 UI 线程上使用。
/// </summary>
internal sealed class UpdateChecker
{
    public static UpdateChecker Shared { get; } = new();

    public sealed record Release(string Version, string Url);

    private const string Api = "https://api.github.com/repos/NekroAI/qantrip/releases/latest";
    private static readonly TimeSpan LaunchMinInterval = TimeSpan.FromHours(12);
    private static readonly TimeSpan Interval = TimeSpan.FromHours(24);
    private static readonly HttpClient http = new() { Timeout = TimeSpan.FromSeconds(15) };

    /// 比当前版本新的正式版本（含已跳过的版本）
    public Release? Available { get; private set; }
    public bool Checking { get; private set; }
    /// 最近一次手动检查的错误（自动检查失败不显示）
    public string? Error { get; private set; }
    /// 最近一次手动检查确认已是最新版本
    public bool UpToDate { get; private set; }
    /// 状态变化（设置 › 关于刷新）
    public event Action? Changed;
    /// 自动检查发现了尚未提醒过、也没有跳过的新版本
    public event Action<Release>? Found;

    private DispatcherQueueTimer? first, timer;

    public static string Current => UI.SettingsView.Version;

    /// 托盘菜单中显示的新版本（跳过的版本不显示）
    public Release? Visible => Available is { } r && r.Version != Settings.UpdateSkippedVersion ? r : null;

    public void Start(DispatcherQueue queue)
    {
        // 上次检查得到的新版本：重启后托盘菜单与关于页仍然显示
        if (Settings.UpdateLatestVersion is { } v && Settings.UpdateLatestUrl is { } url && IsNewer(v, Current)) Available = new Release(v, url);
        first = queue.CreateTimer();
        first.Interval = TimeSpan.FromSeconds(10);
        first.IsRepeating = false;
        first.Tick += (_, _) => _ = Auto(LaunchMinInterval);
        first.Start();
        timer = queue.CreateTimer();
        timer.Interval = TimeSpan.FromHours(1);
        timer.IsRepeating = true;
        timer.Tick += (_, _) => _ = Auto(Interval);
        timer.Start();
    }

    private async Task Auto(TimeSpan minInterval)
    {
        if (!Settings.AutoCheckUpdates || Checking) return;
        var since = DateTimeOffset.UtcNow - Settings.UpdateLastCheck;
        if (since >= TimeSpan.Zero && since < minInterval) return;
        await Check(manual: false);
    }

    /// 设置 › 关于 › 检查更新
    public Task CheckNow() => Check(manual: true);

    private async Task Check(bool manual)
    {
        if (Checking) return;
        Checking = true;
        if (manual) { Error = null; UpToDate = false; }
        Changed?.Invoke();
        // 记录尝试时间（失败也算），自动检查因此不会频繁重试
        Settings.UpdateLastCheck = DateTimeOffset.UtcNow;
        try
        {
            var latest = await FetchLatest();
            Available = latest is { } r && IsNewer(r.Version, Current) ? r : null;
            Settings.UpdateLatestVersion = Available?.Version;
            Settings.UpdateLatestUrl = Available?.Url;
            UpToDate = Available is null;
            Log.Event("update_check", new() { ["manual"] = manual, ["latest"] = latest?.Version, ["newer"] = Available is not null });
            if (Visible is { } found && Settings.UpdateNotifiedVersion != found.Version)
            {
                Settings.UpdateNotifiedVersion = found.Version;
                // 手动检查时结果已经显示在关于页，不再弹通知
                if (!manual) Found?.Invoke(found);
            }
        }
        catch (Exception e)
        {
            Log.Event("update_check", new() { ["manual"] = manual, ["error"] = e.GetType().Name });
            if (manual) Error = Describe(e);
        }
        finally
        {
            Checking = false;
            Changed?.Invoke();
        }
    }

    /// 最新的正式版本；仓库还没有正式版本（404）或最新的是草稿 / 预发布时为 null。
    private static async Task<Release?> FetchLatest()
    {
        using var req = new HttpRequestMessage(HttpMethod.Get, Api);
        req.Headers.UserAgent.ParseAdd($"Qantrip/{Current}");
        req.Headers.Accept.ParseAdd("application/vnd.github+json");
        req.Headers.Add("X-GitHub-Api-Version", "2022-11-28");
        using var resp = await http.SendAsync(req);
        if (resp.StatusCode == HttpStatusCode.NotFound) return null;
        resp.EnsureSuccessStatusCode();
        using var doc = JsonDocument.Parse(await resp.Content.ReadAsStringAsync());
        var root = doc.RootElement;
        static bool Flag(JsonElement e, string name) => e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;
        if (Flag(root, "draft") || Flag(root, "prerelease")) return null;
        var tag = root.TryGetProperty("tag_name", out var t) && t.ValueKind == JsonValueKind.String ? t.GetString() : null;
        var url = root.TryGetProperty("html_url", out var u) && u.ValueKind == JsonValueKind.String ? u.GetString() : null;
        if (string.IsNullOrEmpty(tag) || Parse(tag) is null) throw new InvalidDataException("tag_name");
        // 只打开 GitHub 上的发布页
        if (url is null || !url.StartsWith("https://github.com/", StringComparison.OrdinalIgnoreCase)) url = "https://github.com/NekroAI/qantrip/releases/latest";
        return new Release(tag.TrimStart('v', 'V'), url);
    }

    private static string Describe(Exception e) => e switch
    {
        TaskCanceledException => Loc.T("连接超时", "the connection timed out"),
        HttpRequestException { StatusCode: { } code } => Loc.T($"服务器返回 {(int)code}", $"the server returned {(int)code}"),
        HttpRequestException => Loc.T("无法连接到 GitHub", "couldn't reach GitHub"),
        _ => Loc.T("无法读取版本信息", "couldn't read the release information"),
    };

    /// "v1.2.3" / "1.2" / "1.2.3-beta" → [1, 2, 3]；无法识别时为 null
    public static int[]? Parse(string version)
    {
        var v = version.Trim().TrimStart('v', 'V');
        int cut = v.IndexOfAny(new[] { '-', '+', ' ' });
        if (cut >= 0) v = v[..cut];
        var parts = v.Split('.');
        if (parts.Length is 0 or > 4) return null;
        var nums = new int[parts.Length];
        for (int i = 0; i < parts.Length; i++)
            if (!int.TryParse(parts[i], System.Globalization.NumberStyles.None, System.Globalization.CultureInfo.InvariantCulture, out nums[i])) return null;
        return nums;
    }

    /// 按数字逐段比较（缺少的段视为 0）
    public static bool IsNewer(string candidate, string current)
    {
        if (Parse(candidate) is not { } a || Parse(current) is not { } b) return false;
        for (int i = 0; i < Math.Max(a.Length, b.Length); i++)
        {
            int x = i < a.Length ? a[i] : 0, y = i < b.Length ? b[i] : 0;
            if (x != y) return x > y;
        }
        return false;
    }

    /// 在默认浏览器中打开发布页
    public static void OpenReleasePage(Release r)
    {
        try { Process.Start(new ProcessStartInfo(r.Url) { UseShellExecute = true }); }
        catch (Exception e) { Log.Error("open_release", e); }
    }
}
