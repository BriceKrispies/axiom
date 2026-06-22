//! `LeaveRoom` — a client's intentional departure from a room (client → server).

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::frame;
use crate::room_id::RoomId;

/// A client's notice that it is leaving a room, sent before an intentional
/// disconnect so the server can release the slot promptly rather than waiting
/// for a timeout. It names only the [`RoomId`]; the server already knows the
/// client from its connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeaveRoom {
    room_id: RoomId,
}

impl LeaveRoom {
    /// Validate and construct a `LeaveRoom`. Fails if the room id is empty or
    /// over-long.
    pub(crate) fn new(room_id: &[u8]) -> KernelResult<Self> {
        RoomId::new(room_id).map(|room_id| LeaveRoom { room_id })
    }

    /// The room being left.
    pub(crate) fn room_id(&self) -> &[u8] {
        self.room_id.as_bytes()
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_LEAVE_ROOM);
        self.room_id.write_to(&mut w);
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_LEAVE_ROOM)
            .and_then(|()| RoomId::read_from(&mut r))
            .map(|room_id| LeaveRoom { room_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::KernelErrorCode;

    fn sample() -> LeaveRoom {
        LeaveRoom::new(b"lobby").unwrap()
    }

    #[test]
    fn accessor_returns_the_room() {
        assert_eq!(sample().room_id(), b"lobby");
    }

    #[test]
    fn round_trips() {
        assert_eq!(LeaveRoom::decode(&sample().encode()).unwrap(), sample());
    }

    #[test]
    fn construction_rejects_an_empty_room() {
        assert_eq!(
            LeaveRoom::new(b"").unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        let other = crate::join_room::JoinRoom::new(1, b"r", b"")
            .unwrap()
            .encode();
        assert_eq!(
            LeaveRoom::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(
                LeaveRoom::decode(&bytes[..k]).is_err(),
                "prefix {k} must fail"
            );
        });
        assert!(LeaveRoom::decode(&bytes).is_ok());
    }
}
