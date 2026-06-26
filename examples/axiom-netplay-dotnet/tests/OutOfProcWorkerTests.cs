using System.Buffers.Binary;
using System.Diagnostics;
using Axiom.Netplay;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace Axiom.Netplay.Tests;

/// <summary>Phase C — running the authoritative sim OUT OF PROCESS (a spawned
/// worker per sim, over a local socket) must be behaviourally identical to the
/// in-process path, and must survive a worker crash by respawning and restoring
/// the last authoritative snapshot.</summary>
public sealed class OutOfProcWorkerTests
{
    // Build the worker binary on demand so `dotnet test` is self-sufficient even
    // if `cargo build` has not run yet.
    private static readonly Lazy<string> WorkerExeLazy = new(() =>
    {
        try { return OutOfProcAxiomSim.ResolveWorkerExe(); }
        catch (FileNotFoundException)
        {
            BuildWorker();
            return OutOfProcAxiomSim.ResolveWorkerExe();
        }
    });

    private static string WorkerExe() => WorkerExeLazy.Value;

    private static OutOfProcAxiomSim OutProc(ulong seed) =>
        new(WorkerExe(), seed, maxPlayers: 2, fixedStepNs: 16_666_667, NullLogger.Instance);

    private static byte[] Move(float dx, float dy)
    {
        var p = new byte[8];
        BinaryPrimitives.WriteSingleLittleEndian(p.AsSpan(0, 4), dx);
        BinaryPrimitives.WriteSingleLittleEndian(p.AsSpan(4, 4), dy);
        return p;
    }

    [Fact]
    public void out_of_process_is_byte_identical_to_in_process()
    {
        var worker = new AxiomWorker();
        Assert.False(worker.OutOfProcess, "this parity test expects the default in-process mode");

        using IAxiomSim inproc = worker.CreateSim(seed: 5, maxPlayers: 2, fixedStepNs: 16_666_667);
        using IAxiomSim outproc = OutProc(5);

        // Drive both with identical scripted inputs; every tick's count and hash
        // must match, across the process boundary.
        for (uint tick = 0; tick < 20; tick++)
        {
            if (tick % 2 == 0)
            {
                byte[] move = Move(0.1f, 0f);
                inproc.SubmitIntent(0, tick + 1, 0, move);
                outproc.SubmitIntent(0, tick + 1, 0, move);
            }
            TickOutcome i = inproc.AdvanceTick(tick);
            TickOutcome o = outproc.AdvanceTick(tick);
            Assert.Equal(i.Tick, o.Tick);
            Assert.Equal(i.StateHash, o.StateHash);
        }

        // Full snapshot byte-equality is the determinism proof.
        Assert.Equal(inproc.GetSnapshot().Snapshot, outproc.GetSnapshot().Snapshot);
    }

    [Fact]
    public void a_crashed_worker_is_respawned_and_resumes_identical_state()
    {
        using var outproc = OutProc(9);
        outproc.SubmitIntent(0, 1, 0, Move(0.5f, 0f));
        for (ulong t = 1; t <= 8; t++) outproc.AdvanceTick(t);
        ulong before = outproc.GetStateHash();

        // Simulate a worker crash. The next call must transparently respawn the
        // process and restore the checkpoint — the authoritative state is exact.
        outproc.KillWorkerForTest();
        ulong after = outproc.GetStateHash();
        Assert.Equal(before, after);

        // And the recovered worker keeps advancing deterministically.
        TickOutcome next = outproc.AdvanceTick(9);
        Assert.Equal(9ul, next.Tick);
    }

    private static void BuildWorker()
    {
        string root = RepoRoot();
        var psi = new ProcessStartInfo("cargo") { UseShellExecute = false, WorkingDirectory = root };
        psi.ArgumentList.Add("build");
        psi.ArgumentList.Add("-p");
        psi.ArgumentList.Add("axiom-netplay-ffi");
        using Process? p = Process.Start(psi);
        p?.WaitForExit();
    }

    private static string RepoRoot()
    {
        for (DirectoryInfo? d = new(AppContext.BaseDirectory); d is not null; d = d.Parent)
            if (File.Exists(Path.Combine(d.FullName, "Cargo.toml")) && Directory.Exists(Path.Combine(d.FullName, "examples")))
                return d.FullName;
        return Directory.GetCurrentDirectory();
    }
}
