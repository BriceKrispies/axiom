namespace Axiom.Netplay;

/// <summary>
/// Liveness/readiness for the in-process worker. Readiness requires: the native
/// library loaded, the version handshake passed, and a create/advance/destroy
/// self-test succeeded. Because the worker is in-process, liveness is the process
/// itself; readiness is what proves the native worker actually runs here.
/// </summary>
public sealed class WorkerHealth
{
    private readonly AxiomWorker _worker;
    private bool _selfTestPassed;

    /// <summary>Build over a constructed worker (constructing it already loaded the
    /// native library — a missing library throws before this point).</summary>
    public WorkerHealth(AxiomWorker worker) => _worker = worker;

    /// <summary>The native library is loaded (the worker exists and reported a version).</summary>
    public bool LibraryLoaded => _worker.Version.ProtocolVersion != 0 || _worker.Version.Major != 0 || _worker.Version.Minor != 0;

    /// <summary>The worker's Tier-B protocol version matches the host's expectation.</summary>
    public bool VersionHandshakePassed => _worker.IsCompatible;

    /// <summary>Whether the boot self-test passed.</summary>
    public bool SelfTestPassed => _selfTestPassed;

    /// <summary>Run (or re-run) the create/advance/destroy self-test.</summary>
    public void RunSelfTest() => _selfTestPassed = _worker.SelfTest();

    /// <summary>Pure readiness rule (testable without a native mismatch): ready iff
    /// the version handshake passed and the self-test is green.</summary>
    public static bool IsReady(bool versionHandshakePassed, bool selfTestPassed) =>
        versionHandshakePassed && selfTestPassed;

    /// <summary>Fully ready to serve: handshake passed and self-test green.</summary>
    public bool Ready => IsReady(VersionHandshakePassed, _selfTestPassed);

    /// <summary>A readiness report for the endpoint.</summary>
    public IReadOnlyDictionary<string, object> Report() => new Dictionary<string, object>
    {
        ["libraryLoaded"] = LibraryLoaded,
        ["versionHandshakePassed"] = VersionHandshakePassed,
        ["selfTestPassed"] = SelfTestPassed,
        ["ready"] = Ready,
        ["workerVersion"] = _worker.Version.ToString(),
        ["expectedProtocolVersion"] = AxiomWorker.ExpectedProtocolVersion,
    };
}
