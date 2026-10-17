using System.Net;
using System.Net.Sockets;
using System.Text;

namespace Qantrip.Platform;

/// <summary>
/// 浏览器登录的本机回调（PKCE，RFC 8252 §7.3）：在 127.0.0.1 的随机端口上接收一次 `/callback?code=…&state=…`，
/// 返回一个"可以回到 Qantrip"的页面。用 TcpListener 而不是 HttpListener：后者监听 127.0.0.1 需要 URL ACL（管理员）。
/// 对应 macOS SyncService.swift 的 LoopbackReceiver。
/// </summary>
internal sealed class LoopbackReceiver
{
    private TcpListener? listener;
    private CancellationTokenSource? cts;

    public sealed record Result(string? Code, string? State, string? Error);

    /// 开始监听，返回回调地址。收到回调（或取消 / 出错）后在调用线程的同步上下文中调用 completion 一次。
    public string Start(Action<Result> completion)
    {
        Stop();
        var l = new TcpListener(IPAddress.Loopback, 0);
        l.Start();
        listener = l;
        var c = cts = new CancellationTokenSource();
        int port = ((IPEndPoint)l.LocalEndpoint).Port;
        var context = SynchronizationContext.Current;
        void Done(Result r)
        {
            if (context is null) completion(r); else context.Post(_ => completion(r), null);
        }
        _ = Task.Run(async () =>
        {
            try
            {
                // 浏览器可能先请求 favicon 等无关地址：一直处理到收到 /callback 为止
                while (!c.IsCancellationRequested)
                {
                    using var client = await l.AcceptTcpClientAsync(c.Token);
                    var r = await Handle(client, c.Token);
                    if (r != null) { Done(r); break; }
                }
            }
            catch (OperationCanceledException) { }
            catch (ObjectDisposedException) { }
            catch (Exception e)
            {
                Log.Error("loopback", e);
                if (!c.IsCancellationRequested) Done(new Result(null, null, e.Message));
            }
            finally { try { l.Stop(); } catch { } }
        });
        return $"http://127.0.0.1:{port}/callback";
    }

    public void Stop()
    {
        cts?.Cancel();
        try { listener?.Stop(); } catch { }
        listener = null;
        cts = null;
    }

    private static async Task<Result?> Handle(TcpClient client, CancellationToken ct)
    {
        client.ReceiveTimeout = 5000;
        var stream = client.GetStream();
        var buf = new byte[16 * 1024];
        int n = 0;
        // 读到请求头结束（只需要请求行）
        while (n < buf.Length)
        {
            int k = await stream.ReadAsync(buf.AsMemory(n), ct);
            if (k <= 0) break;
            n += k;
            if (Encoding.ASCII.GetString(buf, 0, n).Contains("\r\n\r\n")) break;
        }
        var request = Encoding.UTF8.GetString(buf, 0, n);
        var line = request.Split("\r\n", 2)[0];
        var parts = line.Split(' ');
        var target = parts.Length >= 2 ? parts[1] : "";
        var query = Query(target);
        bool isCallback = target.StartsWith("/callback", StringComparison.Ordinal);
        query.TryGetValue("code", out var code);
        query.TryGetValue("state", out var state);
        query.TryGetValue("error", out var error);
        bool ok = isCallback && code != null && state != null;
        bool denied = isCallback && error != null;

        string body;
        int status = 200;
        if (!isCallback) { status = 404; body = ""; }
        else
        {
            var page = ok ? Loc.T("登录完成，可以关闭此页面并回到 Qantrip。", "Signed in. You can close this tab and return to Qantrip.")
                : Loc.T("登录没有完成，请回到 Qantrip 重试。", "Sign-in didn't complete. Return to Qantrip and try again.");
            body = "<!doctype html><meta charset=utf-8><title>Qantrip</title><body style=\"font:16px 'Segoe UI',sans-serif;background:#0b0b0c;color:#ececee;display:grid;place-items:center;height:90vh\"><p>"
                + WebUtility.HtmlEncode(page) + "</p>";
        }
        var bytes = Encoding.UTF8.GetBytes(body);
        var head = $"HTTP/1.1 {status} {(status == 200 ? "OK" : "Not Found")}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {bytes.Length}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";
        await stream.WriteAsync(Encoding.ASCII.GetBytes(head), ct);
        await stream.WriteAsync(bytes, ct);
        await stream.FlushAsync(ct);
        if (ok) return new Result(code, state, null);
        if (denied) return new Result(null, null, error);
        return null;
    }

    private static Dictionary<string, string> Query(string target)
    {
        var d = new Dictionary<string, string>();
        int q = target.IndexOf('?');
        if (q < 0) return d;
        foreach (var kv in target[(q + 1)..].Split('&', StringSplitOptions.RemoveEmptyEntries))
        {
            int eq = kv.IndexOf('=');
            var k = Uri.UnescapeDataString((eq < 0 ? kv : kv[..eq]).Replace('+', ' '));
            var v = eq < 0 ? "" : Uri.UnescapeDataString(kv[(eq + 1)..].Replace('+', ' '));
            d.TryAdd(k, v);
        }
        return d;
    }
}
