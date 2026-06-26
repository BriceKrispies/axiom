using System.Collections.Concurrent;

namespace Axiom.Netplay;

/// <summary>
/// Minimum structured observability for the authoritative loop. Counters are
/// thread-safe; the room loop and session loops update them, and the readiness /
/// metrics endpoints read a <see cref="Report"/>. No ad-hoc logging, no printing
/// from the engine — the engine never logs; this is the host's edge.
/// </summary>
public sealed class WorkerMetrics
{
    private long _acceptedInputs;
    private long _rejectedInputs;
    private long _malformedFrames;
    private long _disconnects;
    private long _workerErrors;
    private readonly ConcurrentDictionary<AxiomRejectReason, long> _rejectByReason = new();

    /// <summary>Tick duration of the last advance, measured in .NET (ms).</summary>
    public double LastTickMs { get; private set; }
    /// <summary>The last worker-call duration measured in .NET (ms).</summary>
    public double LastWorkerCallMs { get; private set; }
    /// <summary>How many ticks behind real time the room loop is.</summary>
    public long TickBacklog { get; set; }
    /// <summary>Size in bytes of the last authoritative snapshot.</summary>
    public int LastSnapshotSize { get; private set; }
    /// <summary>The last authoritative per-tick state hash.</summary>
    public ulong LastStateHash { get; private set; }
    /// <summary>The last replay verification result, if one was run.</summary>
    public bool? LastReplayVerified { get; set; }

    /// <summary>Record an accepted input.</summary>
    public void Accepted() => Interlocked.Increment(ref _acceptedInputs);

    /// <summary>Record a rejected input and its reason.</summary>
    public void Rejected(AxiomRejectReason reason)
    {
        Interlocked.Increment(ref _rejectedInputs);
        _rejectByReason.AddOrUpdate(reason, 1, (_, n) => n + 1);
    }

    /// <summary>Record a malformed/undecodable client frame.</summary>
    public void MalformedFrame() => Interlocked.Increment(ref _malformedFrames);

    /// <summary>Record a client disconnect.</summary>
    public void Disconnect() => Interlocked.Increment(ref _disconnects);

    /// <summary>Record a worker error/panic surfaced to the host.</summary>
    public void WorkerError() => Interlocked.Increment(ref _workerErrors);

    /// <summary>Record the timings and outputs of one advanced tick.</summary>
    public void RecordTick(double tickMs, double workerCallMs, int snapshotSize, ulong stateHash)
    {
        LastTickMs = tickMs;
        LastWorkerCallMs = workerCallMs;
        LastSnapshotSize = snapshotSize;
        LastStateHash = stateHash;
    }

    /// <summary>A point-in-time snapshot of the counters for an endpoint or log.</summary>
    public IReadOnlyDictionary<string, object> Report() => new Dictionary<string, object>
    {
        ["acceptedInputs"] = Interlocked.Read(ref _acceptedInputs),
        ["rejectedInputs"] = Interlocked.Read(ref _rejectedInputs),
        ["rejectByReason"] = _rejectByReason.ToDictionary(kv => kv.Key.ToString(), kv => kv.Value),
        ["malformedFrames"] = Interlocked.Read(ref _malformedFrames),
        ["disconnects"] = Interlocked.Read(ref _disconnects),
        ["workerErrors"] = Interlocked.Read(ref _workerErrors),
        ["lastTickMs"] = LastTickMs,
        ["lastWorkerCallMs"] = LastWorkerCallMs,
        ["tickBacklog"] = TickBacklog,
        ["lastSnapshotSize"] = LastSnapshotSize,
        ["lastStateHash"] = LastStateHash,
        ["lastReplayVerified"] = LastReplayVerified ?? false,
    };

    /// <summary>The accepted-input counter (for tests).</summary>
    public long AcceptedInputs => Interlocked.Read(ref _acceptedInputs);
    /// <summary>The rejected-input counter (for tests).</summary>
    public long RejectedInputs => Interlocked.Read(ref _rejectedInputs);
    /// <summary>The worker-error counter (for tests).</summary>
    public long WorkerErrors => Interlocked.Read(ref _workerErrors);
    /// <summary>The malformed-frame counter (for tests).</summary>
    public long MalformedFrames => Interlocked.Read(ref _malformedFrames);
    /// <summary>The disconnect counter (for tests).</summary>
    public long Disconnects => Interlocked.Read(ref _disconnects);
    /// <summary>The count of rejections for a specific reason (for tests).</summary>
    public long RejectCount(AxiomRejectReason reason) => _rejectByReason.TryGetValue(reason, out long n) ? n : 0;
}
