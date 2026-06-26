using System.Threading.Channels;
using Axiom.Netplay;

namespace Axiom.Netplay.Tests;

/// <summary>
/// An in-memory transport endpoint that drives <see cref="Room.RunSessionAsync"/>
/// with no sockets — deterministic, fast, and offline. The test plays the client:
/// it pushes frames toward the server (<see cref="ClientSend"/>) and captures the
/// frames the server sends back (<see cref="NextServerFrame"/>).
/// </summary>
public sealed class TestClient
{
    private readonly Channel<byte[]> _toServer = Channel.CreateUnbounded<byte[]>();
    private readonly Channel<byte[]> _fromServer = Channel.CreateUnbounded<byte[]>();

    /// <summary>The frame source the server reads (client → server).</summary>
    public ReadFrame Read => async ct =>
    {
        try { return await _toServer.Reader.ReadAsync(ct); }
        catch { return null; }
    };

    /// <summary>The frame sink the server writes (server → client); captured here.</summary>
    public SendFrame Send => (mem, _) =>
    {
        _fromServer.Writer.TryWrite(mem.ToArray());
        return Task.CompletedTask;
    };

    /// <summary>Push one frame toward the server.</summary>
    public void ClientSend(byte[] frame) => _toServer.Writer.TryWrite(frame);

    /// <summary>End the connection (the server's read loop sees end-of-stream).</summary>
    public void Disconnect() => _toServer.Writer.TryComplete();

    /// <summary>Await the next frame the server sent to this client.</summary>
    public async Task<byte[]> NextServerFrame(CancellationToken ct) => await _fromServer.Reader.ReadAsync(ct);
}
