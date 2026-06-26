using System.Buffers.Binary;
using System.Diagnostics;
using System.Net;
using System.Net.Sockets;
using System.Reflection;

namespace Axiom.Netplay;

/// <summary>
/// An <see cref="IAxiomSim"/> backed by a SEPARATE worker process. Each call is
/// marshalled over a local TCP socket to the <c>axiom-netplay-worker</c> binary,
/// which owns one <see cref="Axiom.Netplay"/> sim. Running the authoritative sim
/// out-of-process gives crash isolation: a worker fault takes down only its room,
/// not the host.
///
/// It is self-supervising. After every advance it checkpoints the full
/// authoritative snapshot; if a call finds the worker dead, it respawns the
/// process, reconnects, restores that snapshot, and retries the call once. Because
/// the sim is deterministic and the snapshot is exact, recovery resumes the room
/// at precisely the pre-crash authoritative state.
/// </summary>
public sealed class OutOfProcAxiomSim : IAxiomSim
{
    // Opcodes — must match apps/axiom-netplay-ffi/src/ipc.rs.
    private const byte OpSubmitIntent = 0x01;
    private const byte OpAdvanceTick = 0x02;
    private const byte OpRenderView = 0x03;
    private const byte OpSnapshot = 0x04;
    private const byte OpLoadState = 0x05;
    private const byte OpStateHash = 0x06;
    private const byte OpExportReplay = 0x07;
    private const byte OpRestoreAt = 0x08;

    private readonly string _workerExe;
    private readonly ulong _seed;
    private readonly ulong _fixedStepNs;
    private readonly ILogger _logger;
    private readonly object _gate = new();

    private Process _process = null!;
    private NetworkStream _stream = null!;
    private byte[]? _lastSnapshot;
    private ulong _lastTick;
    private bool _disposed;

    /// <inheritdoc/>
    public uint MaxPlayers { get; }

    /// <summary>Spawn a worker process for a fresh sim and connect to it.</summary>
    public OutOfProcAxiomSim(string workerExe, ulong seed, uint maxPlayers, ulong fixedStepNs, ILogger logger)
    {
        _workerExe = workerExe;
        _seed = seed;
        MaxPlayers = maxPlayers;
        _fixedStepNs = fixedStepNs;
        _logger = logger;
        Spawn();
    }

    /// <inheritdoc/>
    public IntentOutcome SubmitIntent(uint playerId, ulong clientSequence, ulong predictedClientTick, byte[] payload)
    {
        var w = new IpcWriter(OpSubmitIntent);
        w.U32(playerId);
        w.U64(clientSequence);
        w.U64(predictedClientTick);
        w.Bytes(payload);
        lock (_gate)
        {
            var r = new IpcReader(RoundtripLocked(w.ToArray()));
            r.ExpectTag(OpSubmitIntent);
            uint reason = r.U32();
            return reason == 0
                ? new IntentOutcome(true, AxiomRejectReason.None)
                : new IntentOutcome(false, (AxiomRejectReason)reason);
        }
    }

    /// <inheritdoc/>
    public TickOutcome AdvanceTick(ulong targetTick)
    {
        var w = new IpcWriter(OpAdvanceTick);
        w.U64(targetTick);
        lock (_gate)
        {
            var r = new IpcReader(RoundtripLocked(w.ToArray()));
            r.ExpectTag(OpAdvanceTick);
            ulong tick = r.U64();
            ulong hash = r.U64();
            // Checkpoint the post-tick snapshot AND tick so a crash recovers exactly
            // (the engine snapshot carries scene state but not the worker's tick).
            _lastTick = tick;
            _lastSnapshot = SnapshotBytesLocked();
            return new TickOutcome(tick, hash);
        }
    }

    /// <inheritdoc/>
    public float[] GetRenderView()
    {
        lock (_gate)
        {
            var r = new IpcReader(RoundtripLocked(new IpcWriter(OpRenderView).ToArray()));
            r.ExpectTag(OpRenderView);
            uint count = r.U32();
            var floats = new float[count];
            for (uint i = 0; i < count; i++) floats[i] = r.F32();
            return floats;
        }
    }

    /// <inheritdoc/>
    public (byte[] Snapshot, ulong StateHash) GetSnapshot()
    {
        lock (_gate)
        {
            (byte[] bytes, ulong hash) = ReadSnapshot(RoundtripLocked(new IpcWriter(OpSnapshot).ToArray()));
            return (bytes, hash);
        }
    }

    /// <inheritdoc/>
    public void LoadState(byte[] snapshotBytes)
    {
        var w = new IpcWriter(OpLoadState);
        w.Bytes(snapshotBytes);
        lock (_gate)
        {
            var r = new IpcReader(RoundtripLocked(w.ToArray()));
            r.ExpectTag(OpLoadState);
            if (r.U8() == 0)
                throw new AxiomWorkerException("load_state", AxiomStatus.Deserialize, 0, "worker rejected snapshot");
            _lastSnapshot = snapshotBytes;
        }
    }

    /// <inheritdoc/>
    public ulong GetStateHash()
    {
        lock (_gate)
        {
            var r = new IpcReader(RoundtripLocked(new IpcWriter(OpStateHash).ToArray()));
            r.ExpectTag(OpStateHash);
            return r.U64();
        }
    }

    /// <inheritdoc/>
    public byte[] ExportReplay()
    {
        lock (_gate)
        {
            var r = new IpcReader(RoundtripLocked(new IpcWriter(OpExportReplay).ToArray()));
            r.ExpectTag(OpExportReplay);
            return r.LpBytes();
        }
    }

    /// <summary>Forcibly kill the worker process — used by tests to simulate a
    /// crash; the next call must transparently respawn and resume.</summary>
    public void KillWorkerForTest()
    {
        lock (_gate)
        {
            try { if (!_process.HasExited) _process.Kill(entireProcessTree: true); } catch { /* already gone */ }
        }
    }

    private byte[] SnapshotBytesLocked()
    {
        (byte[] bytes, ulong _) = ReadSnapshot(RoundtripLocked(new IpcWriter(OpSnapshot).ToArray()));
        return bytes;
    }

    private static (byte[] Bytes, ulong Hash) ReadSnapshot(byte[] frame)
    {
        var r = new IpcReader(frame);
        r.ExpectTag(OpSnapshot);
        ulong hash = r.U64();
        byte[] bytes = r.LpBytes();
        return (bytes, hash);
    }

    /// One request→response over the socket, recovering ONCE from a dead worker
    /// (respawn + restore the last snapshot) before retrying.
    private byte[] RoundtripLocked(byte[] request)
    {
        try
        {
            return RoundtripRaw(request);
        }
        catch (Exception ex) when (ex is IOException or SocketException or ObjectDisposedException)
        {
            _logger.LogWarning(ex, "out-of-proc worker connection lost — respawning and restoring");
            RecoverLocked();
            return RoundtripRaw(request);
        }
    }

    private byte[] RoundtripRaw(byte[] request)
    {
        WriteFrame(_stream, request);
        return ReadFrame(_stream) ?? throw new IOException("worker closed the connection");
    }

    private void RecoverLocked()
    {
        SafeShutdown();
        Spawn(); // reconnects AND restores _lastSnapshot
    }

    private void Spawn()
    {
        var psi = new ProcessStartInfo(_workerExe)
        {
            RedirectStandardOutput = true,
            UseShellExecute = false,
            CreateNoWindow = true,
        };
        psi.ArgumentList.Add("--seed"); psi.ArgumentList.Add(_seed.ToString());
        psi.ArgumentList.Add("--max-players"); psi.ArgumentList.Add(MaxPlayers.ToString());
        psi.ArgumentList.Add("--fixed-step"); psi.ArgumentList.Add(_fixedStepNs.ToString());

        _process = Process.Start(psi) ?? throw new AxiomWorkerException("spawn", AxiomStatus.Engine, 0, "failed to start worker process");

        // Handshake: the worker prints its chosen ephemeral port on stdout.
        string? portLine = _process.StandardOutput.ReadLine();
        if (portLine is null || !int.TryParse(portLine.Trim(), out int port))
            throw new AxiomWorkerException("spawn", AxiomStatus.Engine, 0, "worker did not report a port");

        var client = new TcpClient();
        client.Connect(IPAddress.Loopback, port);
        client.NoDelay = true;
        _stream = client.GetStream();

        // If we are recovering from a crash, restore the last good snapshot AND the
        // tick so the room resumes at exactly the pre-crash authoritative state.
        if (_lastSnapshot is not null)
        {
            var w = new IpcWriter(OpRestoreAt);
            w.U64(_lastTick);
            w.Bytes(_lastSnapshot);
            var r = new IpcReader(RoundtripRaw(w.ToArray()));
            r.ExpectTag(OpLoadState); // RestoreAt is answered with a Loaded(bool)
            if (r.U8() == 0)
                throw new AxiomWorkerException("restore", AxiomStatus.Deserialize, 0, "respawned worker rejected snapshot");
            _logger.LogWarning("out-of-proc worker respawned and restored ({Bytes} bytes @ tick {Tick})", _lastSnapshot.Length, _lastTick);
        }
    }

    private void SafeShutdown()
    {
        try { _stream?.Dispose(); } catch { /* ignore */ }
        try { if (_process is { HasExited: false }) _process.Kill(entireProcessTree: true); } catch { /* ignore */ }
    }

    /// <inheritdoc/>
    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed) return;
            _disposed = true;
            SafeShutdown();
            try { _process?.Dispose(); } catch { /* ignore */ }
        }
    }

    /// <summary>Resolve the worker executable: an <c>AXIOM_WORKER_EXE</c> override,
    /// else the cargo build output (release preferred) found by walking up from the
    /// host assembly and the working directory.</summary>
    public static string ResolveWorkerExe()
    {
        string file = OperatingSystem.IsWindows() ? "axiom-netplay-worker.exe" : "axiom-netplay-worker";

        string? env = Environment.GetEnvironmentVariable("AXIOM_WORKER_EXE");
        if (!string.IsNullOrEmpty(env)) return env;

        foreach (string start in new[] { AppContext.BaseDirectory, Directory.GetCurrentDirectory(), Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location) ?? "." })
            for (DirectoryInfo? dir = new(start); dir is not null; dir = dir.Parent)
            {
                string release = Path.Combine(dir.FullName, "target", "release", file);
                if (File.Exists(release)) return release;
                string debug = Path.Combine(dir.FullName, "target", "debug", file);
                if (File.Exists(debug)) return debug;
            }
        throw new FileNotFoundException(
            $"could not find the {file} worker binary — build it with `cargo build -p axiom-netplay-ffi --release`");
    }

    private static void WriteFrame(NetworkStream stream, byte[] body)
    {
        Span<byte> len = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(len, (uint)body.Length);
        stream.Write(len);
        stream.Write(body);
        stream.Flush();
    }

    private static byte[]? ReadFrame(NetworkStream stream)
    {
        byte[]? len = ReadExact(stream, 4);
        if (len is null) return null;
        uint n = BinaryPrimitives.ReadUInt32LittleEndian(len);
        return ReadExact(stream, (int)n) ?? throw new IOException("worker truncated a frame");
    }

    private static byte[]? ReadExact(NetworkStream stream, int count)
    {
        var buf = new byte[count];
        int read = 0;
        while (read < count)
        {
            int n = stream.Read(buf, read, count - read);
            if (n == 0) return read == 0 ? null : throw new IOException("worker closed mid-frame");
            read += n;
        }
        return buf;
    }
}

/// A little-endian frame builder for IPC requests.
internal sealed class IpcWriter
{
    private readonly List<byte> _bytes = new();

    public IpcWriter(byte opcode) => _bytes.Add(opcode);

    public void U32(uint v)
    {
        Span<byte> b = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(b, v);
        _bytes.AddRange(b.ToArray());
    }

    public void U64(ulong v)
    {
        Span<byte> b = stackalloc byte[8];
        BinaryPrimitives.WriteUInt64LittleEndian(b, v);
        _bytes.AddRange(b.ToArray());
    }

    public void Bytes(byte[] payload)
    {
        U32((uint)payload.Length);
        _bytes.AddRange(payload);
    }

    public byte[] ToArray() => _bytes.ToArray();
}

/// A little-endian frame reader for IPC responses.
internal sealed class IpcReader
{
    private readonly byte[] _data;
    private int _pos;

    public IpcReader(byte[] data) => _data = data;

    public void ExpectTag(byte expected)
    {
        byte tag = U8();
        if (tag != expected)
            throw new AxiomWorkerException("ipc", AxiomStatus.Engine, tag,
                $"unexpected response tag {tag:X2} (wanted {expected:X2})");
    }

    public byte U8() => _data[_pos++];

    public uint U32()
    {
        uint v = BinaryPrimitives.ReadUInt32LittleEndian(_data.AsSpan(_pos, 4));
        _pos += 4;
        return v;
    }

    public ulong U64()
    {
        ulong v = BinaryPrimitives.ReadUInt64LittleEndian(_data.AsSpan(_pos, 8));
        _pos += 8;
        return v;
    }

    public float F32()
    {
        float v = BinaryPrimitives.ReadSingleLittleEndian(_data.AsSpan(_pos, 4));
        _pos += 4;
        return v;
    }

    public byte[] LpBytes()
    {
        int n = (int)U32();
        byte[] slice = _data.AsSpan(_pos, n).ToArray();
        _pos += n;
        return slice;
    }
}
