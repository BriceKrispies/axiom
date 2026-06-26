namespace Axiom.Netplay;

/// <summary>
/// Quickplay matchmaking: hand each ticket a room that has a free slot, minting a
/// fresh room id when every known room is full. A ticket is a *reservation* — the
/// caller is expected to connect (JoinRoom) shortly after — so occupancy counts
/// both the room's live clients AND its outstanding, not-yet-expired reservations.
/// That stops a burst of tickets from all piling into one room before anyone has
/// actually connected, while still filling rooms compactly.
///
/// This is host orchestration, not simulation: it may read wall-clock time for
/// reservation expiry. The authoritative sim stays deterministic and untouched.
/// </summary>
public sealed class Matchmaker
{
    private readonly RoomRegistry _registry;
    private readonly int _capacity;
    private readonly long _reservationTtlMs;
    private readonly Func<long> _nowMs;

    private readonly object _gate = new();
    private readonly List<Reservation> _reservations = new();
    private int _counter;

    private readonly record struct Reservation(string RoomId, long ExpiresAtMs);

    /// <summary>Create a matchmaker over a registry. <paramref name="capacity"/> is
    /// the per-room player cap; a reservation that is not fulfilled within
    /// <paramref name="reservationTtlMs"/> frees its slot again.</summary>
    public Matchmaker(RoomRegistry registry, int capacity, long reservationTtlMs = 15_000, Func<long>? nowMs = null)
    {
        _registry = registry;
        _capacity = capacity;
        _reservationTtlMs = reservationTtlMs;
        _nowMs = nowMs ?? (() => Environment.TickCount64);
    }

    /// <summary>The number of currently-reserved (assigned, awaiting join) slots —
    /// for observability/tests.</summary>
    public int OutstandingReservations
    {
        get { lock (_gate) { PruneExpired(); return _reservations.Count; } }
    }

    /// <summary>Assign a ticket to a room with a free slot (an existing one if any,
    /// else a fresh id), reserving that slot. Returns the room id to JoinRoom.</summary>
    public string FindOrCreate()
    {
        lock (_gate)
        {
            PruneExpired();

            // Consider every room we know about — live (from the registry) or merely
            // reserved — in a deterministic order, and take the first with room.
            var candidates = new SortedSet<string>(StringComparer.Ordinal);
            candidates.UnionWith(_registry.RoomIds());
            foreach (Reservation r in _reservations) candidates.Add(r.RoomId);

            foreach (string roomId in candidates)
            {
                int live = _registry.PeekRoom(roomId)?.ClientCount ?? 0;
                int reserved = _reservations.Count(r => r.RoomId == roomId);
                if (live + reserved < _capacity)
                {
                    Reserve(roomId);
                    return roomId;
                }
            }

            string fresh = $"mm-{++_counter}";
            Reserve(fresh);
            return fresh;
        }
    }

    private void Reserve(string roomId) =>
        _reservations.Add(new Reservation(roomId, _nowMs() + _reservationTtlMs));

    private void PruneExpired()
    {
        long now = _nowMs();
        _reservations.RemoveAll(r => r.ExpiresAtMs <= now);
    }
}
