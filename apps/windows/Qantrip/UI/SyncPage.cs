using System.Text;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Qantrip.Core;

namespace Qantrip.UI;

/// <summary>
/// 设置 › 同步（docs/sync-design.md，对应 macOS SyncSettingsView.swift）。按状态分为：
/// 未登录 → 登录；已登录、本机没有保险库 → 创建（恢复短语只显示一次）/ 加入（双向校验码）/ 用恢复短语恢复；
/// 已加入 → 账号、上次同步、云端用量、立即同步、安全提醒、加入请求、设备（账号设备 + 已退出登录但仍在保险库中的设备）、轮换密钥、重置；
/// 超出设备上限 → 列出其他设备，解除一台后本机即可同步；最后是高级（服务端地址）。
/// 状态来自 <see cref="SyncService"/>；状态签名变化时整页重建，输入框实例跨重建保留（不丢失正在输入的内容与焦点）。
/// </summary>
internal sealed class SyncPage
{
    private readonly FrameworkElement host;
    private readonly StackPanel page = new() { Spacing = 4, MaxWidth = 860, Padding = new Thickness(28, 8, 28, 28) };
    /// 当前页面中的输入框（按 id）；重建时新建输入框并还原文字与焦点（不移动旧元素：WinUI 中改换父元素容易失败）
    private Dictionary<string, TextBox> inputs = new();
    private readonly Dictionary<string, string> texts = new();
    private readonly SyncService sync = SyncService.Shared;
    private DispatcherTimerLike? poll;
    private string signature = "";
    private bool recoverMode;
    private bool dialogOpen;

    public FrameworkElement View => page;

    /// 所在窗口（文件保存对话框、剪贴板）
    private readonly IntPtr hwnd;

    public SyncPage(FrameworkElement host, IntPtr hwnd)
    {
        this.host = host;
        this.hwnd = hwnd;
    }

    public void Attach()
    {
        sync.Changed += OnChanged;
        sync.SettingsVisible = true;
        sync.Start();
        sync.Refresh();
        Render(force: true);
        // 设置页打开期间，较快地刷新加入请求
        poll = new DispatcherTimerLike(page.DispatcherQueue, TimeSpan.FromSeconds(4), () => sync.PollJoinRequests());
    }

    public void Detach()
    {
        sync.Changed -= OnChanged;
        sync.SettingsVisible = false;
        poll?.Stop();
        poll = null;
    }

    private void OnChanged() => Render(force: false);

    // MARK: 渲染

    /// 影响界面结构的状态；不变时不重建（避免定时刷新打断输入）
    private string Signature()
    {
        var s = sync.Status;
        var b = new StringBuilder();
        b.Append($"{s?.SignedIn}|{s?.Account}|{s?.HasVault}|{s?.Enrolled}|{s?.LastSync}|{s?.WritesBlocked}|{s?.Alerts.Count}|{s?.DeviceId}|");
        b.Append($"{sync.RemoteVault}|{sync.Join?.Stage}|{sync.Join?.Sas}|{sync.Working}|{sync.Error}|{sync.Phrase != null}|{sync.LastReport?.Conflicts}|{recoverMode}|");
        foreach (var r in sync.JoinRequests) b.Append($"{r.JoinId}:{r.Status},");
        b.Append('|');
        foreach (var d in sync.Devices) b.Append($"{d.DeviceId}:{d.IsSelf},");
        b.Append('|');
        foreach (var c in sync.ConfirmCodes) b.Append($"{c.Key}:{c.Value},");
        b.Append($"|{s?.Usage?.Bytes}/{s?.Usage?.QuotaBytes}|{s?.NextPushAt}|{s?.DeviceLimit}|{sync.AccountDevices?.MaxDevices}|");
        foreach (var d in sync.AccountDevices?.Devices ?? new()) b.Append($"{d.GrantId}:{d.Name}:{d.Platform}:{d.AppVersion}:{d.LastSeenAt}:{d.IsSelf}:{d.Limited}:{d.InVault},");
        b.Append('|').Append(Settings.SyncServer).Append('|').Append(Loc.En);
        return b.ToString();
    }

    private void Render(bool force)
    {
        var sig = Signature();
        if (!force && sig == signature) { UpdateRelative(); return; }
        signature = sig;
        // 记住焦点所在的输入框，重建后还原
        var focused = inputs.FirstOrDefault(kv => kv.Value.FocusState != FocusState.Unfocused).Key;
        foreach (var (id, t) in inputs) texts[id] = t.Text;
        inputs = new();
        page.Children.Clear();
        Build();
        if (focused != null && inputs.TryGetValue(focused, out var box))
        {
            box.Focus(FocusState.Programmatic);
            box.Select(box.Text.Length, 0);
        }
        if (sync.Phrase != null && !dialogOpen)
        {
            if (host.XamlRoot != null) _ = ShowPhrase(sync.Phrase);
            else page.Loaded += ShowPhraseWhenLoaded;
        }
    }

    private void ShowPhraseWhenLoaded(object sender, RoutedEventArgs e)
    {
        page.Loaded -= ShowPhraseWhenLoaded;
        if (sync.Phrase != null && !dialogOpen && host.XamlRoot != null) _ = ShowPhrase(sync.Phrase);
    }

    private TextBox Input(string id, string placeholder, bool mono = false, double width = double.NaN, bool multiline = false)
    {
        var t = new TextBox { PlaceholderText = placeholder, IsSpellCheckEnabled = false, Width = width, Text = texts.GetValueOrDefault(id, "") };
        if (mono) t.FontFamily = Theme.Mono;
        if (multiline) { t.AcceptsReturn = true; t.TextWrapping = TextWrapping.Wrap; t.Height = 96; }
        AutomationProperties.SetAutomationId(t, "sync_" + id);
        AutomationProperties.SetName(t, placeholder);
        t.TextChanged += (_, _) => texts[id] = t.Text;
        inputs[id] = t;
        return t;
    }

    private static Button Btn(string id, string text, Action click, bool accent = false, bool enabled = true)
    {
        var b = new Button { Content = text, IsEnabled = enabled };
        if (accent) b.Style = (Style)Application.Current.Resources["AccentButtonStyle"];
        AutomationProperties.SetAutomationId(b, "sync_" + id);
        b.Click += (_, _) => click();
        return b;
    }

    private static TextBlock Text(string text, bool secondary = false, double size = 14, bool bold = false, bool mono = false)
    {
        var t = new TextBlock { Text = text, TextWrapping = TextWrapping.Wrap, FontSize = size, IsTextSelectionEnabled = mono };
        if (secondary) t.Foreground = SettingsView.Res("TextFillColorSecondaryBrush");
        if (bold) t.FontWeight = FontWeights.SemiBold;
        if (mono) t.FontFamily = Theme.Mono;
        return t;
    }

    private static Border Box(params UIElement[] children)
    {
        var sp = new StackPanel { Spacing = 10 };
        foreach (var c in children) sp.Children.Add(c);
        return new Border
        {
            Child = sp, Padding = new Thickness(16, 14, 16, 14), CornerRadius = new CornerRadius(6), BorderThickness = new Thickness(1),
            Background = SettingsView.Res("CardBackgroundFillColorDefaultBrush"), BorderBrush = SettingsView.Res("CardStrokeColorDefaultBrush"),
        };
    }

    private static StackPanel Row(params UIElement[] children)
    {
        var sp = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        foreach (var c in children) sp.Children.Add(c);
        return sp;
    }

    /// 左右两端对齐的一行
    private static Grid Spread(UIElement left, UIElement right)
    {
        var g = new Grid();
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.Children.Add(left);
        Grid.SetColumn((FrameworkElement)right, 1);
        g.Children.Add(right);
        return g;
    }

    private static TextBlock Big(string code, string id)
    {
        var t = new TextBlock { Text = code, FontFamily = Theme.Mono, FontSize = 32, FontWeight = FontWeights.SemiBold, CharacterSpacing = 200, IsTextSelectionEnabled = true };
        AutomationProperties.SetAutomationId(t, "sync_" + id);
        return t;
    }

    private bool Busy => sync.Working != null;

    private void Build()
    {
        page.Children.Add(new TextBlock { Text = Loc.T("同步", "Sync"), FontFamily = Theme.Display, FontSize = 30, Margin = new Thickness(0, 0, 0, 4) });
        page.Children.Add(Text(Loc.T("端到端加密：密钥只在你的设备上，服务器只保存无法解密的密文。", "End-to-end encrypted: keys stay on your devices, and the server only stores ciphertext it can't decrypt."), secondary: true, size: 13));

        if (sync.Working is { } working)
        {
            var row = Row(new ProgressRing { IsActive = true, Width = 18, Height = 18 }, Text(working));
            if (!sync.SignedIn) row.Children.Add(Btn("cancel", Loc.T("取消", "Cancel"), sync.CancelSignIn));
            AutomationProperties.SetAutomationId(row, "sync_working");
            page.Children.Add(new Border { Child = row, Margin = new Thickness(0, 12, 0, 0) });
        }
        if (sync.Error is { } error)
        {
            var bar = new InfoBar { IsOpen = true, Severity = InfoBarSeverity.Error, Message = error, IsClosable = true, Margin = new Thickness(0, 12, 0, 0) };
            AutomationProperties.SetAutomationId(bar, "sync_error");
            bar.Closed += (_, _) => { sync.Error = null; };
            page.Children.Add(bar);
        }

        if (!sync.SignedIn) SignedOut();
        else if (sync.Status?.DeviceLimit is uint max) DeviceLimit(max);
        else if (!(sync.Status?.HasVault ?? false)) NoVault();
        else Enrolled();

        Advanced();
    }

    private void SignedOut()
    {
        page.Children.Add(SettingsView.Section(Loc.T("账号", "Account")));
        page.Children.Add(Box(
            Text(Loc.T("在多台设备之间同步你的模板", "Sync your templates across your devices"), bold: true),
            Text(Loc.T("使用 GitHub 账号登录。登录在浏览器中完成，完成后回到 Qantrip。", "Sign in with your GitHub account. Sign-in happens in your browser; return to Qantrip afterwards."), secondary: true, size: 13),
            Row(Btn("signin", Loc.T("使用 GitHub 登录", "Sign in with GitHub"), sync.SignIn, accent: true, enabled: !Busy))));
    }

    private void AccountCard()
    {
        page.Children.Add(SettingsView.Section(Loc.T("账号", "Account")));
        page.Children.Add(Box(Spread(
            Row(Text(Loc.T("账号", "Account"), secondary: true), Text(sync.Status?.Account ?? "", bold: true)),
            Btn("signout", Loc.T("退出登录", "Sign out"), sync.SignOut, enabled: !Busy))));
    }

    private void NoVault()
    {
        AccountCard();
        page.Children.Add(SettingsView.Section(Loc.T("保险库", "Vault")));
        if (sync.Join is { } join)
        {
            JoinWaiting(join);
        }
        else if (recoverMode)
        {
            var phrase = Input("phrase", Loc.T("24 个词，用空格分隔", "24 words separated by spaces"), mono: true, multiline: true);
            page.Children.Add(Box(
                Text(Loc.T("输入创建保险库时保存的 24 个词", "Enter the 24 words you saved when creating the vault"), bold: true),
                phrase,
                Spread(Btn("back", Loc.T("返回", "Back"), () => { recoverMode = false; Render(true); }),
                    Btn("recover", Loc.T("恢复", "Recover"), () =>
                    {
                        if (phrase.Text.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries).Length < 12) return;
                        sync.Recover(phrase.Text.Trim());
                    }, accent: true, enabled: !Busy))));
        }
        else if (sync.RemoteVault == false)
        {
            page.Children.Add(Box(
                Text(Loc.T("这是你的第一台设备", "This is your first device"), bold: true),
                Text(Loc.T("创建保险库后会显示一份恢复短语，请抄写保存。它是所有设备都丢失时找回数据的唯一方式。", "Creating a vault shows a recovery phrase. Write it down: it's the only way back in if you lose every device."), secondary: true, size: 13),
                Row(Btn("create", Loc.T("创建保险库", "Create vault"), sync.CreateVault, accent: true, enabled: !Busy))));
        }
        else if (sync.RemoteVault == true)
        {
            page.Children.Add(Box(
                Text(Loc.T("把这台电脑加入保险库", "Add this PC to your vault"), bold: true),
                Text(Loc.T("在一台已经同步的设备上打开 Qantrip 设置 › 同步，输入这里显示的校验码。", "On a device that already syncs, open Qantrip Settings › Sync and enter the code shown here."), secondary: true, size: 13),
                Row(Btn("join", Loc.T("开始加入", "Start"), sync.StartJoin, accent: true, enabled: !Busy),
                    Btn("userecovery", Loc.T("改用恢复短语", "Use recovery phrase"), () => { recoverMode = true; Render(true); }))));
        }
        else
        {
            page.Children.Add(Box(Row(new ProgressRing { IsActive = true, Width = 18, Height = 18 }, Text(Loc.T("正在查询保险库…", "Checking for a vault…"), secondary: true))));
        }
    }

    private void JoinWaiting(JoinProgress join)
    {
        var items = new List<UIElement>();
        switch (join.Stage)
        {
            case "waiting_for_approver":
                items.Add(Row(new ProgressRing { IsActive = true, Width = 18, Height = 18 }, Text(Loc.T("等待已同步的设备响应…", "Waiting for a device that already syncs…"))));
                items.Add(Text(Loc.T("在那台设备上打开 Qantrip 设置 › 同步。", "Open Qantrip Settings › Sync on that device."), secondary: true, size: 13));
                break;
            case "enter_code_on_other_device":
                items.Add(Text(Loc.T("在另一台设备上输入这个校验码", "Enter this code on your other device"), bold: true));
                items.Add(Big(join.Sas ?? "", "sas"));
                items.Add(Text(Loc.T("这台电脑上不需要任何确认。", "Nothing to confirm on this PC."), secondary: true, size: 13));
                break;
            case "enter_code_here":
                var code = Input("confirm", Loc.T("确认码", "Code"), mono: true, width: 220);
                code.FontSize = 18;
                items.Add(Text(Loc.T("输入另一台设备上显示的确认码", "Enter the code shown on your other device"), bold: true));
                items.Add(Row(code, Btn("confirmjoin", Loc.T("加入", "Join"), () => { if (code.Text.Trim().Length >= 4) sync.ConfirmJoin(code.Text); }, accent: true, enabled: !Busy)));
                items.Add(Text(Loc.T("两边互相输入校验码，确保这台电脑加入的是你自己的保险库。", "Entering a code on each side makes sure this PC joins your own vault."), secondary: true, size: 12));
                break;
            case "completed":
                items.Add(Text(Loc.T("已加入", "Joined"), bold: true));
                break;
            case "expired":
                items.Add(Text(Loc.T("请求已过期", "The request expired"), bold: true));
                break;
        }
        items.Add(Row(Btn("canceljoin", join.Stage == "expired" ? Loc.T("关闭", "Close") : Loc.T("取消", "Cancel"), sync.CancelJoin)));
        page.Children.Add(Box(items.ToArray()));
    }

    private TextBlock? relative;

    private void Enrolled()
    {
        var status = sync.Status!;
        if (status.WritesBlocked)
        {
            var bar = new InfoBar
            {
                IsOpen = true, IsClosable = false, Severity = InfoBarSeverity.Error, Margin = new Thickness(0, 12, 0, 0),
                Title = Loc.T("已暂停上传", "Uploads paused"),
                Message = Loc.T("检测到异常，已暂停上传。本地模板不受影响。", "Something looked wrong, so uploads are paused. Your local templates are fine."),
                ActionButton = Btn("reverify", Loc.T("重新验证", "Check again"), sync.Reverify, enabled: !Busy),
            };
            AutomationProperties.SetAutomationId(bar, "sync_blocked");
            page.Children.Add(bar);
        }

        page.Children.Add(SettingsView.Section(Loc.T("账号", "Account")));
        relative = Text("", bold: true);
        AutomationProperties.SetAutomationId(relative, "sync_lastsync");
        UpdateRelative();
        var info = new StackPanel { Spacing = 6 };
        info.Children.Add(Row(Text(Loc.T("账号", "Account"), secondary: true), Text(status.Account ?? "", bold: true)));
        info.Children.Add(Row(Text(Loc.T("上次同步", "Last synced"), secondary: true), relative));
        var card = new List<UIElement> { Spread(info, Row(Btn("syncnow", Loc.T("立即同步", "Sync now"), sync.SyncNow, accent: true, enabled: !Busy), Btn("signout", Loc.T("退出登录", "Sign out"), sync.SignOut, enabled: !Busy))) };
        if (status.Usage is { } usage) card.Add(UsageRow(usage));
        if (status.NextPushAt is long at)
        {
            var time = DateTimeOffset.FromUnixTimeSeconds(at).ToLocalTime().ToString("HH:mm");
            var t = Text(Loc.T($"本地修改将在 {time} 自动上传；点「立即同步」可马上上传。", $"Local changes upload automatically at {time}. Click Sync now to upload right away."), secondary: true, size: 12);
            AutomationProperties.SetAutomationId(t, "sync_nextpush");
            card.Add(t);
        }
        if (sync.LastReport is { Conflicts: > 0 } report)
        {
            var t = Text(Loc.T($"有 {report.Conflicts} 个模板在两台设备上同时修改，已保存为冲突副本，请在模板库中合并。", $"{report.Conflicts} template(s) changed on two devices at once. Both versions were kept; merge them in the template library."), size: 12);
            t.Foreground = new SolidColorBrush(Microsoft.UI.Colors.DarkOrange);
            card.Add(t);
        }
        page.Children.Add(Box(card.ToArray()));

        if (status.Alerts.Count > 0)
        {
            page.Children.Add(SettingsView.Section(Loc.T("安全提醒", "Security alerts")));
            var items = status.Alerts.Select(a => (UIElement)Text(Loc.En ? a.Code.Replace('_', ' ') : a.Message, size: 13)).ToList();
            items.Add(Row(Btn("dismissalerts", Loc.T("知道了", "Dismiss"), sync.DismissAlerts)));
            page.Children.Add(Box(items.ToArray()));
        }

        foreach (var (id, confirm) in sync.ConfirmCodes.OrderBy(kv => kv.Key))
        {
            page.Children.Add(SettingsView.Section(Loc.T("最后一步", "Last step")));
            page.Children.Add(Box(
                Text(Loc.T("在新的设备上输入这个确认码", "Enter this code on the new device")),
                Big(confirm, "confirmcode"),
                Row(Btn("done_" + id, Loc.T("完成", "Done"), () => sync.DismissConfirmCode(id)))));
        }

        var requests = sync.JoinRequests.Where(r => r.Status != "handled_elsewhere" && !sync.ConfirmCodes.ContainsKey(r.JoinId)).ToList();
        if (requests.Count > 0)
        {
            page.Children.Add(SettingsView.Section(Loc.T("新设备请求加入", "A new device wants to join")));
            foreach (var r in requests)
            {
                if (r.Status == "waiting_for_new_device")
                {
                    page.Children.Add(Box(Row(new ProgressRing { IsActive = true, Width = 18, Height = 18 }, Text(Loc.T("等待新设备显示校验码…", "Waiting for the new device to show its code…")))));
                    continue;
                }
                var code = Input("sas_" + r.JoinId, Loc.T("校验码", "Code"), mono: true, width: 200);
                var id = r.JoinId;
                page.Children.Add(Box(
                    Text(Loc.T("输入新设备上显示的校验码。如果不是你本人在操作，请拒绝。", "Enter the code shown on the new device. If this isn't you, reject it."), size: 13),
                    Row(code,
                        Btn("approve", Loc.T("批准", "Approve"), () => { if (code.Text.Trim().Length >= 4) sync.Approve(id, code.Text); }, accent: true, enabled: !Busy),
                        Btn("reject", Loc.T("拒绝", "Reject"), () => sync.Reject(id)))));
            }
        }

        page.Children.Add(SettingsView.Section(Loc.T("设备", "Devices")));
        var devices = new List<UIElement>();
        if (sync.AccountDevices is { } account)
        {
            foreach (var d in account.Devices) devices.Add(AccountDeviceRow(d));
            // 已退出登录、但仍在保险库中的设备：建议从保险库移除并轮换密钥
            foreach (var d in sync.Devices.Where(r => !r.IsSelf && !account.Devices.Any(a => a.DeviceId == r.DeviceId)))
            {
                var device = d;
                var left = new StackPanel { Spacing = 2 };
                left.Children.Add(Text(Loc.T("已退出登录的设备", "Signed-out device")));
                left.Children.Add(Text(Loc.T("仍在保险库中。建议移除并轮换密钥。", "Still in the vault. Removing it rotates the keys."), secondary: true, size: 12));
                devices.Add(Spread(Row(Icon(""), left),
                    Btn("revoke_" + Short(d.DeviceId), Loc.T("从保险库移除", "Remove from vault"), () => _ = ConfirmRevoke(device), enabled: !Busy)));
            }
            devices.Add(Text(Loc.T($"这个账号最多同时登录 {account.MaxDevices} 台设备。要添加设备，在新设备上登录同一个账号并按提示操作。",
                $"This account can be signed in on up to {account.MaxDevices} devices. To add one, sign in with the same account on it and follow the steps."), secondary: true, size: 12));
        }
        else devices.Add(Row(new ProgressRing { IsActive = true, Width = 18, Height = 18 }, Text(Loc.T("正在读取设备…", "Loading devices…"), secondary: true)));
        var devicesBox = Box(devices.ToArray());
        AutomationProperties.SetAutomationId(devicesBox, "sync_devices");
        page.Children.Add(devicesBox);

        page.Children.Add(SettingsView.Section(Loc.T("密钥与本机", "Keys and this PC")));
        page.Children.Add(Box(
            Spread(Text(Loc.T("轮换密钥后，之前移除的设备即使保留了旧密钥，也无法读取之后的内容。", "After rotating keys, previously removed devices can't read anything new even if they kept old keys."), secondary: true, size: 13),
                Btn("rotate", Loc.T("轮换密钥", "Rotate keys"), sync.RotateKeys, enabled: !Busy)),
            Spread(Text(Loc.T("重置只删除这台电脑的同步身份，不影响模板文件，也不影响其他设备。", "Resetting only removes this PC's sync identity. Your template files and other devices are not affected."), secondary: true, size: 13),
                Btn("reset", Loc.T("重置这台设备的同步", "Reset sync on this PC"), () => _ = ConfirmReset(), enabled: !Busy))));
    }

    // MARK: 用量与账号设备

    private static string Short(string id) => id.Length > 8 ? id[..8] : id;

    private static FontIcon Icon(string glyph) => new() { Glyph = glyph, FontFamily = Theme.Icons, FontSize = 16, VerticalAlignment = VerticalAlignment.Center };

    /// 云端用量：已用 / 配额、进度条；用满时提示新的修改不会上传
    private static UIElement UsageRow(SyncUsage usage)
    {
        double ratio = Math.Min((double)usage.Bytes / Math.Max(usage.QuotaBytes, 1UL), 1);
        var sp = new StackPanel { Spacing = 4 };
        var amount = Text($"{FormatBytes(usage.Bytes)} / {FormatBytes(usage.QuotaBytes)}", secondary: true, size: 13);
        AutomationProperties.SetAutomationId(amount, "sync_usage");
        sp.Children.Add(Spread(Text(Loc.T("云端用量", "Cloud storage"), secondary: true), amount));
        var bar = new ProgressBar { Minimum = 0, Maximum = 1, Value = ratio };
        if (ratio > 0.9) bar.Foreground = new SolidColorBrush(Microsoft.UI.Colors.Crimson);
        else if (ratio > 0.75) bar.Foreground = new SolidColorBrush(Microsoft.UI.Colors.DarkOrange);
        AutomationProperties.SetName(bar, Loc.T("云端用量", "Cloud storage"));
        sp.Children.Add(bar);
        if (ratio >= 1)
        {
            var full = Text(Loc.T("云端空间已满：新的修改不会上传，本地模板不受影响。可以删除不用的模板腾出空间。",
                "Cloud storage is full: new changes won't upload; local templates are unaffected. Delete unused templates to free space."), size: 12);
            full.Foreground = new SolidColorBrush(Microsoft.UI.Colors.Crimson);
            sp.Children.Add(full);
        }
        return sp;
    }

    /// 字节数（十进制单位，与 macOS ByteCountFormatter 的 .file 一致）
    internal static string FormatBytes(ulong n)
    {
        if (n < 1000) return Loc.T($"{n} 字节", n == 1 ? "1 byte" : $"{n} bytes");
        string[] units = { "KB", "MB", "GB", "TB" };
        double v = n;
        int i = -1;
        do { v /= 1000; i++; } while (v >= 1000 && i < units.Length - 1);
        return $"{(v >= 100 ? v.ToString("0") : v.ToString("0.#"))} {units[i]}";
    }

    /// 相对时间（按界面语言）
    private static string Relative(long unix)
    {
        var d = DateTimeOffset.UtcNow - DateTimeOffset.FromUnixTimeSeconds(unix);
        if (d.TotalSeconds < 60) return Loc.T("刚刚", "just now");
        int m = (int)d.TotalMinutes, h = (int)d.TotalHours, days = (int)d.TotalDays;
        if (m < 60) return Loc.T($"{m} 分钟前", m == 1 ? "1 minute ago" : $"{m} minutes ago");
        if (h < 24) return Loc.T($"{h} 小时前", h == 1 ? "1 hour ago" : $"{h} hours ago");
        if (days < 30) return Loc.T($"{days} 天前", days == 1 ? "yesterday" : $"{days} days ago");
        return DateTimeOffset.FromUnixTimeSeconds(unix).ToLocalTime().ToString("yyyy-MM-dd");
    }

    private static Border Tag(string text, Windows.UI.Color color)
    {
        var t = new TextBlock { Text = text, FontSize = 10, FontWeight = FontWeights.SemiBold };
        return new Border
        {
            Child = t, Padding = new Thickness(6, 1, 6, 2), CornerRadius = new CornerRadius(8), VerticalAlignment = VerticalAlignment.Center,
            Background = new SolidColorBrush(color) { Opacity = 0.22 },
        };
    }

    private static Windows.UI.Color Accent =>
        Application.Current.Resources.TryGetValue("SystemAccentColor", out var c) && c is Windows.UI.Color color ? color : Microsoft.UI.Colors.SteelBlue;

    /// 账号下的一台设备：名称、标签、系统 · Qantrip 版本 · 上次在线；右侧"解除"（本机为"退出登录"）
    private UIElement AccountDeviceRow(AccountDevice d)
    {
        var title = Row(Text(d.Name.Length == 0 ? Loc.T("未命名设备", "Unnamed device") : d.Name));
        if (d.IsSelf) title.Children.Add(Tag(Loc.T("本机", "This device"), Accent));
        if (d.Limited) title.Children.Add(Tag(Loc.T("超出上限", "Over limit"), Microsoft.UI.Colors.DarkOrange));
        // 本机没有保险库状态时无法判断其他设备是否在保险库中
        else if (!d.InVault && sync.Enrolled) title.Children.Add(Tag(Loc.T("未加入保险库", "Not in vault"), Microsoft.UI.Colors.Gray));
        var parts = new List<string>();
        if (d.Platform.Length > 0) parts.Add(d.Platform);
        if (d.AppVersion.Length > 0) parts.Add("Qantrip " + d.AppVersion);
        parts.Add(Loc.T("上次在线 ", "Last seen ") + Relative(d.LastSeenAt));
        var left = new StackPanel { Spacing = 2 };
        left.Children.Add(title);
        left.Children.Add(Text(string.Join(" · ", parts), secondary: true, size: 12));
        var icon = Icon(d.Platform.StartsWith("windows", StringComparison.OrdinalIgnoreCase) ? "" : "");
        var device = d;
        var row = Spread(Row(icon, left), Btn("release_" + d.GrantId, d.IsSelf ? Loc.T("退出登录", "Sign out") : Loc.T("解除", "Release"), () => _ = ConfirmRelease(device), enabled: !Busy));
        AutomationProperties.SetAutomationId(row, "sync_device_" + d.GrantId);
        return row;
    }

    /// 本机超出设备上限：列出账号下的其他设备，解除一台后本机即可同步；旧设备丢失时也可以在这里直接解除。
    private void DeviceLimit(uint max)
    {
        page.Children.Add(SettingsView.Section(Loc.T("设备数量", "Devices")));
        var items = new List<UIElement>();
        var head = Row(new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 16, Foreground = new SolidColorBrush(Microsoft.UI.Colors.DarkOrange) },
            Text(Loc.T($"这个账号最多同时在 {max} 台设备上同步", $"This account can sync on up to {max} devices at a time"), bold: true));
        AutomationProperties.SetAutomationId(head, "sync_devicelimit");
        items.Add(head);
        items.Add(Text(Loc.T("解除下面任意一台设备后，这台设备就可以同步。旧设备丢失或无法再使用时，也可以在这里直接解除它。",
            "Release any device below and this one can sync. If an old device is lost or unusable, you can release it right here."), secondary: true, size: 13));
        if (sync.AccountDevices is { } account)
            foreach (var d in account.Devices.Where(d => !d.IsSelf)) items.Add(AccountDeviceRow(d));
        else items.Add(Row(new ProgressRing { IsActive = true, Width = 18, Height = 18 }, Text(Loc.T("正在读取设备…", "Loading devices…"), secondary: true)));
        items.Add(Row(Btn("signout", Loc.T("退出登录", "Sign out"), sync.SignOut, enabled: !Busy)));
        page.Children.Add(Box(items.ToArray()));
    }

    /// 只更新"x 分钟前"，不重建页面
    private void UpdateRelative()
    {
        if (relative is null) return;
        relative.Text = sync.Status?.LastSync is long t ? Ago(t) : Loc.T("尚未同步", "Not yet");
    }

    private static string Ago(long unix)
    {
        var d = DateTimeOffset.UtcNow - DateTimeOffset.FromUnixTimeSeconds(unix);
        if (d.TotalSeconds < 60) return Loc.T("刚刚", "just now");
        if (d.TotalMinutes < 60) return Loc.T($"{(int)d.TotalMinutes} 分钟前", $"{(int)d.TotalMinutes} min ago");
        if (d.TotalHours < 24) return Loc.T($"{(int)d.TotalHours} 小时前", $"{(int)d.TotalHours} h ago");
        return DateTimeOffset.FromUnixTimeSeconds(unix).ToLocalTime().ToString("yyyy-MM-dd HH:mm");
    }

    private void Advanced()
    {
        if (sync.SignedIn || !texts.ContainsKey("server")) texts["server"] = Settings.SyncServer;
        var server = Input("server", Settings.DefaultSyncServer, mono: true, width: 320);
        server.IsEnabled = !sync.SignedIn;
        void Apply()
        {
            if (sync.SignedIn) return;
            var v = server.Text.Trim();
            if (v.Length > 0 && !(Uri.TryCreate(v, UriKind.Absolute, out var u) && u.Scheme is "https" or "http")) { server.Text = Settings.SyncServer; return; }
            if (v.TrimEnd('/') == Settings.SyncServer) return;
            Settings.SyncServer = v;
            server.Text = Settings.SyncServer;
            sync.Start();
        }
        server.LostFocus += (_, _) => Apply();
        server.KeyDown += (_, e) => { if (e.Key == Windows.System.VirtualKey.Enter) Apply(); };
        var body = new StackPanel { Spacing = 10 };
        body.Children.Add(Spread(Text(Loc.T("服务器", "Server")), server));
        if (sync.Status?.DeviceId is { } id)
            body.Children.Add(Spread(Text(Loc.T("本机标识", "This PC's ID")), Text(id, secondary: true, size: 12, mono: true)));
        body.Children.Add(Text(Loc.T("可以把服务端部署到自己的 Cloudflare 账号，在这里填写地址。登录后不能修改。", "You can deploy the server to your own Cloudflare account and enter its address here. Locked while signed in."), secondary: true, size: 12));
        var exp = new Expander
        {
            Header = Loc.T("高级", "Advanced"), Content = body, HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Stretch,
            IsExpanded = advancedOpen, Margin = new Thickness(0, 18, 0, 0),
        };
        exp.Expanding += (_, _) => advancedOpen = true;
        exp.Collapsed += (_, _) => advancedOpen = false;
        AutomationProperties.SetAutomationId(exp, "sync_advanced");
        page.Children.Add(exp);
    }

    private bool advancedOpen = Environment.GetEnvironmentVariable("QANTRIP_DEBUG_SYNC_ADVANCED") == "1";

    // MARK: 对话框

    /// 显示对话框；已有对话框或页面尚未显示时不显示，返回 null。
    private async Task<ContentDialogResult?> Dialog(ContentDialog d)
    {
        if (dialogOpen || host.XamlRoot is null) return null;
        dialogOpen = true;
        try
        {
            d.XamlRoot = host.XamlRoot;
            d.RequestedTheme = (host as FrameworkElement)?.ActualTheme ?? ElementTheme.Default;
            return await d.ShowAsync();
        }
        finally { dialogOpen = false; }
    }

    private async Task ConfirmRelease(AccountDevice d)
    {
        bool limit = sync.Status?.DeviceLimit != null;
        var r = await Dialog(new ContentDialog
        {
            Title = d.IsSelf ? Loc.T("退出这台设备的登录？", "Sign out on this device?") : Loc.T("解除这台设备？", "Release this device?"),
            Content = d.IsSelf
                ? Loc.T("这台设备会退出登录，不再上传或下载。本机的模板文件不受影响。", "This device will be signed out and stop uploading or downloading. Your template files stay on this PC.")
                : limit
                    ? Loc.T("它会退出登录，无法再上传或下载。", "It will be signed out and can no longer upload or download.")
                    : Loc.T("它会退出登录，无法再上传或下载。如果它还在保险库中，之后可以在这里把它移除并轮换密钥。",
                        "It will be signed out and can no longer upload or download. If it is still in the vault, you can then remove it here, which rotates the keys."),
            PrimaryButtonText = d.IsSelf ? Loc.T("退出登录", "Sign out") : Loc.T("解除", "Release"),
            CloseButtonText = Loc.T("取消", "Cancel"),
            DefaultButton = ContentDialogButton.Close,
        });
        if (r == ContentDialogResult.Primary) sync.Release(d);
    }

    private async Task ConfirmRevoke(DeviceInfo d)
    {
        var r = await Dialog(new ContentDialog
        {
            Title = Loc.T("移除这台设备？", "Remove this device?"),
            Content = Loc.T("被移除的设备将无法读取之后的任何同步内容。", "The removed device won't be able to read anything synced afterwards."),
            PrimaryButtonText = Loc.T("移除并轮换密钥", "Remove and rotate keys"),
            CloseButtonText = Loc.T("取消", "Cancel"),
            DefaultButton = ContentDialogButton.Close,
        });
        if (r != ContentDialogResult.Primary) return;
        sync.Revoke(d.DeviceId, null, () => _ = AskPhraseForRevoke(d));
    }

    private async Task AskPhraseForRevoke(DeviceInfo d)
    {
        var box = new TextBox { AcceptsReturn = true, TextWrapping = TextWrapping.Wrap, Height = 96, FontFamily = Theme.Mono, IsSpellCheckEnabled = false };
        AutomationProperties.SetAutomationId(box, "sync_revokephrase");
        var content = new StackPanel { Spacing = 10 };
        content.Children.Add(Text(Loc.T("这是除本机外的最后一台设备。移除它需要输入恢复短语。", "This is the last other device. Removing it needs your recovery phrase."), secondary: true, size: 13));
        content.Children.Add(box);
        var r = await Dialog(new ContentDialog
        {
            Title = Loc.T("需要恢复短语", "Recovery phrase needed"),
            Content = content,
            PrimaryButtonText = Loc.T("移除", "Remove"),
            CloseButtonText = Loc.T("取消", "Cancel"),
        });
        if (r == ContentDialogResult.Primary && box.Text.Trim().Length > 0) sync.Revoke(d.DeviceId, box.Text.Trim(), () => { });
    }

    private async Task ConfirmReset()
    {
        var r = await Dialog(new ContentDialog
        {
            Title = Loc.T("重置这台设备的同步？", "Reset sync on this PC?"),
            Content = Loc.T("之后需要重新加入保险库。", "You'll need to join the vault again afterwards."),
            PrimaryButtonText = Loc.T("重置", "Reset"),
            CloseButtonText = Loc.T("取消", "Cancel"),
            DefaultButton = ContentDialogButton.Close,
        });
        if (r == ContentDialogResult.Primary) sync.ResetDevice();
    }

    /// 恢复短语：只显示一次，确认已抄写后才能关闭。
    private async Task ShowPhrase(string phrase)
    {
        var words = phrase.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        var grid = new Grid { ColumnSpacing = 14, RowSpacing = 8, Padding = new Thickness(12) };
        for (int c = 0; c < 4; c++) grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        for (int r = 0; r < (words.Length + 3) / 4; r++) grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        for (int i = 0; i < words.Length; i++)
        {
            var cell = Row(Text($"{i + 1}", secondary: true, size: 11), Text(words[i], mono: true));
            cell.Spacing = 6;
            Grid.SetRow(cell, i / 4);
            Grid.SetColumn(cell, i % 4);
            grid.Children.Add(cell);
        }
        AutomationProperties.SetAutomationId(grid, "sync_phrasegrid");
        var check = new CheckBox { Content = Loc.T("我已经保存好了", "I've saved it") };
        AutomationProperties.SetAutomationId(check, "sync_phrasesaved");
        var content = new StackPanel { Spacing = 14, MinWidth = 480 };
        content.Children.Add(Text(Loc.T("把它保存在安全的地方。所有设备都丢失时，只有它能找回你的同步数据；任何人（包括我们）都无法替你恢复。", "Keep it somewhere safe. If you lose every device, it's the only way to get your synced data back. Nobody, including us, can recover it for you."), secondary: true, size: 13));
        content.Children.Add(new Border { Child = grid, CornerRadius = new CornerRadius(8), Background = SettingsView.Res("SubtleFillColorSecondaryBrush") });
        // 复制（普通复制，留在剪贴板中，不加排除标记，也不定时清除）与保存为文件
        var copyText = new TextBlock { Text = Loc.T("复制", "Copy") };
        var copyIcon = new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 14 };
        var copy = new Button { Content = Row(copyIcon, copyText) };
        AutomationProperties.SetAutomationId(copy, "sync_phrasecopy");
        AutomationProperties.SetName(copy, Loc.T("复制", "Copy"));
        copy.Click += (_, _) =>
        {
            if (Platform.ClipboardStore.SetText(hwnd, phrase, exclude: false) == 0) return;
            copyIcon.Glyph = "";
            copyText.Text = Loc.T("已复制", "Copied");
        };
        var save = new Button { Content = Row(new FontIcon { Glyph = "", FontFamily = Theme.Icons, FontSize = 14 }, new TextBlock { Text = Loc.T("保存为文件…", "Save to File…") }) };
        AutomationProperties.SetAutomationId(save, "sync_phrasesave");
        AutomationProperties.SetName(save, Loc.T("保存为文件…", "Save to File…"));
        save.Click += (_, _) => _ = SavePhrase(phrase);
        content.Children.Add(Row(copy, save));
        content.Children.Add(Text(Loc.T("请把它保存在只有你能访问的地方，例如密码管理器；不要发给任何人。", "Keep it somewhere only you can access, such as a password manager. Never share it."), secondary: true, size: 12));
        content.Children.Add(check);
        var dialog = new ContentDialog
        {
            Title = Loc.T("你的恢复短语", "Your recovery phrase"),
            Content = content,
            PrimaryButtonText = Loc.T("完成", "Done"),
            IsPrimaryButtonEnabled = false,
            DefaultButton = ContentDialogButton.Primary,
        };
        check.Checked += (_, _) => dialog.IsPrimaryButtonEnabled = true;
        check.Unchecked += (_, _) => dialog.IsPrimaryButtonEnabled = false;
        // 未确认已抄写时不允许关闭（包括 Esc）
        dialog.Closing += (_, e) => { if (check.IsChecked != true) e.Cancel = true; };
        if (await Dialog(dialog) is null) return;
        if (check.IsChecked == true) sync.Phrase = null;
        Render(true);
    }

    /// 保存恢复短语为文本文件：简短说明 + 编号的词（与 macOS 相同）。
    private async Task SavePhrase(string phrase)
    {
        try
        {
            var picker = new Windows.Storage.Pickers.FileSavePicker
            {
                SuggestedFileName = Loc.T("Qantrip 恢复短语", "Qantrip recovery phrase"),
                SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.DocumentsLibrary,
            };
            picker.FileTypeChoices.Add(Loc.T("文本文件", "Text file"), new List<string> { ".txt" });
            WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);
            var file = await picker.PickSaveFileAsync();
            if (file is null) return;
            var words = phrase.Split(' ', StringSplitOptions.RemoveEmptyEntries).Select((w, i) => $"{i + 1}. {w}");
            var text = Loc.T("Nekro Qantrip 同步恢复短语\r\n所有设备都丢失时，用它找回同步数据。请妥善保管，不要发给任何人。\r\n\r\n",
                "Nekro Qantrip sync recovery phrase\r\nUse it to recover your synced data if you lose every device. Keep it safe and never share it.\r\n\r\n")
                + string.Join("\r\n", words) + "\r\n";
            // 带 BOM 的 UTF-8：旧版记事本也能正确显示中文
            await File.WriteAllTextAsync(file.Path, text, new UTF8Encoding(true));
            Log.Event("sync_phrase_saved", new());
        }
        catch (Exception e) { Log.Error("save_phrase", e); }
    }
}

/// DispatcherQueue 上的重复定时器（页面关闭时停止）。
internal sealed class DispatcherTimerLike
{
    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer timer;

    public DispatcherTimerLike(Microsoft.UI.Dispatching.DispatcherQueue queue, TimeSpan interval, Action tick)
    {
        timer = queue.CreateTimer();
        timer.Interval = interval;
        timer.Tick += (_, _) => tick();
        timer.Start();
    }

    public void Stop() => timer.Stop();
}
