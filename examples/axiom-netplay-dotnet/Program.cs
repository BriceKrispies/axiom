using System.Security.Cryptography;
using Microsoft.AspNetCore.Http.Features;
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.FileProviders;
using Axiom.Netplay;

// Transport mode: "websocket" (default) serves only WS; "webtransport" (or
// "both") ALSO stands up an HTTP/3 endpoint so the page can pick WebTransport via
// ?transport=webtransport.
string transportMode = (Environment.GetEnvironmentVariable("AXIOM_TRANSPORT") ?? "websocket").ToLowerInvariant();
bool webTransportRequested = transportMode is "webtransport" or "both";

// WebTransport requires HTTP/3 over QUIC, which needs OS support (Windows 11 /
// Server 2022+, or Linux with libmsquic). If unsupported we degrade to
// WebSocket-only rather than failing to start.
bool quicSupported = System.Net.Quic.QuicListener.IsSupported;
bool enableWebTransport = webTransportRequested && quicSupported;

// WebTransport needs HTTP/3 + a TLS cert; build a short-lived self-signed dev cert
// and publish its hash so the browser can trust it without a CA.
var (wtCert, wtCertHashBase64) = enableWebTransport
    ? WebTransportServer.CreateDevCertificate()
    : (null, null);
if (enableWebTransport)
    AppContext.SetSwitch("Microsoft.AspNetCore.Server.Kestrel.Experimental.WebTransportAndH3Datagrams", true);

var builder = WebApplication.CreateBuilder(args);

if (enableWebTransport)
{
    // Two endpoints: 8090 (HTTP/1.1 — static + WebSocket) and 8091 (HTTP/3 —
    // WebTransport). UseUrls can't coexist with explicit Kestrel listens.
    builder.WebHost.ConfigureKestrel(options =>
    {
        // Bind all interfaces so the endpoints are reachable through a container's
        // published ports (not just localhost inside the container).
        options.ListenAnyIP(8090);
        options.ListenAnyIP(8091, listen =>
        {
            listen.UseHttps(wtCert!);
            listen.Protocols = HttpProtocols.Http3;
        });
    });
}
else if (string.IsNullOrEmpty(Environment.GetEnvironmentVariable("ASPNETCORE_URLS")))
{
    builder.WebHost.UseUrls("http://localhost:8090");
}

var app = builder.Build();
var logger = app.Logger;

// Route SIPSorcery's internal ICE/DTLS logging through our logger (for diagnostics).
SIPSorcery.LogFactory.Set(app.Services.GetRequiredService<ILoggerFactory>());

string webRoot = ResolveWebRoot(builder.Environment.ContentRootPath);

static string ResolveWebRoot(string contentRoot)
{
    string? env = Environment.GetEnvironmentVariable("AXIOM_WEB_ROOT");
    if (!string.IsNullOrEmpty(env)) return Path.GetFullPath(env);
    string rel = Path.Combine("apps", "axiom-netplay-browser", "web");
    foreach (string start in new[] { contentRoot, AppContext.BaseDirectory, Directory.GetCurrentDirectory() })
        for (DirectoryInfo? dir = new(start); dir is not null; dir = dir.Parent)
        {
            string candidate = Path.Combine(dir.FullName, rel);
            if (Directory.Exists(candidate)) return candidate;
        }
    return Path.GetFullPath(Path.Combine(contentRoot, rel));
}

var game = new Game();
logger.LogInformation("Transport mode: {Mode}. Simulated server lag: {Lag}", transportMode, Game.LagSummary);
if (webTransportRequested && !quicSupported)
    logger.LogWarning(
        "WebTransport requested but HTTP/3 (QUIC) is not supported on this OS — needs " +
        "Windows 11 / Server 2022+ or Linux with libmsquic. Serving WebSocket only.");
_ = Task.Run(() => game.RunAsync(app.Lifetime.ApplicationStopping));

// WebTransport: a CONNECT request on the HTTP/3 endpoint is handled here (before
// anything else); everything else (HTTP/1.1 on 8090) falls through.
if (enableWebTransport)
{
    app.Use(async (context, next) =>
    {
        var feature = context.Features.Get<IHttpWebTransportFeature>();
        if (feature is { IsWebTransportRequest: true })
        {
            await WebTransportServer.HandleAsync(feature, game, logger, context.RequestAborted);
            return;
        }
        await next();
    });
}

app.UseWebSockets();

// The authoritative game socket over WebSocket.
app.Map("/ws", async context =>
{
    if (!context.WebSockets.IsWebSocketRequest)
    {
        context.Response.StatusCode = StatusCodes.Status400BadRequest;
        return;
    }
    using var ws = await context.WebSockets.AcceptWebSocketAsync();
    await game.RunSessionAsync(Game.WebSocketReader(ws), Game.WebSocketSender(ws), "ws", logger, context.RequestAborted);
});

// The self-signed WebTransport cert hash, so the page can trust it (no CA).
if (enableWebTransport)
    app.MapGet("/cert-hash", () => Results.Text(wtCertHashBase64!, "text/plain"));

// WebRTC signaling: always available (no special OS support needed). The page
// POSTs its SDP offer; we return the answer. The data channel carries the game.
app.MapPost("/rtc/offer", (HttpContext ctx) =>
    WebRtcServer.HandleOfferAsync(ctx, game, logger, ctx.RequestAborted));

// Serve the client (index.html + the wasm bundle + the vendored SDK).
if (Directory.Exists(webRoot))
{
    var files = new PhysicalFileProvider(webRoot);
    app.UseDefaultFiles(new DefaultFilesOptions { FileProvider = files });
    app.UseStaticFiles(new StaticFileOptions { FileProvider = files });
    logger.LogInformation("Serving the netplay client from {WebRoot}", webRoot);
}
else
{
    logger.LogWarning("Client web root not found at {WebRoot} — run `make netplay-build` first.", webRoot);
}

if (enableWebTransport)
    logger.LogInformation("WebTransport (HTTP/3) on https://localhost:8091 — open http://localhost:8090/?transport=webtransport");
logger.LogInformation("Axiom netplay (.NET authoritative server) ready.");
app.Run();
