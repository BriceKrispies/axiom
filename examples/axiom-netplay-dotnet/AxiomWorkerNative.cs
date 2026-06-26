using System.Reflection;
using System.Runtime.InteropServices;

namespace Axiom.Netplay;

/// <summary>
/// Raw P/Invoke bindings to the native <c>axiom-netplay-ffi</c> shared library —
/// the in-process Axiom simulation worker. Two tiers cross this boundary:
/// <list type="bullet">
/// <item>Tier-B worker control (<c>axiom_sim_*</c> / <c>axiom_worker_*</c>):
/// server-only. The .NET host drives the deterministic simulation through it.</item>
/// <item>Tier-A wire codec (<c>axiom_msg_*</c> / <c>axiom_encode_*</c> /
/// <c>axiom_decode_*</c>): the browser-facing protocol, the one source of truth
/// so there is no hand-written C# codec twin.</item>
/// </list>
/// This class is the unsafe edge only; <see cref="AxiomWorker"/> is the typed,
/// lifetime-safe API the rest of the server uses.
/// </summary>
internal static class AxiomWorkerNative
{
    private const string Lib = "axiom_netplay_ffi";

    static AxiomWorkerNative() => NativeLibrary.SetDllImportResolver(typeof(AxiomWorkerNative).Assembly, Resolve);

    /// <summary>Force the static constructor (and thus the resolver) to register.</summary>
    public static void EnsureLoaded() { }

    private static IntPtr Resolve(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (libraryName != Lib) return IntPtr.Zero;
        foreach (string path in CandidatePaths())
            if (File.Exists(path) && NativeLibrary.TryLoad(path, out IntPtr handle))
                return handle;
        return IntPtr.Zero; // fall back to the default OS search
    }

    private static IEnumerable<string> CandidatePaths()
    {
        string file =
            OperatingSystem.IsWindows() ? $"{Lib}.dll" :
            OperatingSystem.IsMacOS() ? $"lib{Lib}.dylib" :
            $"lib{Lib}.so";

        string? env = Environment.GetEnvironmentVariable("AXIOM_FFI_LIB");
        if (!string.IsNullOrEmpty(env)) yield return env;

        foreach (string start in new[] { AppContext.BaseDirectory, Directory.GetCurrentDirectory() })
            for (DirectoryInfo? dir = new(start); dir is not null; dir = dir.Parent)
            {
                yield return Path.Combine(dir.FullName, "target", "release", file);
                yield return Path.Combine(dir.FullName, "target", "debug", file);
            }
    }

    // --- Tier-B: version handshake ---
    [DllImport(Lib)] public static extern uint axiom_worker_version_major();
    [DllImport(Lib)] public static extern uint axiom_worker_version_minor();
    [DllImport(Lib)] public static extern uint axiom_worker_version_patch();
    [DllImport(Lib)] public static extern uint axiom_worker_protocol_version();

    // --- Tier-B: lifecycle ---
    [DllImport(Lib)] public static extern IntPtr axiom_sim_create(ulong seed, uint maxPlayers, ulong fixedStepNs);
    [DllImport(Lib)] public static extern void axiom_sim_destroy(IntPtr sim);

    // --- Tier-B: state + control ---
    [DllImport(Lib)] public static extern int axiom_sim_load_state(IntPtr sim, byte[] ptr, nuint len);

    [DllImport(Lib)]
    public static extern int axiom_sim_submit_intent(
        IntPtr sim, uint playerId, ulong clientSequence, ulong predictedClientTick,
        byte[] payload, nuint payloadLen, out uint outReasonCode);

    [DllImport(Lib)]
    public static extern int axiom_sim_advance_tick(IntPtr sim, ulong targetTick, out ulong outTick, out ulong outStateHash);

    [DllImport(Lib)] public static extern int axiom_sim_snapshot_len(IntPtr sim, out nuint outLen);

    [DllImport(Lib)]
    public static extern int axiom_sim_snapshot_write(
        IntPtr sim, byte[] outBuf, nuint outCapacity, out nuint outWritten, out ulong outStateHash);

    [DllImport(Lib)] public static extern int axiom_sim_state_hash(IntPtr sim, out ulong outHash);
    [DllImport(Lib)] public static extern int axiom_sim_render_view_write(IntPtr sim, float[] outFloats, nuint cap, out nuint outCount);

    // --- Tier-B: replay ---
    [DllImport(Lib)] public static extern int axiom_sim_export_replay_len(IntPtr sim, out nuint outLen);
    [DllImport(Lib)] public static extern int axiom_sim_export_replay_write(IntPtr sim, byte[] outBuf, nuint outCapacity, out nuint outWritten);

    [DllImport(Lib)]
    public static extern int axiom_sim_verify_replay(
        byte[] ptr, nuint len, out uint outMatched, out ulong outFirstDivergenceTick, out ulong outFinalHash);

    // --- Tier-B: last error ---
    [DllImport(Lib)] public static extern uint axiom_sim_last_error_code(IntPtr sim);
    [DllImport(Lib)] public static extern int axiom_sim_last_error_message_write(IntPtr sim, byte[] outBuf, nuint outCapacity, out nuint outWritten);

    // --- Tier-A: the canonical browser-facing wire codec (one source of truth) ---
    [DllImport(Lib)] public static extern int axiom_msg_kind(byte[] ptr, nuint len);
    [DllImport(Lib)] public static extern uint axiom_decode_join_version(byte[] ptr, nuint len);
    [DllImport(Lib)] public static extern nint axiom_decode_join_room_id(byte[] ptr, nuint len, byte[] outBuf, nuint cap);
    [DllImport(Lib)] public static extern int axiom_decode_client_intent(byte[] ptr, nuint len, out ulong seq, out float dx, out float dy);
    [DllImport(Lib)] public static extern nint axiom_encode_join_room(uint protocolVersion, byte[] roomId, nuint roomIdLen, byte[]? token, nuint tokenLen, byte[] outBuf, nuint cap);
    [DllImport(Lib)] public static extern nint axiom_encode_client_intent(ulong clientSequence, ulong predictedClientTick, ulong lastSeenServerTick, byte[] payload, nuint payloadLen, byte[] outBuf, nuint cap);
    [DllImport(Lib)] public static extern nint axiom_encode_welcome(uint protocolVersion, ulong clientId, ulong serverTick, ulong fixedStepNs, byte[] outBuf, nuint cap);
    [DllImport(Lib)] public static extern nint axiom_encode_snapshot(ulong serverTick, ulong lastAccepted, byte[] payload, nuint payloadLen, byte[] outBuf, nuint cap);
    [DllImport(Lib)] public static extern nint axiom_encode_rejected(ulong seq, uint reason, byte[] outBuf, nuint cap);
}
