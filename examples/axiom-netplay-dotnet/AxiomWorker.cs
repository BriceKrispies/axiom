using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace Axiom.Netplay;

/// <summary>
/// The typed, lifetime-safe facade over the Axiom simulation worker. It performs
/// the version handshake, creates sim instances, and offers the stateless replay
/// verification. A sim instance is an <see cref="IAxiomSim"/>: in-process via
/// P/Invoke (<see cref="AxiomSim"/>) by default, or out-of-process over a socket
/// (<see cref="OutOfProcAxiomSim"/>) when <c>AXIOM_WORKER_MODE=outproc</c>.
/// </summary>
public sealed class AxiomWorker
{
    /// <summary>The Tier-B protocol version this host build was written against.
    /// Startup refuses to proceed if the native worker disagrees.</summary>
    public const uint ExpectedProtocolVersion = 1;

    /// <summary>The worker's reported version, read once at construction.</summary>
    public AxiomWorkerVersion Version { get; }

    private readonly ILogger _logger;
    private readonly bool _outOfProcess;

    /// <summary>Whether new sims run out-of-process (a separate worker per sim).</summary>
    public bool OutOfProcess => _outOfProcess;

    /// <summary>Load the native worker and read its version. Does not yet enforce
    /// compatibility — call <see cref="CheckCompatibility"/> for that. The worker
    /// MODE (in-process vs a spawned process per sim) is read from
    /// <c>AXIOM_WORKER_MODE</c> (<c>inproc</c> default, or <c>outproc</c>).</summary>
    public AxiomWorker(ILogger? logger = null)
    {
        _logger = logger ?? NullLogger.Instance;
        _outOfProcess = string.Equals(
            Environment.GetEnvironmentVariable("AXIOM_WORKER_MODE"), "outproc", StringComparison.OrdinalIgnoreCase);

        AxiomWorkerNative.EnsureLoaded();
        Version = new AxiomWorkerVersion(
            AxiomWorkerNative.axiom_worker_version_major(),
            AxiomWorkerNative.axiom_worker_version_minor(),
            AxiomWorkerNative.axiom_worker_version_patch(),
            AxiomWorkerNative.axiom_worker_protocol_version());
    }

    /// <summary>True when the worker's protocol version matches the host's expectation.</summary>
    public bool IsCompatible => Version.ProtocolVersion == ExpectedProtocolVersion;

    /// <summary>Whether a reported protocol version is compatible with an expected
    /// one. Pure (no native call) so both branches are unit-testable.</summary>
    public static bool IsProtocolCompatible(uint reported, uint expected) => reported == expected;

    /// <summary>Throw unless <paramref name="reported"/> matches <paramref name="expected"/>.</summary>
    public static void EnsureCompatible(uint reported, uint expected)
    {
        if (!IsProtocolCompatible(reported, expected))
            throw new InvalidOperationException(
                $"Axiom worker protocol mismatch: host expects {expected}, native worker reports " +
                $"{reported}. Rebuild the FFI library (`cargo build -p axiom-netplay-ffi --release`).");
    }

    /// <summary>Throw unless the worker's protocol version matches. Called at
    /// startup so an ABI mismatch fails fast instead of corrupting a room.</summary>
    public void CheckCompatibility() => EnsureCompatible(Version.ProtocolVersion, ExpectedProtocolVersion);

    /// <summary>Create an authoritative sim instance — in-process or, when the
    /// worker mode is <c>outproc</c>, as a spawned worker process talking over a
    /// local socket. Either way it is the same authority behind <see cref="IAxiomSim"/>.</summary>
    /// <exception cref="AxiomWorkerException">If the worker refused the arguments.</exception>
    public IAxiomSim CreateSim(ulong seed, uint maxPlayers, ulong fixedStepNs)
    {
        if (_outOfProcess)
            return new OutOfProcAxiomSim(OutOfProcAxiomSim.ResolveWorkerExe(), seed, maxPlayers, fixedStepNs, _logger);

        var handle = AxiomSimHandle.Create(seed, maxPlayers, fixedStepNs);
        if (handle.IsInvalid)
        {
            handle.Dispose();
            throw new AxiomWorkerException("create", AxiomStatus.InvalidArg, 0, "axiom_sim_create returned null");
        }
        return new AxiomSim(handle, maxPlayers);
    }

    /// <summary>Verify a replay record from tick zero (stateless — builds a fresh
    /// worker from the record's seed and compares per-tick hashes).</summary>
    public ReplayVerifyOutcome VerifyReplay(byte[] replayBytes)
    {
        int status = AxiomWorkerNative.axiom_sim_verify_replay(
            replayBytes, (nuint)replayBytes.Length, out uint matched, out ulong firstDivergence, out ulong finalHash);
        if (status != (int)AxiomStatus.Ok)
            throw new AxiomWorkerException("verify_replay", (AxiomStatus)status, 0, "");
        return new ReplayVerifyOutcome(matched == 1, firstDivergence, finalHash);
    }

    /// <summary>A boot self-test: create a sim, advance a tick, destroy it. Used by
    /// the readiness check to prove the native worker actually runs in this process.</summary>
    public bool SelfTest()
    {
        try
        {
            using var sim = CreateSim(seed: 1, maxPlayers: 1, fixedStepNs: 16_666_667);
            sim.AdvanceTick(1);
            return true;
        }
        catch (AxiomWorkerException)
        {
            return false;
        }
    }
}

/// <summary>
/// One authoritative simulation instance. Every call is serialized on a per-sim
/// lock, marshals buffers safely, handles buffer-too-small by querying the length
/// then writing, and converts native status codes into typed results. A native
/// rejection is an <see cref="IntentOutcome"/>, never an exception.
/// </summary>
public sealed class AxiomSim : IAxiomSim
{
    private readonly AxiomSimHandle _handle;
    private readonly object _gate = new();

    /// <summary>The maximum player count this sim was created with.</summary>
    public uint MaxPlayers { get; }

    internal AxiomSim(AxiomSimHandle handle, uint maxPlayers)
    {
        _handle = handle;
        MaxPlayers = maxPlayers;
    }

    /// <summary>Restore authoritative state from snapshot bytes (the "load room state").</summary>
    public void LoadState(byte[] snapshotBytes)
    {
        lock (_gate)
        {
            int status = AxiomWorkerNative.axiom_sim_load_state(_handle.Pointer, snapshotBytes, (nuint)snapshotBytes.Length);
            ThrowIfError("load_state", status);
        }
    }

    /// <summary>Submit a host-assigned player's intent. The reason is returned, not
    /// thrown — a rejection is a normal outcome.</summary>
    public IntentOutcome SubmitIntent(uint playerId, ulong clientSequence, ulong predictedClientTick, byte[] payload)
    {
        lock (_gate)
        {
            int status = AxiomWorkerNative.axiom_sim_submit_intent(
                _handle.Pointer, playerId, clientSequence, predictedClientTick,
                payload, (nuint)payload.Length, out uint reason);
            if (status == (int)AxiomStatus.Ok)
                return new IntentOutcome(true, AxiomRejectReason.None);
            if (status == (int)AxiomStatus.Rejected)
                return new IntentOutcome(false, (AxiomRejectReason)reason);
            ThrowIfError("submit_intent", status);
            return new IntentOutcome(false, AxiomRejectReason.None); // unreachable
        }
    }

    /// <summary>Advance exactly one authoritative tick.</summary>
    public TickOutcome AdvanceTick(ulong targetTick)
    {
        lock (_gate)
        {
            int status = AxiomWorkerNative.axiom_sim_advance_tick(_handle.Pointer, targetTick, out ulong tick, out ulong hash);
            ThrowIfError("advance_tick", status);
            return new TickOutcome(tick, hash);
        }
    }

    /// <summary>The current authoritative snapshot bytes plus their state hash.</summary>
    public (byte[] Snapshot, ulong StateHash) GetSnapshot()
    {
        lock (_gate)
        {
            int lenStatus = AxiomWorkerNative.axiom_sim_snapshot_len(_handle.Pointer, out nuint len);
            ThrowIfError("snapshot_len", lenStatus);
            var buffer = new byte[(int)len];
            int writeStatus = AxiomWorkerNative.axiom_sim_snapshot_write(
                _handle.Pointer, buffer, (nuint)buffer.Length, out nuint written, out ulong hash);
            ThrowIfError("snapshot_write", writeStatus);
            return (TrimTo(buffer, written), hash);
        }
    }

    /// <summary>The current authoritative state hash.</summary>
    public ulong GetStateHash()
    {
        lock (_gate)
        {
            int status = AxiomWorkerNative.axiom_sim_state_hash(_handle.Pointer, out ulong hash);
            ThrowIfError("state_hash", status);
            return hash;
        }
    }

    /// <summary>The authoritative render view: each player's <c>(x, y)</c> world
    /// position (<c>2 * MaxPlayers</c> floats), a read-only projection of
    /// authoritative state. The host broadcasts this so browser clients render and
    /// reconcile against authoritative positions — clients never send positions.</summary>
    public float[] GetRenderView()
    {
        lock (_gate)
        {
            var buffer = new float[2 * (int)MaxPlayers];
            int status = AxiomWorkerNative.axiom_sim_render_view_write(
                _handle.Pointer, buffer, (nuint)buffer.Length, out nuint count);
            ThrowIfError("render_view", status);
            return (int)count == buffer.Length ? buffer : buffer.AsSpan(0, (int)count).ToArray();
        }
    }

    /// <summary>Export the deterministic replay record's canonical bytes.</summary>
    public byte[] ExportReplay()
    {
        lock (_gate)
        {
            int lenStatus = AxiomWorkerNative.axiom_sim_export_replay_len(_handle.Pointer, out nuint len);
            ThrowIfError("export_replay_len", lenStatus);
            var buffer = new byte[(int)len];
            int writeStatus = AxiomWorkerNative.axiom_sim_export_replay_write(
                _handle.Pointer, buffer, (nuint)buffer.Length, out nuint written);
            ThrowIfError("export_replay_write", writeStatus);
            return TrimTo(buffer, written);
        }
    }

    /// <inheritdoc/>
    public void Dispose() => _handle.Dispose();

    private void ThrowIfError(string op, int status)
    {
        if (status == (int)AxiomStatus.Ok) return;
        uint code = AxiomWorkerNative.axiom_sim_last_error_code(_handle.Pointer);
        throw new AxiomWorkerException(op, (AxiomStatus)status, code, ReadLastErrorMessage());
    }

    private string ReadLastErrorMessage()
    {
        var buffer = new byte[256];
        int status = AxiomWorkerNative.axiom_sim_last_error_message_write(
            _handle.Pointer, buffer, (nuint)buffer.Length, out nuint written);
        return status == (int)AxiomStatus.Ok ? System.Text.Encoding.UTF8.GetString(buffer, 0, (int)written) : "";
    }

    private static byte[] TrimTo(byte[] buffer, nuint written) =>
        (int)written == buffer.Length ? buffer : buffer.AsSpan(0, (int)written).ToArray();
}
