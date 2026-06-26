namespace Axiom.Netplay;

/// <summary>
/// The scaleout directory: which game NODE owns which room, and the matchmaking
/// that assigns them. The director uses this to answer <c>/matchmake</c> with a
/// <c>(nodeUrl, roomId)</c> pair; the browser then connects DIRECTLY to that
/// node's game socket. The director is never in the data path.
///
/// The interface exists so a production deployment can swap the in-memory
/// implementation for a shared/replicated store (e.g. Redis) without touching the
/// director — the same reason <c>IAxiomSim</c> abstracts where a sim runs.
/// </summary>
public interface IRoomDirectory
{
    /// <summary>Register a game node by its browser-reachable ws URL (idempotent).</summary>
    void RegisterNode(string nodeWsUrl);

    /// <summary>The registered game nodes.</summary>
    IReadOnlyList<string> Nodes { get; }

    /// <summary>How many rooms are currently assigned across all nodes.</summary>
    int RoomCount { get; }

    /// <summary>Assign a ticket to a room with a free slot, spreading new rooms
    /// across nodes. Returns the owning node's ws URL and the room id. Throws if no
    /// node is registered yet.</summary>
    (string NodeUrl, string RoomId) AssignMatch();
}

/// <summary>In-memory room directory: room→node assignment with reservation-based
/// occupancy (the same quickplay shape as <see cref="Matchmaker"/>, but choosing a
/// node too). New rooms go to the least-loaded node, so load spreads.</summary>
public sealed class InMemoryRoomDirectory : IRoomDirectory
{
    private readonly int _capacity;
    private readonly long _reservationTtlMs;
    private readonly Func<long> _nowMs;

    private readonly object _gate = new();
    private readonly List<string> _nodes = new();
    private readonly Dictionary<string, RoomRec> _rooms = new();
    private int _counter;

    private sealed class RoomRec
    {
        public required string RoomId { get; init; }
        public required string NodeUrl { get; init; }
        public List<long> Reservations { get; } = new();
    }

    /// <summary>Create a directory with the per-room capacity and reservation TTL.</summary>
    public InMemoryRoomDirectory(int capacity, long reservationTtlMs = 15_000, Func<long>? nowMs = null)
    {
        _capacity = capacity;
        _reservationTtlMs = reservationTtlMs;
        _nowMs = nowMs ?? (() => Environment.TickCount64);
    }

    /// <inheritdoc/>
    public void RegisterNode(string nodeWsUrl)
    {
        lock (_gate)
        {
            if (!_nodes.Contains(nodeWsUrl)) _nodes.Add(nodeWsUrl);
        }
    }

    /// <inheritdoc/>
    public IReadOnlyList<string> Nodes
    {
        get { lock (_gate) return _nodes.ToList(); }
    }

    /// <inheritdoc/>
    public int RoomCount
    {
        get { lock (_gate) return _rooms.Count; }
    }

    /// <inheritdoc/>
    public (string NodeUrl, string RoomId) AssignMatch()
    {
        lock (_gate)
        {
            if (_nodes.Count == 0)
                throw new InvalidOperationException("no game nodes registered with the director");
            Prune();

            // Fill an existing room with a free slot first (deterministic order).
            foreach (RoomRec room in _rooms.Values.OrderBy(r => r.RoomId, StringComparer.Ordinal))
                if (room.Reservations.Count < _capacity)
                {
                    Reserve(room);
                    return (room.NodeUrl, room.RoomId);
                }

            // None free — open a new room on the least-loaded node so load spreads.
            string node = LeastLoadedNode();
            var rec = new RoomRec { RoomId = $"mm-{++_counter}", NodeUrl = node };
            _rooms[rec.RoomId] = rec;
            Reserve(rec);
            return (rec.NodeUrl, rec.RoomId);
        }
    }

    private string LeastLoadedNode() =>
        _nodes.OrderBy(n => _rooms.Values.Count(r => r.NodeUrl == n)).First();

    private void Reserve(RoomRec room) => room.Reservations.Add(_nowMs() + _reservationTtlMs);

    private void Prune()
    {
        long now = _nowMs();
        foreach (RoomRec room in _rooms.Values) room.Reservations.RemoveAll(expiry => expiry <= now);
    }
}
