using System.Text;

namespace Axiom.Netplay;

/// <summary>
/// Owns every live <see cref="Room"/>, keyed by the wire room id a client requests
/// in its <c>JoinRoom</c> frame. This is what turns the single hardcoded room into
/// multi-room: it is the only authority over which connection lands in which room.
///
/// A room — its own authoritative sim plus its own tick/broadcast
/// <see cref="RoomLoop"/> — is created on demand when the first client joins it,
/// and reaped (loop cancelled, sim disposed) when its last client leaves. Each
/// room is a fully independent deterministic game; nothing crosses between them.
/// </summary>
public sealed class RoomRegistry : IDisposable
{
    /// <summary>The room id used when a client sends an empty/absent one.</summary>
    public const string DefaultRoomId = "lobby";

    // Generous upper bound for a decoded room id (the protocol caps it well under
    // this); a fixed buffer avoids a per-join allocation dance.
    private const int RoomIdBufferLen = 256;

    private readonly AxiomWorker _worker;
    private readonly WorkerMetrics _metrics;
    private readonly ILogger _logger;
    private readonly ulong _seed;
    private readonly uint _maxPlayers;
    private readonly CancellationToken _appStopping;

    private readonly object _gate = new();
    private readonly Dictionary<string, Entry> _rooms = new();

    private sealed class Entry
    {
        public required Room Room { get; init; }
        public required CancellationTokenSource Cts { get; init; }
        public required Task Running { get; init; }
        public int Sessions;
    }

    /// <summary>Create a registry that mints rooms with the given sim parameters.</summary>
    public RoomRegistry(AxiomWorker worker, WorkerMetrics metrics, ILogger logger, ulong seed, uint maxPlayers, CancellationToken appStopping)
    {
        _worker = worker;
        _metrics = metrics;
        _logger = logger;
        _seed = seed;
        _maxPlayers = maxPlayers;
        _appStopping = appStopping;
    }

    /// <summary>How many rooms are currently live.</summary>
    public int RoomCount { get { lock (_gate) return _rooms.Count; } }

    /// <summary>The live room for an id, or null if none — for observability/tests.</summary>
    public Room? PeekRoom(string roomId)
    {
        lock (_gate) return _rooms.TryGetValue(roomId, out Entry? e) ? e.Room : null;
    }

    /// <summary>A snapshot of the live room ids (for the matchmaker to consider
    /// existing rooms with free slots).</summary>
    public IReadOnlyList<string> RoomIds()
    {
        lock (_gate) return _rooms.Keys.ToList();
    }

    /// <summary>Serve one connection over any transport: read its <c>JoinRoom</c>
    /// frame, validate the protocol, route by room id (creating the room on
    /// demand), run the room session, and reap the room when its last client
    /// leaves. A browser can still only ever send JoinRoom / LeaveRoom /
    /// ClientIntent — the room id only selects which authoritative sim it talks
    /// to, never what it may submit.</summary>
    public async Task ServeAsync(ReadFrame read, SendFrame send, string transport, CancellationToken ct)
    {
        byte[]? join = await read(ct);
        if (join is null) return;
        if (AxiomWorkerNative.axiom_decode_join_version(join, (nuint)join.Length) != Room.ProtocolVersion)
        {
            _logger.LogWarning("[{Transport}] first frame was not a JoinRoom on protocol {V}", transport, Room.ProtocolVersion);
            return;
        }
        string? roomId = DecodeRoomId(join);
        if (roomId is null)
        {
            _logger.LogWarning("[{Transport}] JoinRoom carried no decodable room id", transport);
            return;
        }

        Entry entry = Acquire(roomId);
        try { await entry.Room.RunSessionAsync(join, read, send, transport, _logger, ct); }
        finally { await ReleaseAsync(roomId); }
    }

    private Entry Acquire(string roomId)
    {
        lock (_gate)
        {
            if (!_rooms.TryGetValue(roomId, out Entry? entry))
            {
                var room = new Room(_worker, _seed, _maxPlayers, _metrics);
                var cts = CancellationTokenSource.CreateLinkedTokenSource(_appStopping);
                var loop = new RoomLoop(room, _metrics, _logger);
                entry = new Entry { Room = room, Cts = cts, Running = loop.RunAsync(cts.Token) };
                _rooms[roomId] = entry;
                _logger.LogInformation("room '{Room}' created ({Count} live)", roomId, _rooms.Count);
            }
            entry.Sessions++;
            return entry;
        }
    }

    private async Task ReleaseAsync(string roomId)
    {
        Entry? reap = null;
        lock (_gate)
        {
            if (_rooms.TryGetValue(roomId, out Entry? entry) && --entry.Sessions <= 0)
            {
                _rooms.Remove(roomId);
                reap = entry;
            }
        }
        if (reap is null) return;

        // Stop the loop and wait for the in-flight tick to finish BEFORE disposing
        // the sim, so a tick never runs against a freed handle.
        reap.Cts.Cancel();
        try { await reap.Running; } catch { /* loops just observe cancellation */ }
        reap.Room.Dispose();
        reap.Cts.Dispose();
        _logger.LogInformation("room '{Room}' reaped ({Count} live)", roomId, RoomCount);
    }

    /// Decode the JoinRoom's room id as a lossless Latin-1 key (1:1 over bytes
    /// 0-255, and renders ASCII room ids readably), or null if it is not a valid
    /// JoinRoom.
    private static string? DecodeRoomId(byte[] join)
    {
        var buf = new byte[RoomIdBufferLen];
        nint n = AxiomWorkerNative.axiom_decode_join_room_id(join, (nuint)join.Length, buf, (nuint)buf.Length);
        return n < 0 ? null : Encoding.Latin1.GetString(buf, 0, (int)n);
    }

    /// <inheritdoc/>
    public void Dispose()
    {
        List<Entry> entries;
        lock (_gate)
        {
            entries = _rooms.Values.ToList();
            _rooms.Clear();
        }
        foreach (Entry e in entries)
        {
            e.Cts.Cancel();
            try { e.Running.Wait(TimeSpan.FromSeconds(5)); } catch { /* best-effort shutdown */ }
            e.Room.Dispose();
            e.Cts.Dispose();
        }
    }
}
