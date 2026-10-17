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
}

public sealed class SyncReport
{
    public uint Pulled { get; set; }
    public uint Pushed { get; set; }
    public uint Conflicts { get; set; }
    public List<string> ConflictFiles { get; set; } = new();
    public uint Rejected { get; set; }
    public uint Alerts { get; set; }
    public uint Deferred { get; set; }
    public bool WritesBlocked { get; set; }
}

public sealed class JoinProgress
{
    public string JoinId { get; set; } = "";
    /// waiting_for_approver / enter_code_on_other_device / enter_code_here / completed / expired
    public string Stage { get; set; } = "";
    public string? Sas { get; set; }
}

public sealed class JoinRequest
{
    public string JoinId { get; set; } = "";
    public long ExpiresAt { get; set; }
    /// waiting_for_new_device / ready_for_code / handled_elsewhere
    public string Status { get; set; } = "";
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

    public SyncException(string kind, string message, int status = 0, string code = "", uint remaining = 0, ulong seconds = 0) : base(message)
    {
        Kind = kind; Status = status; Code = code; Remaining = remaining; Seconds = seconds;
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
            "not_enrolled" => Loc.T("这台设备还没有加入保险库", "This PC hasn't joined the vault"),
            "security" => zh ? Message : "Security check failed. Sync has been paused for safety.",
            "sas_mismatch" => Loc.T($"校验码不正确，还可以再试 {Remaining} 次", $"Wrong code. {Remaining} attempts left."),
            "cooldown" => Loc.T($"错误次数过多，请 {(Seconds + 59) / 60} 分钟后再试", $"Too many attempts. Try again in {(Seconds + 59) / 60} min."),
            "recovery_required" => Loc.T("这个操作需要恢复短语", "This needs your recovery phrase"),
            "conflict" => Loc.T("其他设备同时在操作，请稍后重试", "Another device is busy. Try again shortly."),
            "invalid_input" => zh ? Message : "Invalid input",
            "invalid_state" => zh ? Message : "Not possible right now",
            "storage" => zh ? Message : "Couldn't read or write the encrypted key file",
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
    public SyncClient(string serverUrl, string dataDir, string templateDir, string secretsPath)
    {
        handle = Native.qt_sync_new(U8(serverUrl), U8(dataDir), U8(templateDir), U8(secretsPath));
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
            e["seconds"]?.GetValue<ulong>() ?? 0);
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
    public void ConfirmJoin(string code) => Call("confirm_join", new { code });
    public void CancelJoin() => Call("cancel_join");
    public List<JoinRequest> ListJoinRequests() => Get<List<JoinRequest>>("list_join_requests");
    /// 输入新设备显示的校验码以批准加入，返回要在新设备上输入的确认码
    public string ApproveJoin(string joinId, string sas) => Call("approve_join", new { join_id = joinId, sas })!.GetValue<string>();
    public void RejectJoin(string joinId) => Call("reject_join", new { join_id = joinId });
    public void Recover(string phrase) => Call("recover", new { phrase });
    public SyncReport SyncNow() => Get<SyncReport>("sync_now");
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
        [DllImport(Dll)] public static extern IntPtr qt_sync_new(byte[]? serverUrl, byte[]? dataDir, byte[]? templateDir, byte[]? secretsPath);
        [DllImport(Dll)] public static extern void qt_sync_free(IntPtr c);
        [DllImport(Dll)] public static extern IntPtr qt_sync_call(IntPtr c, byte[]? op, byte[]? args);
    }
}
