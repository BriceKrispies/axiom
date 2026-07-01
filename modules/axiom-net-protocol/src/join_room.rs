//! `JoinRoom` — a client's request to join a room (client → server).

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::frame;
use crate::opaque_payload::OpaquePayload;
use crate::protocol_version::ProtocolVersion;
use crate::room_id::RoomId;

/// A client's request to join a room. The first message a client sends after
/// the socket opens: it announces the application [`ProtocolVersion`], names the
/// [`RoomId`], and carries an optional opaque authentication token (empty when
/// absent). The server replies with `Welcome` or refuses the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinRoom {
    protocol_version: ProtocolVersion,
    room_id: RoomId,
    token: OpaquePayload,
}

impl JoinRoom {
    /// Validate and construct a `JoinRoom`. Fails if the protocol version is
    /// zero, the room id is empty/over-long, or the token exceeds the payload
    /// bound.
    pub(crate) fn new(protocol_version: u32, room_id: &[u8], token: &[u8]) -> KernelResult<Self> {
        ProtocolVersion::new(protocol_version).and_then(|protocol_version| {
            RoomId::new(room_id).and_then(|room_id| {
                OpaquePayload::new(token).map(|token| JoinRoom {
                    protocol_version,
                    room_id,
                    token,
                })
            })
        })
    }

    pub(crate) fn protocol_version(&self) -> u32 {
        self.protocol_version.raw()
    }

    pub(crate) fn room_id(&self) -> &[u8] {
        self.room_id.as_bytes()
    }

    pub(crate) fn token(&self) -> &[u8] {
        self.token.as_bytes()
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_JOIN_ROOM);
        self.protocol_version.write_to(&mut w);
        self.room_id.write_to(&mut w);
        self.token.write_to(&mut w);
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_JOIN_ROOM)
            .and_then(|()| ProtocolVersion::read_from(&mut r))
            .and_then(|protocol_version| {
                RoomId::read_from(&mut r).and_then(|room_id| {
                    OpaquePayload::read_from(&mut r).map(|token| JoinRoom {
                        protocol_version,
                        room_id,
                        token,
                    })
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::KernelErrorCode;

    fn sample() -> JoinRoom {
        JoinRoom::new(1, b"lobby", b"token-bytes").unwrap()
    }

    #[test]
    fn accessors_return_the_fields() {
        let m = sample();
        assert_eq!(m.protocol_version(), 1);
        assert_eq!(m.room_id(), b"lobby");
        assert_eq!(m.token(), b"token-bytes");
    }

    #[test]
    fn round_trips() {
        assert_eq!(JoinRoom::decode(&sample().encode()).unwrap(), sample());
    }

    #[test]
    fn empty_token_round_trips() {
        let m = JoinRoom::new(2, b"r", b"").unwrap();
        assert_eq!(JoinRoom::decode(&m.encode()).unwrap(), m);
    }

    #[test]
    fn construction_rejects_invalid_fields() {
        assert_eq!(
            JoinRoom::new(0, b"r", b"").unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
        assert_eq!(
            JoinRoom::new(1, b"", b"").unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        let other = crate::leave_room::LeaveRoom::new(b"r").unwrap().encode();
        assert_eq!(
            JoinRoom::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(
                JoinRoom::decode(&bytes[..k]).is_err(),
                "prefix {k} must fail"
            );
        });
        assert!(JoinRoom::decode(&bytes).is_ok());
    }
}
