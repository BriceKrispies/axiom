namespace Axiom.Netplay;

/// <summary>
/// One authoritative simulation instance, independent of WHERE it runs. The
/// in-process implementation (<see cref="AxiomSim"/>) drives the native engine via
/// P/Invoke; the out-of-process implementation (<see cref="OutOfProcAxiomSim"/>)
/// drives a separate worker process over a local socket. A <see cref="Room"/> is
/// written against this interface, so the two are interchangeable — selected by
/// <c>AXIOM_WORKER_MODE</c> — and the authoritative behaviour is identical either
/// way (proven by parity tests).
/// </summary>
public interface IAxiomSim : IDisposable
{
    /// <summary>The maximum player count this sim was created with.</summary>
    uint MaxPlayers { get; }

    /// <summary>Submit a host-assigned player's intent. A rejection is a normal
    /// outcome, not an exception.</summary>
    IntentOutcome SubmitIntent(uint playerId, ulong clientSequence, ulong predictedClientTick, byte[] payload);

    /// <summary>Advance exactly one authoritative tick.</summary>
    TickOutcome AdvanceTick(ulong targetTick);

    /// <summary>The authoritative render view: each player's (x, y) world position.</summary>
    float[] GetRenderView();

    /// <summary>The current authoritative snapshot bytes plus their state hash.</summary>
    (byte[] Snapshot, ulong StateHash) GetSnapshot();

    /// <summary>Restore authoritative state from snapshot bytes.</summary>
    void LoadState(byte[] snapshotBytes);

    /// <summary>The current authoritative state hash.</summary>
    ulong GetStateHash();

    /// <summary>Export the deterministic replay record's canonical bytes.</summary>
    byte[] ExportReplay();
}
