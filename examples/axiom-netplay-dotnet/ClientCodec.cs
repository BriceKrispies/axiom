using System.Buffers.Binary;
using System.Text;

namespace Axiom.Netplay;

/// <summary>
/// A native client's view of the canonical Tier-A wire codec — the same Rust
/// codec the server uses, exposed for building client frames (a future native
/// client, the malicious-client harness, and tests) without a hand-written twin.
/// Encoders throw on a codec failure; <see cref="MessageKind"/> peeks a frame.
/// </summary>
public static class ClientCodec
{
    /// <summary>Encode a <c>JoinRoom</c> frame.</summary>
    public static byte[] EncodeJoinRoom(uint protocolVersion = Room.ProtocolVersion, string roomId = "lobby", string token = "")
    {
        byte[] room = Encoding.UTF8.GetBytes(roomId);
        byte[] tok = Encoding.UTF8.GetBytes(token);
        return Encode(buf => AxiomWorkerNative.axiom_encode_join_room(
            protocolVersion, room, (nuint)room.Length, tok.Length == 0 ? null : tok, (nuint)tok.Length, buf, (nuint)buf.Length), 128);
    }

    /// <summary>Encode a <c>ClientIntent</c> carrying a <c>(dx, dy)</c> move.</summary>
    public static byte[] EncodeMoveIntent(ulong clientSequence, float dx, float dy, ulong predictedClientTick = 0, ulong lastSeenServerTick = 0)
    {
        var payload = new byte[8];
        BinaryPrimitives.WriteSingleLittleEndian(payload.AsSpan(0, 4), dx);
        BinaryPrimitives.WriteSingleLittleEndian(payload.AsSpan(4, 4), dy);
        return Encode(buf => AxiomWorkerNative.axiom_encode_client_intent(
            clientSequence, predictedClientTick, lastSeenServerTick, payload, (nuint)payload.Length, buf, (nuint)buf.Length), 128);
    }

    /// <summary>Encode a <c>ClientIntent</c> with an arbitrary opaque payload (for
    /// malformed/oversized-payload attack tests).</summary>
    public static byte[] EncodeRawIntent(ulong clientSequence, byte[] payload)
    {
        byte[] p = payload.Length == 0 ? new byte[1] : payload; // codec accepts empty; keep a stable buffer
        return Encode(buf => AxiomWorkerNative.axiom_encode_client_intent(
            clientSequence, 0, 0, payload, (nuint)payload.Length, buf, (nuint)buf.Length), payload.Length + 128);
    }

    /// <summary>Encode a server→client <c>ServerSnapshot</c> frame. Used by the
    /// malicious-client test to PROVE that a client sending an authoritative frame
    /// is rejected — the server must never apply it.</summary>
    public static byte[] EncodeServerSnapshot(ulong serverTick, ulong lastAccepted, byte[] payload) =>
        Encode(buf => AxiomWorkerNative.axiom_encode_snapshot(serverTick, lastAccepted, payload, (nuint)payload.Length, buf, (nuint)buf.Length), payload.Length + 128);

    /// <summary>The message-kind discriminant of a frame (or -1 if malformed).</summary>
    public static int MessageKind(byte[] frame) => AxiomWorkerNative.axiom_msg_kind(frame, (nuint)frame.Length);

    private static byte[] Encode(Func<byte[], nint> encode, int capacity)
    {
        var buffer = new byte[capacity];
        nint written = encode(buffer);
        if (written < 0)
            throw new InvalidOperationException("client codec failed to encode a frame");
        return buffer.AsSpan(0, (int)written).ToArray();
    }
}
