using System.Diagnostics;
using Microsoft.UI.Dispatching;
using Qantrip.Core;
using Qantrip.Platform;

namespace Qantrip;

/// <summary>
/// 端到端加密同步的 Windows 外壳（docs/sync-design.md，对应 macOS SyncService.swift）。加密、花名册与协议都在 Rust 核心里；
/// 这里只负责：在后台线程串行调用核心、浏览器登录回调、按触发原因交给核心判断是否同步、把状态交给界面（UI 线程上的属性 + Changed 事件）。
/// 触发模型（docs/sync-design.md v0.4，与 macOS 相同）：每 60 秒 timer（锁屏 / 睡眠 / 显示器关闭时跳过，恢复后 wake）、启动 launch、
/// 唤起面板 panel_opened（后台执行，面板不等待）、模板目录变化 8 秒后 local_change、被节流的本地修改在 next_push_at 再试一次。
/// 是否真的发请求由核心按服务端下发的策略决定；没到期时不联网。
/// </summary>
internal sealed class SyncService
{
    public static SyncService Shared { get; } = new();

    public SyncStatus? Status { get; private set; }
    public bool? RemoteVault { get; private set; }
    public List<JoinRequest> JoinRequests { get; private set; } = new();
    public List<DeviceInfo> Devices { get; private set; } = new();
    public JoinProgress? Join { get; private set; }
    /// 批准加入后要显示、在新设备上输入的确认码（按 join_id）
    public Dictionary<string, string> ConfirmCodes { get; } = new();
    /// 进行中的操作（显示为进度）
    public string? Working { get; private set; }
    public string? Error { get; set; }
    /// 刚创建保险库时的恢复短语，只显示一次
    public string? Phrase { get; set; }
    public SyncReport? LastReport { get; private set; }
    /// 账号下已登录的设备（含未加入保险库、超出设备上限的）
    public AccountDevices? AccountDevices { get; private set; }
    /// 设置 › 同步页面可见时为真：此时才轮询加入请求、读取账号设备
    public bool SettingsVisible { get; set; }

    /// 状态变化（UI 线程）
    public event Action? Changed;
    /// 同步拉取到新内容后调用（重新加载模板）
    public Action? OnTemplatesChanged;

    public bool SignedIn => Status?.SignedIn ?? false;
    public bool Enrolled => (Status?.HasVault ?? false) && (Status?.Enrolled ?? false);
    /// 托盘提示：有设备等待加入
    public int PendingJoinCount => Math.Max((int)(Status?.PendingJoins ?? 0), JoinRequests.Count(r => r.Status != "handled_elsewhere"));

    private SyncClient? client;
    private string server = "";
    private DispatcherQueue? ui;
    private DispatcherQueueTimer? timer, joinTimer, pending, pushTimer;
    /// 锁屏、睡眠或显示器关闭：暂停定时检查
    private bool asleep;
    private readonly SemaphoreSlim gate = new(1, 1);
    private readonly LoopbackReceiver receiver = new();
    /// 仅调试：QANTRIP_DEBUG_SYNC_MOCK 模拟各个界面状态（不调用核心，用于截图）
    private string? mock;

    private SyncService() { }

    public static string DataDirFor(string serverUrl)
    {
        var host = Uri.TryCreate(serverUrl, UriKind.Absolute, out var u) ? u.Host : "default";
        if (u != null && !u.IsDefaultPort) host += "_" + u.Port;
        // 设备身份与密钥只属于这台电脑：放在 %LOCALAPPDATA%，不随漫游配置文件同步到别的电脑（测试时跟随 QANTRIP_DATA_DIR）
        var root = Environment.GetEnvironmentVariable("QANTRIP_DATA_DIR") is { Length: > 0 } d
            ? d
            : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Qantrip");
        return Path.Combine(root, "sync", host);
    }

    /// 启动或切换服务端时调用（UI 线程）。
    public void Start()
    {
        ui ??= DispatcherQueue.GetForCurrentThread();
        if (Settings.DebugHooks && Environment.GetEnvironmentVariable("QANTRIP_DEBUG_SYNC_MOCK") is { Length: > 0 } m)
        {
            if (mock != m) ApplyMock(m);
            return;
        }
        var url = Settings.SyncServer;
        if (url == server && client != null) return;
        server = url;
        var dataDir = DataDirFor(url);
        try
        {
            Directory.CreateDirectory(dataDir);
            var old = client;
            client = new SyncClient(url, dataDir, Settings.TemplatesDir, Path.Combine(dataDir, "secrets.bin"), ThisDevice());
            if (old != null) _ = Task.Run(async () => { await gate.WaitAsync(); try { old.Dispose(); } finally { gate.Release(); } });
            Status = client.Status(); // 不联网
        }
        catch (Exception e)
        {
            Log.Error("sync_start", e);
            client = null;
            Status = null;
            Error = e.Message;
        }
        RemoteVault = null;
        JoinRequests = new();
        Devices = new();
        Join = null;
        AccountDevices = null;
        timer?.Stop();
        // 每分钟让核心判断一次是否到期；没到期时不发任何请求（间隔由服务端下发的同步策略决定）
        timer = ui.CreateTimer();
        timer.Interval = TimeSpan.FromSeconds(60);
        timer.Tick += (_, _) => Trigger(SyncTrigger.Timer);
        timer.Start();
        Log.Event("sync_start", new() { ["server_default"] = url == Settings.DefaultSyncServer, ["signed_in"] = SignedIn, ["enrolled"] = Enrolled });
        Notify();
        Refresh();
        Trigger(SyncTrigger.Launch);
    }

    /// 登录与刷新令牌时上报的设备信息：计算机名、"Windows 11 26200"、应用版本。
    private static DeviceLabel ThisDevice()
    {
        string name = Environment.MachineName;
        try
        {
            int size = 0;
            Win32.GetComputerNameEx(Win32.ComputerNamePhysicalDnsHostname, null, ref size);
            var buf = new char[Math.Max(size, 1)];
            if (size > 0 && Win32.GetComputerNameEx(Win32.ComputerNamePhysicalDnsHostname, buf, ref size) && size > 0) name = new string(buf, 0, size);
        }
        catch (Exception e) { Log.Error("computer_name", e); }
        var v = Environment.OSVersion.Version;
        var platform = $"Windows {(v.Major == 10 && v.Build >= 22000 ? 11 : v.Major)} {v.Build}";
        return new DeviceLabel(name, platform, UI.SettingsView.Version);
    }

    /// 锁屏 / 睡眠 / 显示器关闭（true）与恢复（false）。恢复后补一次检查。
    public void SetAsleep(bool value)
    {
        bool woke = asleep && !value;
        asleep = value;
        Log.Event("sync_asleep", new() { ["asleep"] = value });
        if (woke) Trigger(SyncTrigger.Wake);
    }

    /// 按触发原因交给核心判断是否同步（docs/sync-design.md v0.4）；没到期时核心不发请求。
    public void Trigger(string trigger)
    {
        if (mock != null || !SignedIn || !(Enrolled || Status?.DeviceLimit != null)) return;
        if (asleep && trigger == SyncTrigger.Timer) return;
        Run(null, c => c.MaybeSync(trigger), report =>
        {
            if (report != null) AfterReport(report);
            AfterTrigger();
        }, e =>
        {
            Log.Event("sync_trigger_error", new() { ["trigger"] = trigger, ["kind"] = e.Kind });
            AfterTrigger();
        });
    }

    private void AfterTrigger()
    {
        SchedulePush();
        if ((Status?.PendingJoins ?? 0) > 0 || SettingsVisible) PollJoinRequests();
        if (Status?.DeviceLimit != null && (SettingsVisible || AccountDevices is null)) LoadAccountDevices();
    }

    /// 唤出面板时调用：距上次检查足够久时在后台检查一次，面板不等待。
    public void PanelOpened() => Trigger(SyncTrigger.PanelOpened);

    /// 本地修改被节流时，在核心给出的时间点再试一次自动上传。
    private void SchedulePush()
    {
        pushTimer?.Stop();
        if (ui is null || Status?.NextPushAt is not long at) return;
        var delay = Math.Max(1, at - DateTimeOffset.UtcNow.ToUnixTimeSeconds() + 1);
        pushTimer = ui.CreateTimer();
        pushTimer.Interval = TimeSpan.FromSeconds(delay);
        pushTimer.IsRepeating = false;
        pushTimer.Tick += (_, _) => Trigger(SyncTrigger.LocalChange);
        pushTimer.Start();
    }

    private void Notify() => Changed?.Invoke();

    /// 在后台线程执行一个核心调用，结束后回到 UI 线程。`label` 不为空时显示为进行中的操作，失败时显示错误。
    private void Run<T>(string? label, Func<SyncClient, T> work, Action<T>? done = null, Action<SyncException>? failed = null)
    {
        if (client is not { } c || ui is null) return;
        if (label != null) Working = label;
        Error = null;
        Notify();
        _ = Task.Run(async () =>
        {
            T? value = default;
            Exception? error = null;
            SyncStatus? status = null;
            await gate.WaitAsync();
            try
            {
                try { value = work(c); } catch (Exception e) { error = e; }
                try { status = c.Status(); } catch (Exception e) { Log.Error("sync_status", e); }
            }
            finally { gate.Release(); }
            ui.TryEnqueue(() =>
            {
                if (label != null) Working = null;
                if (status != null) Status = status;
                if (error is null) done?.Invoke(value!);
                else
                {
                    var se = error as SyncException;
                    Log.Event("sync_error", new() { ["op"] = label ?? "background", ["kind"] = se?.Kind ?? error.GetType().Name });
                    if (se != null && failed != null) failed(se);
                    else if (label != null) Error = se?.Describe() ?? error.Message;
                }
                Notify();
            });
        });
    }

    private void Run(string? label, Action<SyncClient> work, Action? done = null) =>
        Run<bool>(label, c => { work(c); return true; }, _ => done?.Invoke());

    /// 读取服务端信息：是否有保险库、在册设备；设置页可见或超出设备上限时再读账号设备。
    public void Refresh()
    {
        if (mock != null || client is null || !SignedIn) return;
        if (Status is { HasVault: false })
            Run(null, c => c.RemoteVaultExists(), v => RemoteVault = v);
        else
            Run(null, c => c.Devices(), v => Devices = v);
        if (SettingsVisible || Status?.DeviceLimit != null) LoadAccountDevices();
    }

    public void LoadAccountDevices()
    {
        if (mock != null) return;
        Run(null, c => c.AccountDevices(), v => AccountDevices = v);
    }

    /// 解除一台设备的登录（超出设备上限时，或旧设备已丢失）；是本机时等同于退出登录。
    public void Release(AccountDevice device) => Run(Loc.T("正在解除…", "Releasing…"), c => c.ReleaseDevice(device.GrantId), () =>
    {
        if (device.IsSelf)
        {
            RemoteVault = null;
            Devices = new();
            JoinRequests = new();
            AccountDevices = null;
            return;
        }
        Refresh();
        // 腾出名额后立即同步一次（超出上限的本机由此恢复）
        Run(null, c => c.MaybeSync(SyncTrigger.Manual), r => { if (r != null) AfterReport(r); SchedulePush(); });
    });

    private void AfterReport(SyncReport report)
    {
        LastReport = report;
        Log.Event("sync", new() { ["pulled"] = report.Pulled, ["pushed"] = report.Pushed, ["conflicts"] = report.Conflicts, ["rejected"] = report.Rejected });
        if (report.Pulled > 0 || report.Conflicts > 0) OnTemplatesChanged?.Invoke();
    }

    private void AfterSync(SyncReport report)
    {
        AfterReport(report);
        SchedulePush();
        Refresh();
    }

    /// 模板目录变化后稍等片刻，合并连续的保存，再交给核心按上传间隔决定是否上传。
    public void ScheduleSync()
    {
        if (mock != null || !Enrolled || ui is null) return;
        pending?.Stop();
        pending = ui.CreateTimer();
        pending.Interval = TimeSpan.FromSeconds(8);
        pending.IsRepeating = false;
        pending.Tick += (_, _) => Trigger(SyncTrigger.LocalChange);
        pending.Start();
    }

    public void SyncNow() => Run(Loc.T("正在同步…", "Syncing…"), c => c.SyncNow(), AfterSync);

    // MARK: 登录

    public void SignIn()
    {
        if (client is not { } c) return;
        Error = null;
        string redirect;
        try
        {
            redirect = receiver.Start(r =>
            {
                if (r.Code is { } code && r.State is { } state)
                {
                    Run(Loc.T("正在登录…", "Signing in…"), cl => cl.FinishSignIn(code, state), _ =>
                    {
                        App.Current?.ActivateSettings();
                        Refresh();
                        Trigger(SyncTrigger.Launch);
                    });
                }
                else
                {
                    Working = null;
                    Error = Loc.T("已取消登录", "Sign-in was cancelled");
                    Notify();
                }
            });
        }
        catch (Exception e)
        {
            Error = Loc.T("无法监听本机端口", "Couldn't open a local port") + $"（{e.Message}）";
            Notify();
            return;
        }
        Working = Loc.T("请在浏览器中完成登录…", "Finish signing in in your browser…");
        Notify();
        _ = Task.Run(() =>
        {
            string? url = null;
            Exception? error = null;
            try { url = c.BeginSignIn(redirect); } catch (Exception e) { error = e; }
            ui?.TryEnqueue(() =>
            {
                if (url != null)
                {
                    if (Settings.DebugHooks)
                    {
                        // 调试构建不打开浏览器（无人值守测试）：记录回调地址，测试脚本直接请求它
                        Log.Event("debug_signin", new() { ["redirect"] = redirect, ["authorize_host"] = new Uri(url).Host });
                        return;
                    }
                    try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
                    catch (Exception e) { Log.Error("open_browser", e); }
                }
                else
                {
                    receiver.Stop();
                    Working = null;
                    Error = (error as SyncException)?.Describe() ?? error?.Message;
                    Notify();
                }
            });
        });
    }

    public void CancelSignIn()
    {
        receiver.Stop();
        Working = null;
        Notify();
    }

    public void SignOut() => Run(Loc.T("正在退出…", "Signing out…"), c => c.SignOut(), () =>
    {
        RemoteVault = null;
        Devices = new();
        JoinRequests = new();
        AccountDevices = null;
    });

    public void ResetDevice() => Run(Loc.T("正在重置…", "Resetting…"), c => c.ResetDevice(), () =>
    {
        RemoteVault = null;
        Devices = new();
        JoinRequests = new();
        AccountDevices = null;
        Join = null;
        Refresh();
    });

    // MARK: 保险库

    public void CreateVault()
    {
        var language = Settings.UiLocale == "zh" ? "zh" : "en";
        Run(Loc.T("正在创建保险库…", "Creating vault…"), c => c.CreateVault(language), phrase =>
        {
            Phrase = phrase;
            SyncNow();
        });
    }

    public void Recover(string phrase) => Run(Loc.T("正在恢复…", "Recovering…"), c => c.Recover(phrase), SyncNow);

    // MARK: 新设备加入

    public void StartJoin() => Run(Loc.T("正在发起加入请求…", "Requesting to join…"), c => c.StartJoin(), progress =>
    {
        Join = progress;
        joinTimer?.Stop();
        joinTimer = ui!.CreateTimer();
        joinTimer.Interval = TimeSpan.FromSeconds(2);
        joinTimer.Tick += (_, _) => PollJoin();
        joinTimer.Start();
    });

    private void PollJoin() => Run(null, c => c.PollJoin(), progress =>
    {
        Join = progress;
        switch (progress.Stage)
        {
            case "completed":
                joinTimer?.Stop();
                Join = null;
                SyncNow();
                break;
            case "expired" or "enter_code_here":
                joinTimer?.Stop();
                break;
        }
    });

    /// 新设备：输入已登录设备上显示的确认码，完成双向确认后才加入。
    public void ConfirmJoin(string code) => Run(Loc.T("正在验证…", "Verifying…"), c => c.ConfirmJoin(code.Trim()), () =>
    {
        Join = null;
        SyncNow();
    });

    public void CancelJoin()
    {
        joinTimer?.Stop();
        Join = null;
        if (mock != null) { Notify(); return; }
        Run(null, c => c.CancelJoin());
    }

    // MARK: 已登录设备批准加入

    public void PollJoinRequests()
    {
        if (mock != null || !Enrolled) return;
        Run(null, c => c.ListJoinRequests(), v => JoinRequests = v);
    }

    public void Approve(string joinId, string code) => Run(Loc.T("正在验证…", "Verifying…"), c => c.ApproveJoin(joinId, code.Trim()), confirm =>
    {
        ConfirmCodes[joinId] = confirm;
        Refresh();
    });

    public void Reject(string joinId) => Run(null, c => c.RejectJoin(joinId), Refresh);

    // MARK: 设备管理

    /// 撤销设备并轮换密钥。需要恢复短语时调用 needsPhrase。
    public void Revoke(string deviceId, string? phrase, Action needsPhrase) =>
        Run(Loc.T("正在撤销并轮换密钥…", "Removing and rotating keys…"), c => c.RevokeDevice(deviceId, phrase), _ => Refresh(), e =>
        {
            if (e.Kind == "recovery_required") needsPhrase(); else Error = e.Describe();
        });

    public void RotateKeys() => Run(Loc.T("正在轮换密钥…", "Rotating keys…"), c => c.RotateKeys(), _ => Refresh());

    /// 重新验证：服务端恢复正常后解除停止写入。
    public void Reverify() => Run(Loc.T("正在重新验证…", "Re-verifying…"), c => c.Reverify(), _ => Refresh());

    public void DismissConfirmCode(string joinId)
    {
        ConfirmCodes.Remove(joinId);
        Notify();
    }

    public void DismissAlerts()
    {
        if (mock != null) { Status!.Alerts.Clear(); Notify(); return; }
        Run(null, c => c.DismissAlerts());
    }

    // MARK: 调试：模拟界面状态

    private void ApplyMock(string m)
    {
        mock = m;
        var now = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        Status = new SyncStatus { SignedIn = m != "signedout", Account = m == "signedout" ? null : "octocat", DeviceId = "K7Q2M4X9TRV3WZ8H5N6PJ2CDFA" };
        AccountDevices = new AccountDevices
        {
            MaxDevices = 3,
            Devices = new()
            {
                new AccountDevice { GrantId = "g0", DeviceId = "K7Q2M4X9TRV3WZ8H5N6PJ2CDFA", Name = "DESKTOP-QANTRIP", Platform = "Windows 11 26200", AppVersion = "0.3.1", LastSeenAt = now - 30, IsSelf = true, InVault = m != "limit", Limited = m == "limit" },
                new AccountDevice { GrantId = "g1", DeviceId = "B3D5F7H9JKMNPQRSTVWXYZ2345", Name = "MacBook Pro", Platform = "macOS 26.1", AppVersion = "0.3.1", LastSeenAt = now - 7200, InVault = true },
                new AccountDevice { GrantId = "g2", DeviceId = "C4E6G8J2KLMNPQRSTVWXYZ3456", Name = "Mac mini", Platform = "macOS 26.0", AppVersion = "0.3.0", LastSeenAt = now - 86400 * 3 },
            },
        };
        switch (m)
        {
            case "novault": RemoteVault = false; break;
            case "join": RemoteVault = true; break;
            case "join-sas": RemoteVault = true; Join = new JoinProgress { JoinId = "j1", Stage = "enter_code_on_other_device", Sas = "482 913" }; break;
            case "join-code": RemoteVault = true; Join = new JoinProgress { JoinId = "j1", Stage = "enter_code_here" }; break;
            case "phrase":
                RemoteVault = false;
                Phrase = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual";
                break;
            case "limit":
                Status.DeviceLimit = 3;
                break;
            case "enrolled" or "blocked":
                Status.HasVault = true;
                Status.Enrolled = true;
                Status.LastSync = now - 125;
                Status.Usage = new SyncUsage { Bytes = 3_400_000, QuotaBytes = 4_000_000, Records = 120 };
                Status.NextPushAt = now + 600;
                Status.WritesBlocked = m == "blocked";
                if (m == "blocked") Status.Alerts.Add(new SyncAlert { Code = "rollback", Message = "服务器返回的花名册比本机记录的旧，可能被回滚", At = now - 60 });
                Devices = new()
                {
                    new DeviceInfo { DeviceId = "K7Q2M4X9TRV3WZ8H5N6PJ2CDFA", IsSelf = true, RosterIndex = 0 },
                    new DeviceInfo { DeviceId = "B3D5F7H9JKMNPQRSTVWXYZ2345", RosterIndex = 1 },
                    new DeviceInfo { DeviceId = "D5F7H9JKMNPQRSTVWXYZ234567", RosterIndex = 2 }, // 已退出登录、仍在保险库中
                };
                JoinRequests = new() { new JoinRequest { JoinId = "r1", Status = "ready_for_code", ExpiresAt = now + 600 } };
                LastReport = new SyncReport { Pulled = 1, Conflicts = 1 };
                ConfirmCodes["r0"] = "719 204";
                break;
        }
        Log.Event("sync_mock", new() { ["state"] = m });
        Notify();
    }
}
