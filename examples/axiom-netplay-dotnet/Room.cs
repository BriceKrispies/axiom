using System.Buffers.Binary;

namespace Axiom.Netplay;

/// <summary>
/// One authoritative room, driven entirely by the in-process Axiom simulation
/// worker. The room owns player-slot assignment and the authoritative snapshot;
/// it is transport-neutral (a connection supplies a <see cref="ReadFrame"/> +
/// <see cref="SendFrame"/> pair). A browser can only ever send a Tier-A
/// <c>JoinRoom</c>, <c>LeaveRoom</c>, or <c>ClientIntent</c> — there is no code
/// path by which it can submit state, a snapshot, a hash, a score, or an event.
/// </summary>
public sealed class Room : IDisposable
{
    /// <summary>The browser-facing protocol version (Tier-A).</summary>
    public const uint ProtocolVersion = 1;
    /// <summary>The fixed simulation step (~60 Hz), in nanoseconds.</summary>
    public const ulong FixedStepNs = 16_666_667;

    // Wire message-kind discriminants this room acts on (mirrored from the protocol).
    private const int KindLeaveRoom = 1;
    private const int KindClientIntent = 2;

    private readonly IAxiomSim _sim;
    private readonly WorkerMetrics _metrics;
    private readonly uint _maxPlayers;
    private readonly object _gate = new();
    private readonly List<ClientSession> _clients = new();

    private ulong _tick;
    // The authoritative renderable view (each player's x,y as little-endian f32) —
    // a read-only projection the worker derives from the engine. This is the
    // ServerSnapshot payload; clients render and reconcile against it. The
    // authoritative *state hash* still comes from the full snapshot_sim bytes.
    private byte[] _latestRenderView = Array.Empty<byte>();
    private ulong _latestStateHash;

    /// <summary>Create a room over a fresh sim instance.</summary>
    public Room(AxiomWorker worker, ulong seed, uint maxPlayers, WorkerMetrics metrics)
    {
        _sim = worker.CreateSim(seed, maxPlayers, FixedStepNs);
        _metrics = metrics;
        _maxPlayers = maxPlayers;
    }

    /// <summary>The authoritative tick count.</summary>
    public ulong Tick { get { lock (_gate) return _tick; } }

    /// <summary>The current authoritative state hash.</summary>
    public ulong StateHash { get { lock (_gate) return _latestStateHash; } }

    /// <summary>How many clients are connected.</summary>
    public int ClientCount { get { lock (_gate) return _clients.Count; } }

    /// <summary>Export the deterministic replay record (for persistence / audit).</summary>
    public byte[] ExportReplay() => _sim.ExportReplay();

    /// <summary>Advance exactly one authoritative tick and refresh the snapshot.
    /// Called by the room loop on the fixed cadence; the worker owns determinism,
    /// the loop owns wall-clock pacing.</summary>
    public void TickOnce()
    {
        var sw = System.Diagnostics.Stopwatch.StartNew();
        ulong next;
        lock (_gate) next = _tick + 1;
        TickOutcome outcome = _sim.AdvanceTick(next);
        double workerCallMs = sw.Elapsed.TotalMilliseconds;
        byte[] view = FloatsToBytes(_sim.GetRenderView());
        lock (_gate)
        {
            _tick = outcome.Tick;
            _latestRenderView = view;
            _latestStateHash = outcome.StateHash;
        }
        _metrics.RecordTick(sw.Elapsed.TotalMilliseconds, workerCallMs, view.Length, outcome.StateHash);
    }

    private static byte[] FloatsToBytes(float[] floats)
    {
        var bytes = new byte[floats.Length * 4];
        for (int i = 0; i < floats.Length; i++)
            BinaryPrimitives.WriteSingleLittleEndian(bytes.AsSpan(i * 4, 4), floats[i]);
        return bytes;
    }

    /// <summary>Build the authoritative snapshot frames to broadcast this round,
    /// one per connected client (each carries that client's last-accepted sequence).</summary>
    public IReadOnlyList<(ClientSession Client, byte[] Frame)> BuildBroadcast()
    {
        lock (_gate)
        {
            if (_latestRenderView.Length == 0) return Array.Empty<(ClientSession, byte[])>();
            var list = new List<(ClientSession, byte[])>(_clients.Count);
            foreach (ClientSession c in _clients)
                list.Add((c, EncodeSnapshot(_tick, c.LastAcceptedSeq, _latestRenderView)));
            return list;
        }
    }

    /// <summary>Serve one connection over any transport: await its JoinRoom, claim
    /// a slot, reply Welcome, then fold its intents into the worker until it leaves.</summary>
    public async Task RunSessionAsync(byte[] joinFrame, ReadFrame read, SendFrame send, string transport, ILogger logger, CancellationToken ct)
    {
        // The RoomRegistry has already read this connection's JoinRoom frame and
        // routed us by its room id; we re-validate the protocol defensively, then
        // own slot assignment and the per-connection intent loop from here. `read`
        // yields the frames *after* the JoinRoom.
        if (AxiomWorkerNative.axiom_decode_join_version(joinFrame, (nuint)joinFrame.Length) != ProtocolVersion)
        {
            logger.LogWarning("[{Transport}] first frame was not a JoinRoom on protocol {V}", transport, ProtocolVersion);
            return;
        }

        ClientSession? client = ClaimSlot(send);
        if (client is null)
        {
            logger.LogInformation("[{Transport}] rejected an extra player (room is full)", transport);
            return;
        }

        await SendLockedAsync(client, EncodeWelcome(client.WireId, _tick), ct);
        logger.LogInformation("[{Transport}] player {Id} joined slot {Slot}", transport, client.WireId, client.Slot);
        try
        {
            await ReadLoopAsync(client, read, transport, logger, ct);
        }
        finally
        {
            lock (_gate) _clients.Remove(client);
            _metrics.Disconnect();
            logger.LogInformation("[{Transport}] player {Id} left", transport, client.WireId);
        }
    }

    private async Task ReadLoopAsync(ClientSession client, ReadFrame read, string transport, ILogger logger, CancellationToken ct)
    {
        while (true)
        {
            byte[]? frame = await read(ct);
            if (frame is null) break;

            int kind = AxiomWorkerNative.axiom_msg_kind(frame, (nuint)frame.Length);
            if (kind == KindLeaveRoom) break;
            if (kind != KindClientIntent)
            {
                // Anything else from a client (an unknown frame, or a server→client
                // kind the client must never send) is malformed. It is dropped as
                // evidence; it can never mutate state.
                _metrics.MalformedFrame();
                continue;
            }
            HandleIntent(client, frame, ct);
        }
    }

    private void HandleIntent(ClientSession client, byte[] frame, CancellationToken ct)
    {
        int ok = AxiomWorkerNative.axiom_decode_client_intent(frame, (nuint)frame.Length, out ulong seq, out float dx, out float dy);
        if (ok != 1)
        {
            _metrics.MalformedFrame();
            return;
        }

        // Rebuild the opaque ruleset payload from the decoded move (the worker's
        // ruleset re-interprets it; predicted-tick is informational in v1).
        var payload = new byte[8];
        BinaryPrimitives.WriteSingleLittleEndian(payload.AsSpan(0, 4), dx);
        BinaryPrimitives.WriteSingleLittleEndian(payload.AsSpan(4, 4), dy);

        IntentOutcome outcome;
        try
        {
            outcome = _sim.SubmitIntent(client.Slot, seq, predictedClientTick: 0, payload);
        }
        catch (AxiomWorkerException)
        {
            _metrics.WorkerError();
            return;
        }

        if (outcome.Accepted)
        {
            _metrics.Accepted();
            if (seq > client.LastAcceptedSeq) client.LastAcceptedSeq = seq;
        }
        else
        {
            _metrics.Rejected(outcome.Reason);
            // Echo a Tier-A RejectedIntent so the client can roll the intent out
            // of its prediction buffer. Best-effort; a dropped client is reaped.
            _ = SendLockedAsync(client, EncodeRejected(seq, outcome.WireReasonCode), ct);
        }
    }

    private ClientSession? ClaimSlot(SendFrame send)
    {
        lock (_gate)
        {
            for (uint slot = 0; slot < _maxPlayers; slot++)
                if (!_clients.Any(c => c.Slot == slot))
                {
                    var client = new ClientSession(slot, send);
                    _clients.Add(client);
                    return client;
                }
            return null;
        }
    }

    private static async Task SendLockedAsync(ClientSession client, ReadOnlyMemory<byte> bytes, CancellationToken ct)
    {
        await client.SendLock.WaitAsync(ct);
        try { await client.Send(bytes, ct); }
        catch { /* a dropped client is reaped by its own session loop */ }
        finally { client.SendLock.Release(); }
    }

    /// <summary>Deliver one prepared frame to a client (used by the room loop's broadcast).</summary>
    public static Task DeliverAsync(ClientSession client, byte[] frame, CancellationToken ct) =>
        SendLockedAsync(client, frame, ct);

    private static byte[] EncodeWelcome(ulong clientId, ulong serverTick) =>
        Encode(buf => AxiomWorkerNative.axiom_encode_welcome(ProtocolVersion, clientId, serverTick, FixedStepNs, buf, (nuint)buf.Length), 64);

    private static byte[] EncodeSnapshot(ulong serverTick, ulong lastAccepted, byte[] payload) =>
        Encode(buf => AxiomWorkerNative.axiom_encode_snapshot(serverTick, lastAccepted, payload, (nuint)payload.Length, buf, (nuint)buf.Length), payload.Length + 64);

    private static byte[] EncodeRejected(ulong seq, uint reason) =>
        Encode(buf => AxiomWorkerNative.axiom_encode_rejected(seq, reason, buf, (nuint)buf.Length), 64);

    private static byte[] Encode(Func<byte[], nint> encode, int capacity)
    {
        var buffer = new byte[capacity];
        nint written = encode(buffer);
        if (written < 0)
            throw new InvalidOperationException("engine codec failed to encode a frame");
        return buffer.AsSpan(0, (int)written).ToArray();
    }

    /// <inheritdoc/>
    public void Dispose() => _sim.Dispose();
}

/// <summary>Per-connection state: the server-assigned slot, the send sink, and
/// the last client sequence the worker accepted for this player (echoed in
/// snapshots so the client can reconcile its prediction buffer).</summary>
public sealed class ClientSession
{
    /// <summary>The 0-based worker player slot the server assigned.</summary>
    public uint Slot { get; }
    /// <summary>The 1-based wire client id (slot + 1).</summary>
    public ulong WireId => Slot + 1;
    /// <summary>The frame sink for this connection.</summary>
    public SendFrame Send { get; }
    /// <summary>The last client sequence the worker accepted for this player.</summary>
    public ulong LastAcceptedSeq { get; set; }
    /// <summary>Serializes concurrent sends to this one connection.</summary>
    public SemaphoreSlim SendLock { get; } = new(1, 1);

    /// <summary>Create a session for a claimed slot.</summary>
    public ClientSession(uint slot, SendFrame send)
    {
        Slot = slot;
        Send = send;
    }
}
