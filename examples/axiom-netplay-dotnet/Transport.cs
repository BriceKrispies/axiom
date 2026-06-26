using System.Net.WebSockets;

namespace Axiom.Netplay;

/// <summary>Send one frame to a client over whatever transport it connected with.</summary>
public delegate Task SendFrame(ReadOnlyMemory<byte> data, CancellationToken ct);

/// <summary>Read the next inbound frame, or null when the connection ends.</summary>
public delegate Task<byte[]?> ReadFrame(CancellationToken ct);

/// <summary>Transport-neutral frame source/sink helpers. The authoritative
/// <see cref="Room"/> consumes a <see cref="ReadFrame"/> + <see cref="SendFrame"/>
/// pair and is unaware of WebSocket / WebTransport / WebRTC underneath.</summary>
public static class Transport
{
    /// <summary>A WebSocket frame source (WebSocket preserves message boundaries).</summary>
    public static ReadFrame WebSocketReader(WebSocket ws) => async (ct) =>
    {
        var buffer = new byte[4096];
        using var ms = new MemoryStream();
        while (true)
        {
            WebSocketReceiveResult result;
            try { result = await ws.ReceiveAsync(buffer, ct); }
            catch { return null; }
            if (result.MessageType == WebSocketMessageType.Close) return null;
            ms.Write(buffer, 0, result.Count);
            if (result.EndOfMessage) break;
        }
        return ms.ToArray();
    };

    /// <summary>A WebSocket frame sink.</summary>
    public static SendFrame WebSocketSender(WebSocket ws) =>
        (data, ct) => ws.SendAsync(data, WebSocketMessageType.Binary, endOfMessage: true, ct).AsTask();
}
