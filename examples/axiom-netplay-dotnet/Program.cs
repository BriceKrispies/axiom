using System.Net.Http.Json;
using Microsoft.AspNetCore.Http.Features;
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.FileProviders;
using Axiom.Netplay;

// Server role:
//   "allinone" (default) — director + node in one process; same-origin matchmaking.
//   "director"           — matchmaker + room directory; redirects browsers to a node.
//   "node"               — hosts game rooms; registers itself with a director.
string role = (Environment.GetEnvironmentVariable("AXIOM_ROLE") ?? "allinone").ToLowerInvariant();
bool isNode = role is "node" or "allinone";
bool isDirector = role is "director";
bool isAllInOne = role is "allinone";

// Transport mode: "websocket" (default) serves only WS; "webtransport" (or "both")
// ALSO stands up an HTTP/3 endpoint so the page can pick WebTransport.
string transportMode = (Environment.GetEnvironmentVariable("AXIOM_TRANSPORT") ?? "websocket").ToLowerInvariant();
bool webTransportRequested = transportMode is "webtransport" or "both";

bool quicSupported = System.Net.Quic.QuicListener.IsSupported;
bool enableWebTransport = webTransportRequested && quicSupported;

var (wtCert, wtCertHashBase64) = enableWebTransport
    ? WebTransportServer.CreateDevCertificate()
    : (null, null);
if (enableWebTransport)
    AppContext.SetSwitch("Microsoft.AspNetCore.Server.Kestrel.Experimental.WebTransportAndH3Datagrams", true);

var builder = WebApplication.CreateBuilder(args);

if (enableWebTransport)
{
    builder.WebHost.ConfigureKestrel(options =>
    {
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

SIPSorcery.LogFactory.Set(app.Services.GetRequiredService<ILoggerFactory>());

string webRoot = ResolveWebRoot(builder.Environment.ContentRootPath);

const ulong roomSeed = 1;
const uint maxPlayers = 2;

// --- Game node (role "node" or "allinone"): the worker + the rooms ---
RoomRegistry? registry = null;
Matchmaker? matchmaker = null;
WorkerMetrics? metrics = null;
WorkerHealth? health = null;
if (isNode)
{
    // Construct the worker (loads the native library / sets the worker mode),
    // enforce the version handshake, and run the boot self-test before serving.
    var worker = new AxiomWorker(logger);
    logger.LogInformation("Axiom worker version: {Version} (mode: {Mode})", worker.Version, worker.OutOfProcess ? "out-of-process" : "in-process");
    worker.CheckCompatibility();

    health = new WorkerHealth(worker);
    health.RunSelfTest();
    if (!health.Ready)
    {
        logger.LogCritical("Axiom worker is not ready (self-test failed) — refusing to serve.");
        return;
    }

    metrics = new WorkerMetrics();
    // One registry owns every live room (each its own sim + tick/broadcast loop),
    // created on demand when a client joins it and reaped when its last client leaves.
    registry = new RoomRegistry(worker, metrics, logger, roomSeed, maxPlayers, app.Lifetime.ApplicationStopping);
    app.Lifetime.ApplicationStopped.Register(() => registry.Dispose());
    // All-in-one matchmaking (same-origin); the director uses the directory below.
    matchmaker = new Matchmaker(registry, (int)maxPlayers);
}

// --- Director (role "director"): the room directory + node registry ---
InMemoryRoomDirectory? directory = isDirector ? new InMemoryRoomDirectory((int)maxPlayers) : null;
if (isDirector)
{
    // Optionally seed a static node list (e.g. AXIOM_NODES="ws://localhost:8101/ws,...").
    string? seed = Environment.GetEnvironmentVariable("AXIOM_NODES");
    if (!string.IsNullOrEmpty(seed))
        foreach (string node in seed.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
            directory!.RegisterNode(node);
}

logger.LogInformation("Role: {Role}. Transport: {Transport}. Simulated server lag: {Lag}", role, transportMode, RoomLoop.LagSummary);

if (webTransportRequested && !quicSupported)
    logger.LogWarning(
        "WebTransport requested but HTTP/3 (QUIC) is not supported on this OS — needs " +
        "Windows 11 / Server 2022+ or Linux with libmsquic. Serving WebSocket only.");

// WebTransport: a CONNECT request on the HTTP/3 endpoint is handled here (node only).
if (enableWebTransport && registry is not null)
{
    app.Use(async (context, next) =>
    {
        var feature = context.Features.Get<IHttpWebTransportFeature>();
        if (feature is { IsWebTransportRequest: true })
        {
            await WebTransportServer.HandleAsync(feature, registry, logger, context.RequestAborted);
            return;
        }
        await next();
    });
}

app.UseWebSockets();

// The authoritative game socket + WebRTC signaling (game node only).
if (registry is not null)
{
    app.Map("/ws", async context =>
    {
        if (!context.WebSockets.IsWebSocketRequest)
        {
            context.Response.StatusCode = StatusCodes.Status400BadRequest;
            return;
        }
        using var ws = await context.WebSockets.AcceptWebSocketAsync();
        await registry.ServeAsync(Transport.WebSocketReader(ws), Transport.WebSocketSender(ws), "ws", context.RequestAborted);
    });

    app.MapPost("/rtc/offer", (HttpContext ctx, CancellationToken ct) =>
        WebRtcServer.HandleOfferAsync(ctx, registry, logger, ct));

    if (enableWebTransport)
        app.MapGet("/cert-hash", () => Results.Text(wtCertHashBase64!, "text/plain"));
}

// Liveness, readiness, and metrics.
app.MapGet("/healthz", () => Results.Ok(new { status = "live", role }));
app.MapGet("/readyz", () =>
{
    bool ready = isDirector ? directory!.Nodes.Count > 0 : health?.Ready ?? false;
    object report = isDirector
        ? new { role, nodes = directory!.Nodes.Count, ready }
        : health!.Report();
    return ready ? Results.Ok(report) : Results.Json(report, statusCode: StatusCodes.Status503ServiceUnavailable);
});
if (registry is not null && metrics is not null)
    app.MapGet("/metrics", () => Results.Ok(new { role, rooms = registry.RoomCount, worker = metrics.Report() }));

// Matchmaking. All-in-one hands back a room id (same-origin). The director hands
// back the owning node's URL too, so the browser connects directly to that node.
if (isAllInOne)
{
    app.MapPost("/matchmake", () => Results.Ok(new { roomId = matchmaker!.FindOrCreate() }));
}
else if (isDirector)
{
    app.MapPost("/matchmake", () =>
    {
        try
        {
            (string nodeUrl, string roomId) = directory!.AssignMatch();
            return Results.Ok(new { roomId, nodeUrl });
        }
        catch (InvalidOperationException)
        {
            return Results.Json(new { error = "no game nodes available yet" }, statusCode: StatusCodes.Status503ServiceUnavailable);
        }
    });
    app.MapPost("/nodes/register", async (HttpContext ctx) =>
    {
        var reg = await ctx.Request.ReadFromJsonAsync<NodeRegistration>();
        if (reg is { Url.Length: > 0 })
        {
            directory!.RegisterNode(reg.Url);
            logger.LogInformation("game node registered: {Url} ({N} total)", reg.Url, directory.Nodes.Count);
            return Results.Ok();
        }
        return Results.BadRequest();
    });
}

// Serve the client (index.html + the wasm bundle + the vendored SDK). The director
// serves the page; the browser then matchmakes and connects to a node.
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

// A game node registers itself with its director (retrying until the director is up).
if (role == "node")
{
    string? directorUrl = Environment.GetEnvironmentVariable("AXIOM_DIRECTOR_URL");
    string? nodeUrl = Environment.GetEnvironmentVariable("AXIOM_NODE_URL");
    if (!string.IsNullOrEmpty(directorUrl) && !string.IsNullOrEmpty(nodeUrl))
        _ = RegisterWithDirectorAsync(directorUrl, nodeUrl, logger, app.Lifetime.ApplicationStopping);
    else
        logger.LogWarning("node role but AXIOM_DIRECTOR_URL / AXIOM_NODE_URL not set — will not register");
}

logger.LogInformation("Axiom netplay ({Role}) ready.", role);
app.Run();

static string ResolveWebRoot(string contentRoot)
{
    string? env = Environment.GetEnvironmentVariable("AXIOM_WEB_ROOT");
    if (!string.IsNullOrEmpty(env)) return Path.GetFullPath(env);
    // Every browser demo is merged into the gallery and packaged into dist/ (one
    // shared wasm bundle + every demo's page). `make netplay-build` builds that
    // dist/, so the netplay client lives at dist/netplay/ — serve dist/ and open
    // /netplay/. (Override with AXIOM_WEB_ROOT for a custom layout.)
    string rel = "dist";
    foreach (string start in new[] { contentRoot, AppContext.BaseDirectory, Directory.GetCurrentDirectory() })
        for (DirectoryInfo? dir = new(start); dir is not null; dir = dir.Parent)
        {
            string candidate = Path.Combine(dir.FullName, rel);
            if (Directory.Exists(candidate)) return candidate;
        }
    return Path.GetFullPath(Path.Combine(contentRoot, rel));
}

static async Task RegisterWithDirectorAsync(string directorUrl, string nodeWsUrl, ILogger logger, CancellationToken ct)
{
    using var http = new HttpClient();
    string url = $"{directorUrl.TrimEnd('/')}/nodes/register";
    for (int attempt = 0; attempt < 120 && !ct.IsCancellationRequested; attempt++)
    {
        try
        {
            var resp = await http.PostAsJsonAsync(url, new { url = nodeWsUrl }, ct);
            if (resp.IsSuccessStatusCode)
            {
                logger.LogInformation("registered with director {Director} as {Url}", directorUrl, nodeWsUrl);
                return;
            }
        }
        catch { /* director not up yet — retry */ }
        try { await Task.Delay(500, ct); } catch { return; }
    }
}

/// <summary>A game node's self-registration payload (its browser-reachable ws URL).</summary>
internal sealed record NodeRegistration(string Url);
