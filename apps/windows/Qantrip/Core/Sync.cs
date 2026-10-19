using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Qantrip.Core;

// 端到端加密同步客户端（core/src/sync，经 qantrip-ffi 的 qt_sync_* 接入）。所有调用都会阻塞（可能联网），
// 只能在后台线程调用；核心内部串行执行。与 core/src/sync/mod.rs 的对外类型一一对应。

public sealed class SyncAlert
{
    public string Code { get; set; } = "";
    public string Message { get; set; } = "";
    public long At { get; set; }
}

public sealed class SyncStatus
{
    public bool SignedIn { get; set; }
    public string? Account { get; set; }
    public string? DeviceId { get; set; }
    /// 本机已加入保险库（本地有保险库状态）
    public bool HasVault { get; set; }
    /// 本机在花名册中在册
    public bool Enrolled { get; set; }
    public long? LastSync { get; set; }
    /// 因安全原因停止写入
    public bool WritesBlocked { get; set; }
    public List<SyncAlert> Alerts { get; set; } = new();
    public bool Busy { get; set; }
    /// 保险库用量（最近一次版本检查或 account_usage 的结果）
    public SyncUsage? Usage { get; set; }
    /// 其他设备发起的待处理加入请求数（最近一次版本检查）
    public uint PendingJoins { get; set; }
    public SyncPolicy Policy { get; set; } = new();
    /// 被节流的本地变化将自动上传的时间（unix 秒）
    public long? NextPushAt { get; set; }
    /// 服务端报告本机超出设备数量限制时为上限
    public uint? DeviceLimit { get; set; }
}

/// 保险库用量（服务端统计的密文大小与记录数）
public sealed class SyncUsage
{
    public ulong Bytes { get; set; }
    public ulong QuotaBytes { get; set; }
    public uint Records { get; set; }
}

/// 服务端下发的同步策略
public sealed class SyncPolicy
{
    public uint CheckIntervalSecs { get; set; } = 3600;
    public uint PanelCheckSecs { get; set; } = 900;
    public uint MinPushIntervalSecs { get; set; } = 900;
}

/// 此账号已登录的一台设备（含超出设备数量限制、未加入保险库的）
public sealed class AccountDevice
{
    public string GrantId { get; set; } = "";
    public string DeviceId { get; set; } = "";
    public string Name { get; set; } = "";
    public string Platform { get; set; } = "";
    public string AppVersion { get; set; } = "";
    public long CreatedAt { get; set; }
    public long LastSeenAt { get; set; }
    public bool IsSelf { get; set; }
    /// 超出设备数量限制，不能访问保险库
    public bool Limited { get; set; }
    /// 在本机已验证的花名册中在册（本机没有保险库状态时为 false）
    public bool InVault { get; set; }
}

public sealed class AccountDevices
{
    public uint MaxDevices { get; set; }
    /// 最近活跃的在前
    public List<AccountDevice> Devices { get; set; } = new();
}

/// 触发同步的原因（core SyncTrigger）
public static class SyncTrigger
{
    public const string Manual = "manual", Timer = "timer", PanelOpened = "panel_opened", LocalChange = "local_change", Launch = "launch", Wake = "wake";
}

/// 登录与刷新令牌时上报给服务端的设备信息（core DeviceLabel）
public sealed record DeviceLabel(string Name, string Platform, string AppVersion);

public sealed class SyncReport
{
    public uint Pulled { get; set; }
    public uint Pushed { get; set; }
    /// 同一模板在两台设备上都改过时以最后修改为准，被覆盖的版本备份到历史版本目录：本次备份的版本数
    public uint Replaced { get; set; }
    public List<string> ReplacedFiles { get; set; } = new();
    public uint Rejected { get; set; }
    public uint Alerts { get; set; }
    public uint Deferred { get; set; }
    public bool WritesBlocked { get; set; }
}

public sealed class JoinProgress
{
    public string JoinId { get; set; } = "";
    /// waiting_for_approver / show_code / completed / expired
    public string Stage { get; set; } = "";
    public string? Sas { get; set; }
}

public sealed class JoinRequest
{
    public string JoinId { get; set; } = "";
    public long ExpiresAt { get; set; }
    /// waiting_for_new_device / ready_for_code / handled_elsewhere / incompatible
    public string Status { get; set; } = "";
    /// ready_for_code 时的三个候选校验码（其中一个与新设备上显示的相同）
    public List<string> Choices { get; set; } = new();
}

public sealed class DeviceInfo
{
    public string DeviceId { get; set; } = "";
    public bool IsSelf { get; set; }
    public uint RosterIndex { get; set; }
}

/// 核心 SyncError（`kind` 为变体名的 snake_case）。界面文字用 <see cref="Describe"/>。
public sealed class SyncException : Exception
{
    public string Kind { get; }
    public int Status { get; }
    public string Code { get; }
    public uint Remaining { get; }
    public ulong Seconds { get; }
    public uint MaxDevices { get; }

    public SyncException(string kind, string message, int status = 0, string code = "", uint remaining = 0, ulong seconds = 0, uint maxDevices = 0) : base(message)
    {
        Kind = kind; Status = status; Code = code; Remaining = remaining; Seconds = seconds; MaxDevices = maxDevices;
    }

    /// 界面文字（核心的错误信息是中文，这里按界面语言重新表述；对应 macOS SyncService.swift 的 describe）。
    public string Describe()
    {
        bool zh = !Loc.En;
        return Kind switch
        {
            "not_signed_in" => Loc.T("需要重新登录", "Please sign in again"),
            "network" => Loc.T("无法连接同步服务器，请检查网络", "Can't reach the sync server. Check your connection."),
            "server" => zh ? Message : $"Server error {Status} ({Code})",
            "no_vault" => Loc.T("还没有保险库", "There is no vault yet"),
            "not_enrolled" => Loc.T("这台设备还没有加入保险库", "This device hasn't joined the vault"),
            "security" => zh ? Message : "Security check failed. Sync has been paused for safety.",
            "code_mismatch" => Loc.T("选的码和新设备上显示的不一样，这次请求已取消。请在新设备上重新开始。", "That isn't the code shown on the new device, so the request was cancelled. Start again on the new device."),
            "recovery_required" => Loc.T("这个操作需要恢复短语", "This needs your recovery phrase"),
            "conflict" => Loc.T("其他设备同时在操作，请稍后重试", "Another device is busy. Try again shortly."),
            "invalid_input" => zh ? Message : "Invalid input",
            "invalid_state" => zh ? Message : "Not possible right now",
            "storage" => zh ? Message : "Couldn't read or write the encrypted key file",
            "device_limit" => Loc.T($"此账号最多同时在 {MaxDevices} 台设备上同步", $"This account can sync on up to {MaxDevices} devices at a time"),
            _ => Message,
        };
    }
}

/// qantrip-ffi 的同步客户端句柄。线程安全；方法会阻塞。
public sealed class SyncClient : IDisposable
{
    private IntPtr handle;

    /// <param name="serverUrl">后端地址（如 https://qantrip.nekro.ai）</param>
    /// <param name="dataDir">同步状态目录</param>
    /// <param name="templateDir">用户模板目录</param>
    /// <param name="secretsPath">DPAPI 加密的密钥文件</param>
    /// <param name="device">登录与刷新令牌时上报的设备名称、平台与应用版本</param>
    public SyncClient(string serverUrl, string dataDir, string templateDir, string secretsPath, DeviceLabel device)
    {
        handle = Native.qt_sync_new(U8(serverUrl), U8(dataDir), U8(templateDir), U8(secretsPath), U8(device.Name), U8(device.Platform), U8(device.AppVersion));
        if (handle == IntPtr.Zero) throw new InvalidOperationException("qt_sync_new failed");
    }

    /// 调用一个操作，返回 value（可能为 null）；失败时抛出 SyncException。
    public JsonNode? Call(string op, object? args = null)
    {
        var p = Native.qt_sync_call(handle, U8(op), args is null ? null : U8(JsonSerializer.Serialize(args)));
        if (p == IntPtr.Zero) throw new SyncException("invalid_state", "qt_sync_call failed");
        string json;
        try { json = Marshal.PtrToStringUTF8(p) ?? "{}"; }
        finally { Native.qt_string_free(p); }
        var root = JsonNode.Parse(json)!;
        if (root["ok"]?.GetValue<bool>() == true) return root["value"];
        var e = root["error"]!;
        throw new SyncException(
            e["kind"]?.GetValue<string>() ?? "unknown",
            e["message"]?.GetValue<string>() ?? "",
            e["status"]?.GetValue<int>() ?? 0,
            e["code"]?.GetValue<string>() ?? "",
            e["remaining"]?.GetValue<uint>() ?? 0,
            e["seconds"]?.GetValue<ulong>() ?? 0,
            e["max_devices"]?.GetValue<uint>() ?? 0);
    }

    private T Get<T>(string op, object? args = null) => Call(op, args).Deserialize<T>(Engine.Json)!;

    public SyncStatus Status() => Get<SyncStatus>("status");
    public bool RemoteVaultExists() => Call("remote_vault_exists")!.GetValue<bool>();
    /// 开始 PKCE 登录：返回在浏览器中打开的授权地址
    public string BeginSignIn(string redirectUri) => Call("begin_sign_in", new { redirect_uri = redirectUri })!.GetValue<string>();
    /// 用回调收到的授权码换取令牌，返回 GitHub 用户名
    public string FinishSignIn(string code, string state) => Call("finish_sign_in", new { code, state })!.GetValue<string>();
    public void SignOut() => Call("sign_out");
    public void ResetDevice() => Call("reset_device");
    /// 创建保险库，返回 24 个词的恢复短语（只显示一次）。language：zh / en
    public string CreateVault(string language) => Call("create_vault", new { language })!.GetValue<string>();
    public JoinProgress StartJoin() => Get<JoinProgress>("start_join");
    public JoinProgress PollJoin() => Get<JoinProgress>("poll_join");
    public void CancelJoin() => Call("cancel_join");
    public List<JoinRequest> ListJoinRequests() => Get<List<JoinRequest>>("list_join_requests");
    /// 从三个候选码中选出新设备上显示的那个以批准加入；选错时抛出 code_mismatch 并取消这次请求
    public void ApproveJoin(string joinId, string code) => Call("approve_join", new { join_id = joinId, code });
    /// 历史版本目录（同步时被较新版本覆盖的模板备份在这里，保留 30 天；目录可能还不存在）
    public string HistoryDir() => Call("history_dir")!.GetValue<string>();
    public void RejectJoin(string joinId) => Call("reject_join", new { join_id = joinId });
    public void Recover(string phrase) => Call("recover", new { phrase });
    public SyncReport SyncNow() => Get<SyncReport>("sync_now");
    /// 按触发原因与服务端策略决定是否同步（<see cref="SyncTrigger"/>）；返回 null 表示没有发出任何请求
    public SyncReport? MaybeSync(string trigger) => Call("maybe_sync", new { trigger }).Deserialize<SyncReport?>(Engine.Json);
    /// 此账号已登录的设备（超出设备数量限制的设备也可以调用）
    public AccountDevices AccountDevices() => Get<AccountDevices>("account_devices");
    /// 让一台设备退出此账号（吊销其登录授权）；是本机时等同于退出登录
    public void ReleaseDevice(string grantId) => Call("release_device", new { grant_id = grantId });
    /// 账号的保险库用量（没有保险库时为 null）
    public SyncUsage? AccountUsage() => Call("account_usage").Deserialize<SyncUsage?>(Engine.Json);
    public List<DeviceInfo> Devices() => Get<List<DeviceInfo>>("devices");
    public SyncReport RevokeDevice(string deviceId, string? recoveryPhrase) => Get<SyncReport>("revoke_device", new { device_id = deviceId, recovery_phrase = recoveryPhrase });
    public SyncReport RotateKeys() => Get<SyncReport>("rotate_keys");
    public SyncStatus Reverify() => Get<SyncStatus>("reverify");
    public void DismissAlerts() => Call("dismiss_alerts");

    public void Dispose()
    {
        if (handle != IntPtr.Zero) Native.qt_sync_free(handle);
        handle = IntPtr.Zero;
    }

    private static byte[]? U8(string? s) => s is null ? null : Encoding.UTF8.GetBytes(s + "\0");

    private static class Native
    {
        private const string Dll = "qantrip_ffi";
        [DllImport(Dll)] public static extern void qt_string_free(IntPtr s);
        [DllImport(Dll)] public static extern IntPtr qt_sync_new(byte[]? serverUrl, byte[]? dataDir, byte[]? templateDir, byte[]? secretsPath, byte[]? deviceName, byte[]? platform, byte[]? appVersion);
        [DllImport(Dll)] public static extern void qt_sync_free(IntPtr c);
        [DllImport(Dll)] public static extern IntPtr qt_sync_call(IntPtr c, byte[]? op, byte[]? args);
    }
}
