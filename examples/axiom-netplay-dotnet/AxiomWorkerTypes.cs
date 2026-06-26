namespace Axiom.Netplay;

/// <summary>Status codes returned by every Tier-B worker-control call. Mirrors
/// the Rust <c>status::STATUS_*</c> constants.</summary>
public enum AxiomStatus
{
    /// <summary>The call succeeded.</summary>
    Ok = 0,
    /// <summary>The sim handle was null.</summary>
    NullHandle = 1,
    /// <summary>An argument was invalid.</summary>
    InvalidArg = 2,
    /// <summary>A caller buffer was too small; query the length and retry.</summary>
    BufferTooSmall = 3,
    /// <summary>An intent was rejected by validation (see the reason).</summary>
    Rejected = 4,
    /// <summary>Provided bytes failed to deserialize.</summary>
    Deserialize = 5,
    /// <summary>The engine returned a deterministic error.</summary>
    Engine = 6,
    /// <summary>The worker panicked and was caught at the boundary.</summary>
    Panic = 7,
}

/// <summary>Why the worker rejected an intent. Mirrors the Rust <c>REASON_*</c>
/// constants; 0..=3 also match <c>axiom-net-protocol</c>'s wire reason codes.</summary>
public enum AxiomRejectReason : uint
{
    /// <summary>Not a rejection — accepted.</summary>
    None = 0,
    /// <summary>Payload malformed / undecodable.</summary>
    Malformed = 1,
    /// <summary>Sequence arrived out of order (older than last accepted).</summary>
    OutOfOrder = 2,
    /// <summary>Player not a member of the room.</summary>
    NotInRoom = 3,
    /// <summary>Sequence duplicates the last accepted for that player.</summary>
    DuplicateSequence = 4,
    /// <summary>Payload exceeded the maximum length.</summary>
    PayloadTooLarge = 5,
    /// <summary>Player id outside [0, max_players).</summary>
    InvalidPlayer = 6,
    /// <summary>Too many intents for one player in a tick.</summary>
    RateLimited = 7,
    /// <summary>The implied move is illegal for the ruleset (e.g. a teleport).</summary>
    ImpossibleMovement = 8,
}

/// <summary>The outcome of submitting a player intent.</summary>
public readonly record struct IntentOutcome(bool Accepted, AxiomRejectReason Reason)
{
    /// <summary>The wire reason code to echo to the browser as a Tier-A
    /// <c>RejectedIntent</c> (the worker's 0..=3 already match the wire codes; a
    /// worker-specific reason maps to <see cref="AxiomRejectReason.Malformed"/>
    /// as the closest browser-meaningful cause).</summary>
    public uint WireReasonCode => (uint)Reason <= 3 ? (uint)Reason : (uint)AxiomRejectReason.Malformed;
}

/// <summary>The result of advancing one authoritative tick.</summary>
public readonly record struct TickOutcome(ulong Tick, ulong StateHash);

/// <summary>The result of verifying a replay record.</summary>
public readonly record struct ReplayVerifyOutcome(bool Matched, ulong FirstDivergenceTick, ulong FinalHash);

/// <summary>The worker's reported version, gathered at startup for the handshake.</summary>
public readonly record struct AxiomWorkerVersion(uint Major, uint Minor, uint Patch, uint ProtocolVersion)
{
    /// <inheritdoc/>
    public override string ToString() => $"{Major}.{Minor}.{Patch} (protocol {ProtocolVersion})";
}

/// <summary>Raised when a Tier-B worker call fails in a way the host cannot
/// proceed past (a null/invalid argument, a panic, or an engine error). Rejections
/// are NOT exceptions — they are ordinary <see cref="IntentOutcome"/> values.</summary>
public sealed class AxiomWorkerException : Exception
{
    /// <summary>The status code the native call returned.</summary>
    public AxiomStatus Status { get; }

    /// <summary>The worker's last-error code, if a handle was available.</summary>
    public uint NativeErrorCode { get; }

    /// <summary>Construct from a failing call.</summary>
    public AxiomWorkerException(string operation, AxiomStatus status, uint nativeErrorCode, string nativeMessage)
        : base($"Axiom worker call '{operation}' failed: {status}"
               + (nativeErrorCode != 0 ? $" (native {nativeErrorCode}: {nativeMessage})" : ""))
    {
        Status = status;
        NativeErrorCode = nativeErrorCode;
    }
}
