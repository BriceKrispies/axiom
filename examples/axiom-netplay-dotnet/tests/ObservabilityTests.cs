using Axiom.Netplay;
using Xunit;

namespace Axiom.Netplay.Tests;

/// <summary>Phase 10 — observability and operational hardening: readiness gating
/// on the version handshake + self-test, and the metric counters.</summary>
public sealed class ObservabilityTests
{
    [Fact]
    public void readiness_fails_when_worker_version_mismatch()
    {
        // A failed version handshake must make the worker not-ready even if the
        // self-test would pass (pure rule, no native mismatch needed).
        Assert.False(WorkerHealth.IsReady(versionHandshakePassed: false, selfTestPassed: true));
        Assert.False(AxiomWorker.IsProtocolCompatible(reported: 99, expected: AxiomWorker.ExpectedProtocolVersion));
    }

    [Fact]
    public void readiness_succeeds_after_worker_self_test()
    {
        var worker = new AxiomWorker();
        var health = new WorkerHealth(worker);
        Assert.False(health.Ready); // self-test not run yet
        health.RunSelfTest();
        Assert.True(health.SelfTestPassed);
        Assert.True(health.Ready);
    }

    [Fact]
    public void worker_error_increments_error_metric()
    {
        var metrics = new WorkerMetrics();
        Assert.Equal(0, metrics.WorkerErrors);
        metrics.WorkerError();
        Assert.Equal(1, metrics.WorkerErrors);
    }

    [Fact]
    public void rejected_intent_increments_reason_counter()
    {
        var metrics = new WorkerMetrics();
        metrics.Rejected(AxiomRejectReason.OutOfOrder);
        metrics.Rejected(AxiomRejectReason.OutOfOrder);
        metrics.Rejected(AxiomRejectReason.DuplicateSequence);
        Assert.Equal(2, metrics.RejectCount(AxiomRejectReason.OutOfOrder));
        Assert.Equal(1, metrics.RejectCount(AxiomRejectReason.DuplicateSequence));
        Assert.Equal(3, metrics.RejectedInputs);
    }

    [Fact]
    public void tick_metrics_are_recorded_after_an_advance()
    {
        var worker = new AxiomWorker();
        var metrics = new WorkerMetrics();
        using var room = new Room(worker, seed: 1, maxPlayers: 2, metrics);
        room.TickOnce();
        Assert.True(metrics.LastSnapshotSize > 0);
        Assert.NotEqual(0ul, metrics.LastStateHash);
    }
}
