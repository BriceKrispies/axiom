using System.Net;
using System.Threading.Channels;
using Microsoft.AspNetCore.Http;
using SIPSorcery.Net;

namespace Axiom.Netplay;

/// <summary>
/// The WebRTC edge — true unreliable, unordered UDP in the browser, via a
/// DataChannel (`{ordered:false, maxRetransmits:0}`). Unlike WebTransport/HTTP-3,
/// WebRTC negotiates a direct UDP socket between the two peers (here both on the
/// same host, over loopback), so it works on Windows 10 with no QUIC/WSL gap and
/// no manual cert (DTLS is self-negotiated). One HTTP POST exchanges the SDP
/// offer/answer (non-trickle ICE); the data channel then bridges to the same
/// transport-neutral <see cref="Room.RunSessionAsync"/>.
/// </summary>
public static class WebRtcServer
{
    private sealed record SdpMessage(string type, string sdp);

    /// <summary>Handle a browser's SDP offer: build a peer + data channel, wire it to the room, return the answer.</summary>
    public static async Task<IResult> HandleOfferAsync(HttpContext context, RoomRegistry registry, ILogger logger, CancellationToken ct)
    {
        var offer = await context.Request.ReadFromJsonAsync<SdpMessage>(ct);
        if (offer is null) return Results.BadRequest();

        // Bind ICE to all IPv4 interfaces and advertise the routable host
        // candidate (the correct config for a real deployment). On a single dev
        // host the browser↔server UDP must be permitted by the OS firewall.
        var pc = new RTCPeerConnection(new RTCConfiguration { X_BindAddress = IPAddress.Any });

        pc.ondatachannel += (RTCDataChannel dc) =>
        {
            // The data channel preserves message boundaries (each send = one frame),
            // so no length-framing is needed. Inbound frames queue for ReadFrame.
            var inbound = Channel.CreateUnbounded<byte[]>();
            dc.onmessage += (RTCDataChannel _, DataChannelPayloadProtocols _, byte[] data) => inbound.Writer.TryWrite(data);
            dc.onclose += () => inbound.Writer.TryComplete();
            dc.onerror += (_) => inbound.Writer.TryComplete();

            ReadFrame read = async (c) =>
            {
                try { return await inbound.Reader.ReadAsync(c); }
                catch { return null; }
            };
            SendFrame send = (mem, _) => { dc.send(mem.ToArray()); return Task.CompletedTask; };

            dc.onopen += () => _ = Task.Run(async () =>
            {
                try { await registry.ServeAsync(read, send, "webrtc", ct); }
                finally { pc.close(); }
            });
        };

        pc.onconnectionstatechange += (state) =>
        {
            logger.LogInformation("[webrtc] connection state: {State}", state);
            if (state is RTCPeerConnectionState.failed or RTCPeerConnectionState.closed)
                pc.close();
        };
        pc.oniceconnectionstatechange += (state) => logger.LogInformation("[webrtc] ICE state: {State}", state);

        var setResult = pc.setRemoteDescription(new RTCSessionDescriptionInit { type = RTCSdpType.offer, sdp = offer.sdp });
        if (setResult != SetDescriptionResultEnum.OK)
        {
            logger.LogWarning("[webrtc] rejected offer: {Result}", setResult);
            return Results.BadRequest();
        }

        var answer = pc.createAnswer(null);
        await pc.setLocalDescription(answer);
        await WaitForIceGatheringAsync(pc, TimeSpan.FromSeconds(3));

        string answerSdp = pc.localDescription.sdp.ToString();
        var candidates = answerSdp.Split('\n').Where(l => l.Contains("candidate")).Select(l => l.Trim()).ToList();
        logger.LogInformation("[webrtc] answer carries {N} candidate line(s): {C}", candidates.Count, string.Join("  ||  ", candidates));
        return Results.Json(new SdpMessage("answer", answerSdp));
    }

    /// Non-trickle ICE: wait until host candidates are gathered so the answer SDP carries them.
    private static async Task WaitForIceGatheringAsync(RTCPeerConnection pc, TimeSpan timeout)
    {
        if (pc.iceGatheringState == RTCIceGatheringState.complete) return;
        var done = new TaskCompletionSource();
        void OnChange(RTCIceGatheringState s)
        {
            if (s == RTCIceGatheringState.complete) done.TrySetResult();
        }
        pc.onicegatheringstatechange += OnChange;
        try
        {
            if (pc.iceGatheringState != RTCIceGatheringState.complete)
                await Task.WhenAny(done.Task, Task.Delay(timeout));
        }
        finally
        {
            pc.onicegatheringstatechange -= OnChange;
        }
    }
}
