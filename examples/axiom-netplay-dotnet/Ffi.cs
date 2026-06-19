using System.Reflection;
using System.Runtime.InteropServices;

namespace Axiom.Netplay;

/// <summary>
/// P/Invoke bindings to the native <c>axiom-netplay-ffi</c> shared library, which
/// embeds the REAL Axiom engine. This is the ".NET host runs the engine via FFI"
/// route: the engine is compiled to a native <c>.dll</c>/<c>.so</c>/<c>.dylib</c>
/// (NOT wasm) and linked into this process at runtime. The resolver finds it in
/// the Cargo <c>target/</c> dir (built by <c>cargo build -p axiom-netplay-ffi</c>),
/// or via the <c>AXIOM_FFI_LIB</c> environment variable.
/// </summary>
internal static class Ffi
{
    private const string Lib = "axiom_netplay_ffi";

    static Ffi() => NativeLibrary.SetDllImportResolver(typeof(Ffi).Assembly, Resolve);

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

    // --- engine session ---
    [DllImport(Lib)] public static extern IntPtr axiom_netplay_create();
    [DllImport(Lib)] public static extern void axiom_netplay_apply_intent(IntPtr session, uint player, float dx, float dy);
    [DllImport(Lib)] public static extern void axiom_netplay_tick(IntPtr session);
    [DllImport(Lib)] public static extern nuint axiom_netplay_positions(IntPtr session, [Out] float[] outBuffer, nuint cap);
    [DllImport(Lib)] public static extern void axiom_netplay_destroy(IntPtr session);

    // --- canonical wire codec (the one source of truth; no C# codec twin) ---
    [DllImport(Lib)] public static extern int axiom_msg_kind(byte[] ptr, nuint len);
    [DllImport(Lib)] public static extern uint axiom_decode_join_version(byte[] ptr, nuint len);
    [DllImport(Lib)] public static extern int axiom_decode_client_intent(byte[] ptr, nuint len, out ulong seq, out float dx, out float dy);
    [DllImport(Lib)] public static extern nint axiom_encode_welcome(uint protocolVersion, ulong clientId, ulong serverTick, ulong fixedStepNs, [Out] byte[] outBuf, nuint cap);
    [DllImport(Lib)] public static extern nint axiom_encode_snapshot(ulong serverTick, ulong lastAccepted, byte[] payload, nuint payloadLen, [Out] byte[] outBuf, nuint cap);
    [DllImport(Lib)] public static extern nint axiom_encode_rejected(ulong seq, uint reason, [Out] byte[] outBuf, nuint cap);
}
