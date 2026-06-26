using Axiom.Netplay;
using Xunit;

namespace Axiom.Netplay.Tests;

/// <summary>Phase 5 — the .NET worker adapter over the native FFI surface. These
/// drive <see cref="AxiomWorker"/> / <see cref="AxiomSim"/> directly (synchronous,
/// no transport) and prove the marshalling, status→typed-result conversion, and
/// safety of the boundary.</summary>
public sealed class AdapterTests
{
    private static AxiomWorker Worker() => new();

    [Fact]
    public void version_handshake_succeeds()
    {
        var worker = Worker();
        Assert.Equal(AxiomWorker.ExpectedProtocolVersion, worker.Version.ProtocolVersion);
        Assert.True(worker.IsCompatible);
        worker.CheckCompatibility(); // must not throw
    }

    [Fact]
    public void version_mismatch_refuses_startup()
    {
        // The host refuses to proceed when the native worker reports a different
        // protocol version (tested through the pure, deterministic check).
        Assert.False(AxiomWorker.IsProtocolCompatible(reported: 2, expected: 1));
        Assert.Throws<InvalidOperationException>(() => AxiomWorker.EnsureCompatible(reported: 2, expected: 1));
    }

    [Fact]
    public void create_destroy_sim_succeeds()
    {
        var worker = Worker();
        using var sim = worker.CreateSim(seed: 1, maxPlayers: 2, fixedStepNs: 16_666_667);
        Assert.Equal(2u, sim.MaxPlayers);
    }

    [Fact]
    public void create_rejects_invalid_arguments()
    {
        var worker = Worker();
        Assert.Throws<AxiomWorkerException>(() => worker.CreateSim(seed: 1, maxPlayers: 0, fixedStepNs: 16_666_667));
    }

    [Fact]
    public void submit_duplicate_sequence_returns_reject()
    {
        var worker = Worker();
        using var sim = worker.CreateSim(1, 2, 16_666_667);
        byte[] move = MovePayload(0.2f, 0f);

        Assert.True(sim.SubmitIntent(0, 1, 0, move).Accepted);
        sim.AdvanceTick(1);

        IntentOutcome second = sim.SubmitIntent(0, 1, 0, move);
        Assert.False(second.Accepted);
        Assert.Equal(AxiomRejectReason.DuplicateSequence, second.Reason);
    }

    [Fact]
    public void advance_tick_returns_hash()
    {
        var worker = Worker();
        using var sim = worker.CreateSim(1, 2, 16_666_667);
        sim.SubmitIntent(0, 1, 0, MovePayload(0.5f, 0f));
        TickOutcome outcome = sim.AdvanceTick(1);
        Assert.Equal(1ul, outcome.Tick);
        Assert.NotEqual(0ul, outcome.StateHash);
    }

    [Fact]
    public void get_snapshot_returns_bytes()
    {
        var worker = Worker();
        using var sim = worker.CreateSim(1, 2, 16_666_667);
        (byte[] snapshot, ulong hash) = sim.GetSnapshot();
        Assert.NotEmpty(snapshot);
        Assert.Equal(sim.GetStateHash(), hash);
    }

    [Fact]
    public void get_render_view_returns_authoritative_positions()
    {
        var worker = Worker();
        using var sim = worker.CreateSim(1, 2, 16_666_667);
        float[] view = sim.GetRenderView();
        Assert.Equal(4, view.Length);
        Assert.Equal(-1.5f, view[0]); // p0 spawns left
        Assert.Equal(1.5f, view[2]);  // p1 spawns right
        // The view tracks authoritative movement.
        sim.SubmitIntent(0, 1, 0, MovePayload(0.5f, 0f));
        sim.AdvanceTick(1);
        Assert.Equal(-1.0f, sim.GetRenderView()[0]);
    }

    [Fact]
    public void invalid_payload_does_not_crash()
    {
        var worker = Worker();
        using var sim = worker.CreateSim(1, 2, 16_666_667);
        // A short payload is rejected as malformed — never a crash.
        IntentOutcome outcome = sim.SubmitIntent(0, 1, 0, new byte[] { 0x01, 0x02 });
        Assert.False(outcome.Accepted);
        Assert.Equal(AxiomRejectReason.Malformed, outcome.Reason);
        // The sim is still usable.
        Assert.True(sim.SubmitIntent(0, 1, 0, MovePayload(0.1f, 0f)).Accepted);
    }

    [Fact]
    public void native_error_is_surfaced()
    {
        var worker = Worker();
        using var sim = worker.CreateSim(1, 2, 16_666_667);
        // Garbage snapshot bytes → a typed worker exception carrying the status.
        var ex = Assert.Throws<AxiomWorkerException>(() => sim.LoadState(new byte[] { 9, 9, 9 }));
        Assert.Equal(AxiomStatus.Deserialize, ex.Status);
    }

    [Fact]
    public void export_then_verify_round_trips()
    {
        var worker = Worker();
        using var sim = worker.CreateSim(3, 2, 16_666_667);
        sim.SubmitIntent(0, 1, 0, MovePayload(0.4f, 0f));
        TickOutcome tick = sim.AdvanceTick(1);

        byte[] replay = sim.ExportReplay();
        ReplayVerifyOutcome verify = worker.VerifyReplay(replay);
        Assert.True(verify.Matched);
        Assert.Equal(tick.StateHash, verify.FinalHash);
    }

    private static byte[] MovePayload(float dx, float dy)
    {
        var p = new byte[8];
        System.Buffers.Binary.BinaryPrimitives.WriteSingleLittleEndian(p.AsSpan(0, 4), dx);
        System.Buffers.Binary.BinaryPrimitives.WriteSingleLittleEndian(p.AsSpan(4, 4), dy);
        return p;
    }
}
