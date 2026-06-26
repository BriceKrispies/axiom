using Axiom.Netplay;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace Axiom.Netplay.Tests;

/// <summary>Phase B — quickplay matchmaking. A ticket is handed a room with a free
/// slot, filling rooms compactly and minting fresh ids only when all are full.
/// Driven directly (no transport) for deterministic, fast assertions.</summary>
public sealed class MatchmakerTests
{
    private static RoomRegistry NewRegistry(uint players = 2)
    {
        var worker = new AxiomWorker();
        var metrics = new WorkerMetrics();
        return new RoomRegistry(worker, metrics, NullLogger.Instance, seed: 1, maxPlayers: players, CancellationToken.None);
    }

    [Fact]
    public void quickplay_fills_rooms_compactly_then_opens_new_ones()
    {
        using var registry = NewRegistry(players: 2);
        var mm = new Matchmaker(registry, capacity: 2);

        // Five tickets into 2-player rooms must produce sizes [2, 2, 1]: the first
        // two share a room, the next two share another, the fifth opens a third.
        string[] assignments = Enumerable.Range(0, 5).Select(_ => mm.FindOrCreate()).ToArray();

        var sizes = assignments
            .GroupBy(id => id)
            .Select(g => g.Count())
            .OrderByDescending(n => n)
            .ToArray();
        Assert.Equal(new[] { 2, 2, 1 }, sizes);
        Assert.Equal(3, assignments.Distinct().Count());
    }

    [Fact]
    public void a_full_room_is_skipped_in_favor_of_a_new_one()
    {
        using var registry = NewRegistry(players: 2);
        var mm = new Matchmaker(registry, capacity: 2);

        string first = mm.FindOrCreate();
        string second = mm.FindOrCreate();
        Assert.Equal(first, second); // both fill the first room

        string third = mm.FindOrCreate();
        Assert.NotEqual(first, third); // the full room is skipped
    }

    [Fact]
    public void reservations_expire_and_free_their_slots()
    {
        using var registry = NewRegistry(players: 2);
        long now = 0;
        // Inject a controllable clock so expiry is deterministic (no wall-clock).
        var mm = new Matchmaker(registry, capacity: 2, reservationTtlMs: 100, nowMs: () => now);

        // Two tickets fill a room by reservation; a third (room full) opens a new one.
        string r1 = mm.FindOrCreate();
        string r2 = mm.FindOrCreate();
        Assert.Equal(r1, r2);
        Assert.Equal(2, mm.OutstandingReservations);
        Assert.NotEqual(r1, mm.FindOrCreate());

        // Past the TTL, the unfulfilled reservations are pruned: their slots free.
        now = 1_000;
        Assert.Equal(0, mm.OutstandingReservations);
    }
}
