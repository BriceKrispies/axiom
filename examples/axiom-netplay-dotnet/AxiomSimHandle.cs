using System.Runtime.InteropServices;

namespace Axiom.Netplay;

/// <summary>
/// A safe lifetime wrapper around a native sim-instance pointer. The native
/// <c>axiom_sim_destroy</c> is called exactly once when the handle is disposed or
/// finalized, so a sim can never be leaked or double-freed even if a session
/// loop throws.
/// </summary>
public sealed class AxiomSimHandle : SafeHandle
{
    private AxiomSimHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    /// <summary>Create a sim instance. Returns an invalid handle on bad arguments
    /// or a caught panic (the caller checks <see cref="IsInvalid"/>).</summary>
    public static AxiomSimHandle Create(ulong seed, uint maxPlayers, ulong fixedStepNs)
    {
        AxiomWorkerNative.EnsureLoaded();
        var handle = new AxiomSimHandle();
        IntPtr raw = AxiomWorkerNative.axiom_sim_create(seed, maxPlayers, fixedStepNs);
        handle.SetHandle(raw);
        return handle;
    }

    /// <inheritdoc/>
    public override bool IsInvalid => handle == IntPtr.Zero;

    /// <summary>The raw pointer for an interop call. Throws if the handle is invalid.</summary>
    internal IntPtr Pointer => IsInvalid
        ? throw new ObjectDisposedException(nameof(AxiomSimHandle))
        : handle;

    /// <inheritdoc/>
    protected override bool ReleaseHandle()
    {
        AxiomWorkerNative.axiom_sim_destroy(handle);
        return true;
    }
}
