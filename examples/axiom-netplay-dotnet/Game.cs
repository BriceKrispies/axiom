using System.Net.WebSockets;

namespace Axiom.Netplay;

/// <summary>Send one frame to a client over whatever transport it connected with.</summary>
public delegate Task SendFrame(ReadOnlyMemory<byte> data, CancellationToken ct);

/// <summary>Read the next inbound frame, or null when the connection ends.</summary>
public delegate Task<byte[]?> ReadFrame(CancellationToken ct);

/// <summary>
/// The authoritative two-player game — backed entirely by the embedded Axiom
/// engine (see <see cref="Ffi"/>) and **transport-neutral**. The same logic
/// serves WebSocket and WebTransport: each connection supplies a
/// <see cref="ReadFrame"/> + <see cref="SendFrame"/> pair and the engine does the
/// rest. C# here is pure transport plumbing — no game logic, no protocol codec
/// (both come from the one Rust source of truth across the FFI boundary).
/// </summary>
public sealed class Game : IDisposable
{
    private const uint ProtocolVersion = 1;
    private const ulong FixedStepNs = 16_666_667; // ~60 Hz
    private const ulong MaxPlayers = 2;

    // Wire discriminants this transport switches on (mirrored from the protocol).
    private const int KindLeaveRoom = 1;
    private const int KindClientIntent = 2;

    // Simulated server lag (see the previous step): delayed, jittery delivery.
    private static readonly int LagBaseMs = EnvInt("AXIOM_LAG_MS", 90);
    private static readonly int LagJitterMs = EnvInt("AXIOM_JITTER_MS", 120);
    public static string LagSummary => $"{LagBaseMs}ms base + 0..{LagJitterMs}ms jitter";
    private static int EnvInt(string key, int fallback) =>
        int.TryParse(Environment.GetEnvironmentVariable(key), out int v) && v >= 0 ? v : fallback;

    private readonly object _gate = new();
    private readonly IntPtr _session;
    private ulong _tick;
    private byte[] _latestState = Array.Empty<byte>();
    private readonly List<Client> _clients = new();

    private sealed class Client
    {
        public required ulong Id;
        public required SendFrame Send;
        public ulong LastAcceptedSeq;
        public readonly SemaphoreSlim SendLock = new(1, 1);
    }

    public Game()
    {
        Ffi.EnsureLoaded();
        _session = Ffi.axiom_netplay_create();
        if (_session == IntPtr.Zero)
            throw new InvalidOperationException(
                "failed to create the Axiom engine session — is the FFI library built? " +
                "run `cargo build -p axiom-netplay-ffi --release` (or set AXIOM_FFI_LIB).");
    }

    public void Dispose()
    {
        lock (_gate) Ffi.axiom_netplay_destroy(_session);
    }

    /// <summary>
    /// Serve one connection (any transport): await its JoinRoom, register a slot,
    /// reply Welcome, then fold its intents into the engine until it leaves.
    /// </summary>
    public async Task RunSessionAsync(ReadFrame read, SendFrame send, string transport, ILogger logger, CancellationToken ct)
    {
        // 1. The first frame must be a compatible JoinRoom (decoded by the engine).
        byte[]? first = await read(ct);
        if (first is null) return;
        if (Ffi.axiom_decode_join_version(first, (nuint)first.Length) != ProtocolVersion)
        {
            logger.LogWarning("[{Transport}] first frame was not a JoinRoom on protocol {V}", transport, ProtocolVersion);
            return;
        }

        // 2. Claim a slot; build the Welcome under lock.
        Client? client = null;
        byte[]? welcome = null;
        lock (_gate)
        {
            ulong id = FreeId();
            if (id != 0)
            {
                client = new Client { Id = id, Send = send };
                _clients.Add(client);
                welcome = Encode(buf => Ffi.axiom_encode_welcome(ProtocolVersion, id, _tick, FixedStepNs, buf, (nuint)buf.Length), 64);
            }
        }
        if (client is null || welcome is null)
        {
            logger.LogInformation("[{Transport}] rejected an extra player (game is full)", transport);
            return;
        }

        // 3. Welcome, then fold intents until the client leaves.
        await SendLockedAsync(client, welcome, ct);
        logger.LogInformation("[{Transport}] player {Id} joined (engine-backed)", transport, client.Id);
        try
        {
            while (true)
            {
                byte[]? frame = await read(ct);
                if (frame is null) break;

                int kind = Ffi.axiom_msg_kind(frame, (nuint)frame.Length);
                if (kind == KindLeaveRoom) break;
                if (kind == KindClientIntent)
                {
                    int ok = Ffi.axiom_decode_client_intent(frame, (nuint)frame.Length, out ulong seq, out float dx, out float dy);
                    if (ok == 1)
                        lock (_gate)
                        {
                            Ffi.axiom_netplay_apply_intent(_session, (uint)(client.Id - 1), dx, dy);
                            if (seq > client.LastAcceptedSeq) client.LastAcceptedSeq = seq;
                        }
                }
            }
        }
        finally
        {
            lock (_gate) _clients.Remove(client);
            logger.LogInformation("[{Transport}] player {Id} left", transport, client.Id);
        }
    }

    /// <summary>Run the authoritative loops: tick the engine at 60 Hz; deliver
    /// snapshots on the (laggy, jittery) cadence. Decoupled so lag delays delivery
    /// without slowing the simulation.</summary>
    public Task RunAsync(CancellationToken ct) =>
        Task.WhenAll(TickLoopAsync(ct), DeliverLoopAsync(ct));

    private async Task TickLoopAsync(CancellationToken ct)
    {
        using var timer = new PeriodicTimer(TimeSpan.FromMilliseconds(16));
        try
        {
            while (await timer.WaitForNextTickAsync(ct))
                lock (_gate)
                {
                    Ffi.axiom_netplay_tick(_session);
                    var positions = new float[4];
                    Ffi.axiom_netplay_positions(_session, positions, (nuint)positions.Length);
                    _tick++;
                    _latestState = FloatsToBytes(positions);
                }
        }
        catch (OperationCanceledException) { /* shutting down */ }
    }

    private async Task DeliverLoopAsync(CancellationToken ct)
    {
        try
        {
            while (!ct.IsCancellationRequested)
            {
                List<(Client Client, byte[] Bytes)> sends = new();
                lock (_gate)
                {
                    if (_latestState.Length > 0)
                    {
                        ulong tick = _tick;
                        byte[] payload = _latestState;
                        foreach (Client c in _clients)
                        {
                            ulong acked = c.LastAcceptedSeq;
                            byte[] snapshot = Encode(
                                buf => Ffi.axiom_encode_snapshot(tick, acked, payload, (nuint)payload.Length, buf, (nuint)buf.Length),
                                payload.Length + 64);
                            sends.Add((c, snapshot));
                        }
                    }
                }

                foreach (var (client, bytes) in sends)
                {
                    try { await SendLockedAsync(client, bytes, ct); }
                    catch { /* a dropped client is reaped by its own session loop */ }
                }

                int lag = LagBaseMs + Random.Shared.Next(0, LagJitterMs + 1);
                await Task.Delay(lag, ct);
            }
        }
        catch (OperationCanceledException) { /* shutting down */ }
    }

    private ulong FreeId()
    {
        for (ulong id = 1; id <= MaxPlayers; id++)
            if (!_clients.Any(c => c.Id == id))
                return id;
        return 0;
    }

    private static async Task SendLockedAsync(Client client, ReadOnlyMemory<byte> bytes, CancellationToken ct)
    {
        await client.SendLock.WaitAsync(ct);
        try { await client.Send(bytes, ct); }
        finally { client.SendLock.Release(); }
    }

    private static byte[] Encode(Func<byte[], nint> encode, int capacity)
    {
        var buffer = new byte[capacity];
        nint written = encode(buffer);
        if (written < 0)
            throw new InvalidOperationException("engine codec failed to encode a frame");
        return buffer.AsSpan(0, (int)written).ToArray();
    }

    private static byte[] FloatsToBytes(float[] floats)
    {
        var bytes = new byte[floats.Length * 4];
        Buffer.BlockCopy(floats, 0, bytes, 0, bytes.Length);
        return bytes;
    }

    // --- WebSocket frame source/sink (WebSocket preserves message boundaries) ---

    public static ReadFrame WebSocketReader(WebSocket ws) => async (ct) =>
    {
        var buffer = new byte[4096];
        using var ms = new MemoryStream();
        while (true)
        {
            WebSocketReceiveResult result;
            try { result = await ws.ReceiveAsync(buffer, ct); }
            catch { return null; }
            if (result.MessageType == WebSocketMessageType.Close) return null;
            ms.Write(buffer, 0, result.Count);
            if (result.EndOfMessage) break;
        }
        return ms.ToArray();
    };

    public static SendFrame WebSocketSender(WebSocket ws) =>
        (data, ct) => ws.SendAsync(data, WebSocketMessageType.Binary, endOfMessage: true, ct).AsTask();
}
