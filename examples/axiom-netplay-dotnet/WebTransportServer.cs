using System.Buffers;
using System.Buffers.Binary;
using System.IO.Pipelines;
using System.Net;
using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using Microsoft.AspNetCore.Connections;
using Microsoft.AspNetCore.Http.Features;

namespace Axiom.Netplay;

/// <summary>
/// The WebTransport (HTTP/3 over QUIC) edge. .NET's public WebTransport API
/// exposes reliable bidirectional **streams** (not datagrams), so this carries
/// the same byte frames the WebSocket path does, length-prefixed (a stream is a
/// byte stream, not message-framed). One accepted stream per session feeds the
/// transport-neutral <see cref="Game.RunSessionAsync"/>.
/// </summary>
public static class WebTransportServer
{
    /// <summary>
    /// A short-lived self-signed cert for the HTTP/3 endpoint, plus the base64
    /// sha-256 of its DER so the browser can trust it via
    /// `serverCertificateHashes` (no CA). WebTransport requires an ECDSA cert
    /// valid for under two weeks.
    /// </summary>
    public static (X509Certificate2 Cert, string HashBase64) CreateDevCertificate()
    {
        using var ecdsa = ECDsa.Create(ECCurve.NamedCurves.nistP256);
        var request = new CertificateRequest("CN=localhost", ecdsa, HashAlgorithmName.SHA256);
        var san = new SubjectAlternativeNameBuilder();
        san.AddDnsName("localhost");
        san.AddIpAddress(IPAddress.Loopback);
        request.CertificateExtensions.Add(san.Build());

        var now = DateTimeOffset.UtcNow;
        using var selfSigned = request.CreateSelfSigned(now.AddDays(-1), now.AddDays(13));
        // Re-import via PFX so Kestrel can use the private key (Windows quirk).
        var cert = X509CertificateLoader.LoadPkcs12(selfSigned.Export(X509ContentType.Pfx), null);
        string hash = Convert.ToBase64String(SHA256.HashData(cert.RawData));
        return (cert, hash);
    }

    /// <summary>Accept a WebTransport session + its first bidi stream, then run a game session over it.</summary>
    public static async Task HandleAsync(IHttpWebTransportFeature feature, Game game, ILogger logger, CancellationToken ct)
    {
        var session = await feature.AcceptAsync();
        ConnectionContext? stream = await session.AcceptStreamAsync(ct);
        if (stream is null) return;
        try
        {
            var read = FramedReader(stream.Transport.Input);
            var send = FramedSender(stream.Transport.Output);
            await game.RunSessionAsync(read, send, "wt", logger, ct);
        }
        finally
        {
            await stream.DisposeAsync();
        }
    }

    /// Read one length-prefixed (u32 LE) frame from the stream pipe, or null at end.
    private static ReadFrame FramedReader(PipeReader reader) => async (ct) =>
    {
        while (true)
        {
            ReadResult result;
            try { result = await reader.ReadAsync(ct); }
            catch { return null; }

            ReadOnlySequence<byte> buffer = result.Buffer;
            if (TryReadFrame(ref buffer, out byte[]? frame))
            {
                reader.AdvanceTo(buffer.Start);
                return frame;
            }
            reader.AdvanceTo(buffer.Start, buffer.End);
            if (result.IsCompleted) return null;
        }
    };

    /// Write one length-prefixed (u32 LE) frame to the stream pipe.
    private static SendFrame FramedSender(PipeWriter writer) => async (data, ct) =>
    {
        var framed = new byte[4 + data.Length];
        BinaryPrimitives.WriteUInt32LittleEndian(framed, (uint)data.Length);
        data.Span.CopyTo(framed.AsSpan(4));
        await writer.WriteAsync(framed, ct);
    };

    private static bool TryReadFrame(ref ReadOnlySequence<byte> buffer, out byte[]? frame)
    {
        frame = null;
        if (buffer.Length < 4) return false;
        Span<byte> lengthBytes = stackalloc byte[4];
        buffer.Slice(0, 4).CopyTo(lengthBytes);
        uint length = BinaryPrimitives.ReadUInt32LittleEndian(lengthBytes);
        if (buffer.Length < 4 + length) return false;
        frame = buffer.Slice(4, (int)length).ToArray();
        buffer = buffer.Slice(4 + length);
        return true;
    }
}
