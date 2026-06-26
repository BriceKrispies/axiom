namespace Axiom.Netplay;

/// <summary>
/// Drives a <see cref="Room"/>: ticks the simulation at a fixed cadence and
/// broadcasts authoritative snapshots. The two loops are decoupled so simulated
/// network lag delays delivery without slowing the deterministic simulation —
/// the process owns wall-clock pacing, the worker owns the deterministic step.
/// </summary>
public sealed class RoomLoop
{
    // Simulated server lag (a demo aid): delayed, jittery snapshot delivery.
    private static readonly int LagBaseMs = EnvInt("AXIOM_LAG_MS", 90);
    private static readonly int LagJitterMs = EnvInt("AXIOM_JITTER_MS", 120);

    /// <summary>A human-readable description of the simulated lag.</summary>
    public static string LagSummary => $"{LagBaseMs}ms base + 0..{LagJitterMs}ms jitter";

    private readonly Room _room;
    private readonly WorkerMetrics _metrics;
    private readonly ILogger _logger;

    /// <summary>Create a loop over a room.</summary>
    public RoomLoop(Room room, WorkerMetrics metrics, ILogger logger)
    {
        _room = room;
        _metrics = metrics;
        _logger = logger;
    }

    /// <summary>Run both loops until cancellation.</summary>
    public Task RunAsync(CancellationToken ct) => Task.WhenAll(TickLoopAsync(ct), DeliverLoopAsync(ct));

    private async Task TickLoopAsync(CancellationToken ct)
    {
        using var timer = new PeriodicTimer(TimeSpan.FromMilliseconds(16));
        try
        {
            while (await timer.WaitForNextTickAsync(ct))
            {
                try { _room.TickOnce(); }
                catch (AxiomWorkerException ex)
                {
                    // A worker fault must not kill the loop: count it and carry on.
                    _metrics.WorkerError();
                    _logger.LogError(ex, "worker tick failed");
                }
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
                foreach ((ClientSession client, byte[] frame) in _room.BuildBroadcast())
                {
                    try { await Room.DeliverAsync(client, frame, ct); }
                    catch { /* a dropped client is reaped by its own session loop */ }
                }
                int lag = LagBaseMs + Random.Shared.Next(0, LagJitterMs + 1);
                await Task.Delay(lag, ct);
            }
        }
        catch (OperationCanceledException) { /* shutting down */ }
    }

    private static int EnvInt(string key, int fallback) =>
        int.TryParse(Environment.GetEnvironmentVariable(key), out int v) && v >= 0 ? v : fallback;
}
