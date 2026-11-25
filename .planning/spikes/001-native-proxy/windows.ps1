param([switch]$Relay)
$ErrorActionPreference = 'Stop'

# Fixture only. Both listeners bind to loopback and are stopped in finally.
$source = @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Net.WebSockets;
using System.Security.Cryptography;
using System.Text;
using System.Threading;

public static class NativeProxySpike {
    public static volatile string Mode = "pass";
    public static int ApiPort, ProxyPort, Hits;
    static TcpListener api, proxy;

    public static void Start(bool relay) {
        api = new TcpListener(IPAddress.Loopback, relay ? 0 : 8080);
        proxy = new TcpListener(IPAddress.Loopback, relay ? 8080 : 0);
        api.Start();
        try { proxy.Start(); } catch { api.Stop(); throw; }
        ApiPort = ((IPEndPoint)api.LocalEndpoint).Port;
        ProxyPort = ((IPEndPoint)proxy.LocalEndpoint).Port;
        ThreadPool.QueueUserWorkItem(delegate { Serve(api, false); });
        ThreadPool.QueueUserWorkItem(delegate { Serve(proxy, true); });
    }
    public static void Stop() { api.Stop(); proxy.Stop(); }
    static void Send(Stream stream, int status, string body) {
        byte[] data = Encoding.ASCII.GetBytes("HTTP/1.1 " + status
            + " fixture\r\nConnection: close\r\nContent-Length: " + body.Length + "\r\n\r\n" + body);
        stream.Write(data, 0, data.Length);
    }
    static void Serve(TcpListener listener, bool forward) {
        while (true) {
            TcpClient client;
            try { client = listener.AcceptTcpClient(); } catch (SocketException) { return; }
            catch (InvalidOperationException) { return; }
            ThreadPool.QueueUserWorkItem(delegate {
                using (client) {
                    try {
                        client.ReceiveTimeout = 2500;
                        var stream = client.GetStream();
                        var reader = new StreamReader(stream);
                        string line = reader.ReadLine();
                        if (line == null) return;
                        var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
                        string field;
                        while (!string.IsNullOrEmpty(field = reader.ReadLine())) {
                            int colon = field.IndexOf(':');
                            if (colon > 0) headers[field.Substring(0, colon)] = field.Substring(colon + 1).Trim();
                        }
                        bool ws = line == "GET /socket HTTP/1.1" && headers.ContainsKey("Sec-WebSocket-Key");
                        if (!forward) {
                            if (!ws) { Send(stream, 200, "upstream"); return; }
                            string challenge = headers["Sec-WebSocket-Key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
                            string accept;
                            using (var sha = SHA1.Create()) accept = Convert.ToBase64String(sha.ComputeHash(Encoding.ASCII.GetBytes(challenge)));
                            byte[] handshake = Encoding.ASCII.GetBytes(
                                "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: "
                                + accept + "\r\n\r\n");
                            stream.Write(handshake, 0, handshake.Length);
                            byte[] frame = { 129, 7, 104, 101, 97, 108, 116, 104, 121 };
                            stream.Write(frame, 0, frame.Length);
                            stream.ReadByte();
                            return;
                        }
                        if (!ws && line != "GET http://127.0.0.1:8080/items HTTP/1.1"
                            && line != "GET /items HTTP/1.1") { Send(stream, 403, "fixture only"); return; }
                        Interlocked.Increment(ref Hits);
                        string mode = Mode;
                        if (mode == "503" || (ws && mode == "ws-reject")) { Send(stream, 503, "injected"); return; }
                        if (mode == "drop") return;
                        if (mode == "delay") Thread.Sleep(1400);
                        using (var upstream = new TcpClient("127.0.0.1", ApiPort)) {
                            var pipe = upstream.GetStream();
                            var request = new StringBuilder(ws ? "GET /socket HTTP/1.1\r\n" : "GET /items HTTP/1.1\r\n");
                            if (ws) foreach (var entry in headers) request.Append(entry.Key + ": " + entry.Value + "\r\n");
                            else request.Append("Host: 127.0.0.1:8080\r\nConnection: close\r\n");
                            byte[] query = Encoding.ASCII.GetBytes(request.Append("\r\n").ToString());
                            pipe.Write(query, 0, query.Length);
                            if (!ws) { pipe.CopyTo(stream); return; }
                            using (var output = new MemoryStream()) {
                                while (output.Length < 8192) {
                                    int next = pipe.ReadByte();
                                    if (next < 0) break;
                                    output.WriteByte((byte)next);
                                    if (next == 'y' && Encoding.ASCII.GetString(output.ToArray()).EndsWith("healthy")) break;
                                }
                                output.WriteTo(stream);
                            }
                            if (mode == "ws-drop") { Thread.Sleep(200); return; }
                            stream.ReadByte(); // The single-frame fixture waits for the test client to close.
                        }
                    } catch (IOException) {}
                }
            });
        }
    }
    public static string DotNet(bool proxied, bool control) {
        var req = (HttpWebRequest)WebRequest.Create("http://127.0.0.1:" + (control ? ApiPort : 8080) + "/items");
        req.Proxy = proxied ? new WebProxy("http://127.0.0.1:" + ProxyPort, false) : null;
        req.Timeout = 650;
        req.KeepAlive = false;
        try {
            using (var response = (HttpWebResponse)req.GetResponse())
                return ((int)response.StatusCode).ToString();
        } catch (WebException e) {
            var response = e.Response as HttpWebResponse;
            if (response == null) return e.Status.ToString();
            using (response) return ((int)response.StatusCode).ToString();
        }
    }
    public static string WebSocket(bool expectCut) {
        using (var ws = new ClientWebSocket())
        using (var deadline = new CancellationTokenSource(2200)) {
            var buffer = new ArraySegment<byte>(new byte[128]);
            bool healthy = false;
            try {
                ws.Options.Proxy = null;
                ws.ConnectAsync(new Uri("ws://127.0.0.1:8080/socket"), deadline.Token).GetAwaiter().GetResult();
                var message = ws.ReceiveAsync(buffer, deadline.Token).GetAwaiter().GetResult();
                healthy = Encoding.ASCII.GetString(buffer.Array, 0, message.Count) == "healthy";
                if (!healthy) return "unexpected-frame";
                if (!expectCut) return "healthy";
                ws.ReceiveAsync(buffer, deadline.Token).GetAwaiter().GetResult();
                return "unexpected-second-frame";
            } catch (Exception e) {
                if (deadline.IsCancellationRequested) return "watchdog";
                return (healthy ? "cut:" : "ERR:") + e.GetType().Name;
            }
        }
    }
}
'@

if (-not ('NativeProxySpike' -as [type])) { Add-Type -TypeDefinition $source }
function Get-WinHttpStatus {
    try {
        $req = New-Object -ComObject WinHttp.WinHttpRequest.5.1
        $req.Open('GET', 'http://127.0.0.1:8080/items', $false)
        $req.SetTimeouts(650, 650, 650, 650)
        if ($Relay) { $req.SetProxy(1) }
        else { $req.SetProxy(2, ('127.0.0.1:' + [NativeProxySpike]::ProxyPort), '') }
        $req.Send()
        return [string]$req.Status
    } catch {
        return ('ERR:' + $_.Exception.HResult)
    }
}

[NativeProxySpike]::Start([bool]$Relay)
try {
    $rows = foreach ($mode in @('pass', '503', 'delay', 'drop', 'pass')) {
        [NativeProxySpike]::Mode = $mode
        $start = [NativeProxySpike]::Hits
        $direct = [NativeProxySpike]::DotNet($false, $true)
        $dotNet = [NativeProxySpike]::DotNet(-not $Relay, $false)
        $dotNetHits = [NativeProxySpike]::Hits - $start
        $start = [NativeProxySpike]::Hits
        $winHttp = Get-WinHttpStatus
        [pscustomobject]@{
            mode = $mode
            direct = $direct
            dotNet = $dotNet
            dotNetHits = $dotNetHits
            winHttp = $winHttp
            winHttpHits = [NativeProxySpike]::Hits - $start
        }
    }
    $socketRows = if ($Relay) {
        foreach ($mode in @('pass', 'ws-reject', 'ws-drop', 'pass')) {
            [NativeProxySpike]::Mode = $mode
            $start = [NativeProxySpike]::Hits
            $answer = [NativeProxySpike]::WebSocket($mode -eq 'ws-drop')
            [pscustomobject]@{mode = $mode; answer = $answer; hits = [NativeProxySpike]::Hits - $start}
        }
    }
    Write-Host ('Windows spike - relay=' + [bool]$Relay + ' - fixed authority 127.0.0.1:8080')
    $rows | Format-Table -AutoSize
    if ($socketRows) { $socketRows | Format-Table -AutoSize }
    $report = @{relay = [bool]$Relay; http = @($rows); websocket = @($socketRows)}
    $file = 'device-bot-native-proxy-spike-' + $(if ($Relay) { 'relay' } else { 'forward' }) + '.json'
    $report | ConvertTo-Json -Compress | Set-Content -Encoding UTF8 (Join-Path $env:TEMP $file)
} finally {
    [NativeProxySpike]::Stop()
}
