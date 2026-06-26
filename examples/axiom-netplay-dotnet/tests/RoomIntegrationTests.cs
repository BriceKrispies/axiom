using System.Diagnostics;
using Axiom.Netplay;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace Axiom.Netplay.Tests;

/// <summary>Phase 6 — the single-room authoritative loop, driven over an in-memory
/// transport. The browser can only ever send JoinRoom / LeaveRoom / ClientIntent;
/// these prove it can never submit state, and that bad/duplicate/out-of-order
/// frames are rejected while the room keeps ticking.</summary>
public sealed class RoomIntegrationTests
{
    private const int KindWelcome = 3;

    private static (AxiomWorker Worker, WorkerMetrics Metrics, Room Room) NewRoom(uint players = 2)
    {
        var worker = new AxiomWorker();
        var metrics = new WorkerMetrics();
        var room = new Room(worker, seed: 1, maxPlayers: players, metrics);
        return (worker, metrics, room);
    }

    private static async Task JoinAsync(Room room, TestClient client, List<Task> sessions, CancellationToken ct)
    {
        // The registry consumes the JoinRoom frame and routes by its room id; here
        // we drive Room directly (white-box), so we build that frame and hand it in.
        byte[] join = ClientCodec.EncodeJoinRoom();
        sessions.Add(room.RunSessionAsync(join, client.Read, client.Send, "test", NullLogger.Instance, ct));
        byte[] welcome = await client.NextServerFrame(ct);
        Assert.Equal(KindWelcome, ClientCodec.MessageKind(welcome));
    }

    private static RoomRegistry NewRegistry(uint players = 2)
    {
        var worker = new AxiomWorker();
        var metrics = new WorkerMetrics();
        return new RoomRegistry(worker, metrics, NullLogger.Instance, seed: 1, maxPlayers: players, CancellationToken.None);
    }

    /// Connect a client to a room THROUGH the registry (it reads the JoinRoom frame
    /// off the wire and routes by id), then await its Welcome.
    private static async Task JoinViaRegistryAsync(RoomRegistry registry, TestClient client, string roomId, List<Task> sessions, CancellationToken ct)
    {
        sessions.Add(registry.ServeAsync(client.Read, client.Send, "test", ct));
        client.ClientSend(ClientCodec.EncodeJoinRoom(roomId: roomId));
        byte[] welcome = await client.NextServerFrame(ct);
        Assert.Equal(KindWelcome, ClientCodec.MessageKind(welcome));
    }

    private static async Task<bool> WaitUntilAsync(Func<bool> condition, int timeoutMs = 4000)
    {
        var sw = Stopwatch.StartNew();
        while (sw.ElapsedMilliseconds < timeoutMs)
        {
            if (condition()) return true;
            await Task.Delay(15);
        }
        return condition();
    }

    [Fact]
    public async Task two_clients_join_and_receive_consistent_snapshots()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        var (_, _, room) = NewRoom();
        using var _room = room;
        var sessions = new List<Task>();
        var c1 = new TestClient();
        var c2 = new TestClient();

        await JoinAsync(room, c1, sessions, cts.Token);
        await JoinAsync(room, c2, sessions, cts.Token);
        Assert.True(await WaitUntilAsync(() => room.ClientCount == 2));

        room.TickOnce();
        var broadcast = room.BuildBroadcast();

        Assert.Equal(2, broadcast.Count);
        // Neither client sent an intent, so both authoritative snapshot frames are
        // byte-identical (same tick, same state, both last-accepted 0).
        Assert.Equal(broadcast[0].Frame, broadcast[1].Frame);

        c1.Disconnect();
        c2.Disconnect();
        cts.Cancel();
    }

    [Fact]
    public async Task client_cannot_submit_state_mutation()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        var (_, metrics, room) = NewRoom();
        using var _room = room;
        var sessions = new List<Task>();
        var c1 = new TestClient();
        await JoinAsync(room, c1, sessions, cts.Token);

        room.TickOnce();
        ulong before = room.StateHash;

        // The client forges an authoritative ServerSnapshot to inject state.
        byte[] forged = ClientCodec.EncodeServerSnapshot(serverTick: 999, lastAccepted: 0, payload: new byte[] { 1, 2, 3, 4 });
        c1.ClientSend(forged);

        Assert.True(await WaitUntilAsync(() => metrics.MalformedFrames >= 1));
        room.TickOnce();
        // The forged state was never applied — the authoritative hash is unchanged.
        Assert.Equal(before, room.StateHash);

        c1.Disconnect();
        cts.Cancel();
    }

    [Fact]
    public async Task duplicate_intent_is_rejected()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        var (_, metrics, room) = NewRoom();
        using var _room = room;
        var sessions = new List<Task>();
        var c1 = new TestClient();
        await JoinAsync(room, c1, sessions, cts.Token);

        c1.ClientSend(ClientCodec.EncodeMoveIntent(1, 0.2f, 0f));
        Assert.True(await WaitUntilAsync(() => metrics.AcceptedInputs >= 1));
        c1.ClientSend(ClientCodec.EncodeMoveIntent(1, 0.2f, 0f)); // duplicate sequence
        Assert.True(await WaitUntilAsync(() => metrics.RejectCount(AxiomRejectReason.DuplicateSequence) >= 1));

        c1.Disconnect();
        cts.Cancel();
    }

    [Fact]
    public async Task out_of_order_intent_is_rejected()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        var (_, metrics, room) = NewRoom();
        using var _room = room;
        var sessions = new List<Task>();
        var c1 = new TestClient();
        await JoinAsync(room, c1, sessions, cts.Token);

        c1.ClientSend(ClientCodec.EncodeMoveIntent(5, 0.2f, 0f));
        Assert.True(await WaitUntilAsync(() => metrics.AcceptedInputs >= 1));
        c1.ClientSend(ClientCodec.EncodeMoveIntent(4, 0.2f, 0f)); // older sequence
        Assert.True(await WaitUntilAsync(() => metrics.RejectCount(AxiomRejectReason.OutOfOrder) >= 1));

        c1.Disconnect();
        cts.Cancel();
    }

    [Fact]
    public async Task malformed_frame_is_rejected()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        var (_, metrics, room) = NewRoom();
        using var _room = room;
        var sessions = new List<Task>();
        var c1 = new TestClient();
        await JoinAsync(room, c1, sessions, cts.Token);

        c1.ClientSend(new byte[] { 0xFF, 0xFF, 0xFF }); // not a valid frame at all
        Assert.True(await WaitUntilAsync(() => metrics.MalformedFrames >= 1));

        c1.Disconnect();
        cts.Cancel();
    }

    [Fact]
    public async Task disconnected_client_does_not_stop_room_tick()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        var (_, _, room) = NewRoom();
        using var _room = room;
        var sessions = new List<Task>();
        var c1 = new TestClient();
        var c2 = new TestClient();
        await JoinAsync(room, c1, sessions, cts.Token);
        await JoinAsync(room, c2, sessions, cts.Token);
        Assert.True(await WaitUntilAsync(() => room.ClientCount == 2));

        c1.Disconnect();
        Assert.True(await WaitUntilAsync(() => room.ClientCount == 1));

        ulong before = room.Tick;
        room.TickOnce();
        Assert.Equal(before + 1, room.Tick);

        c2.Disconnect();
        cts.Cancel();
    }

    [Fact]
    public void room_tick_continues_without_input()
    {
        var (_, _, room) = NewRoom();
        using var _room = room;
        room.TickOnce();
        room.TickOnce();
        room.TickOnce();
        Assert.Equal(3ul, room.Tick);
    }

    [Fact]
    public async Task rooms_are_routed_and_isolated_by_id()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        using var registry = NewRegistry();
        var sessions = new List<Task>();

        // Two players land in room "a", two in room "b" — purely by the room id in
        // their JoinRoom frame.
        var a1 = new TestClient();
        var a2 = new TestClient();
        var b1 = new TestClient();
        var b2 = new TestClient();
        await JoinViaRegistryAsync(registry, a1, "a", sessions, cts.Token);
        await JoinViaRegistryAsync(registry, a2, "a", sessions, cts.Token);
        await JoinViaRegistryAsync(registry, b1, "b", sessions, cts.Token);
        await JoinViaRegistryAsync(registry, b2, "b", sessions, cts.Token);

        Assert.True(await WaitUntilAsync(() => registry.RoomCount == 2));
        Room roomA = registry.PeekRoom("a")!;
        Room roomB = registry.PeekRoom("b")!;
        Assert.NotNull(roomA);
        Assert.NotNull(roomB);
        Assert.True(await WaitUntilAsync(() => roomA.ClientCount == 2 && roomB.ClientCount == 2));

        // The two rooms are independent deterministic sims: same seed, so once
        // both have ticked at least once they are byte-identical (a no-input tick
        // preserves the hash, so this value is stable until an intent lands).
        Assert.True(await WaitUntilAsync(() => roomA.StateHash != 0 && roomB.StateHash != 0));
        ulong bInitial = roomB.StateHash;
        Assert.Equal(bInitial, roomA.StateHash);

        // A move by a player in room "a" changes ONLY room "a"'s authoritative
        // state. Room "b" never moves — an intent cannot cross a room boundary.
        a1.ClientSend(ClientCodec.EncodeMoveIntent(1, 0.5f, 0f));
        Assert.True(await WaitUntilAsync(() => roomA.StateHash != bInitial));
        Assert.Equal(bInitial, roomB.StateHash);

        a1.Disconnect(); a2.Disconnect(); b1.Disconnect(); b2.Disconnect();
        cts.Cancel();
    }

    [Fact]
    public async Task room_is_created_on_demand_and_reaped_when_empty()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        using var registry = NewRegistry();
        var sessions = new List<Task>();
        Assert.Equal(0, registry.RoomCount);

        var solo = new TestClient();
        await JoinViaRegistryAsync(registry, solo, "solo", sessions, cts.Token);
        Assert.True(await WaitUntilAsync(() => registry.RoomCount == 1));

        // The last client leaving reaps the room (its loop is cancelled, its sim
        // disposed) — empty rooms do not linger.
        solo.Disconnect();
        Assert.True(await WaitUntilAsync(() => registry.RoomCount == 0));
    }
}
