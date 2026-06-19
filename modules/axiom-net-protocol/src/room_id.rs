//! A bounded, non-empty room identifier.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
};

/// The largest room id, in bytes. Room ids are opaque to the protocol (the
/// application chooses their meaning), but they are bounded so a frame cannot
/// carry an unbounded id.
pub(crate) const MAX_ROOM_ID_LEN: usize = 64;

/// An opaque room identifier: a non-empty, length-bounded byte string.
///
/// The protocol does not interpret a room id; it only guarantees it is present
/// (non-empty) and within [`MAX_ROOM_ID_LEN`]. Both bounds are enforced at
/// construction and re-checked on decode, so a malformed id can never enter the
/// system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoomId(Vec<u8>);

impl RoomId {
    /// Construct a room id, rejecting an empty or over-long byte string.
    pub(crate) fn new(bytes: &[u8]) -> KernelResult<Self> {
        let len = bytes.len();
        let valid = (len != 0) & (len <= MAX_ROOM_ID_LEN);
        valid
            .then(|| RoomId(bytes.to_vec()))
            .ok_or_else(|| {
                KernelError::new(
                    KernelErrorScope::Message,
                    KernelErrorCode::OutOfBounds,
                    "room id must be non-empty and within the maximum length",
                )
            })
    }

    /// The room id bytes.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Serialize as a length-prefixed byte slice.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_byte_slice(&self.0);
    }

    /// Read a room id, re-validating its bounds.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        reader.read_byte_slice().and_then(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_empty_bounded_id_is_accepted() {
        assert_eq!(RoomId::new(b"lobby").unwrap().as_bytes(), b"lobby");
    }

    #[test]
    fn empty_id_is_rejected() {
        let err = RoomId::new(b"").unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Message);
        assert_eq!(err.code(), KernelErrorCode::OutOfBounds);
    }

    #[test]
    fn over_long_id_is_rejected() {
        let too_long = vec![b'x'; MAX_ROOM_ID_LEN + 1];
        assert_eq!(
            RoomId::new(&too_long).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn max_length_id_is_accepted() {
        let exact = vec![b'y'; MAX_ROOM_ID_LEN];
        assert_eq!(RoomId::new(&exact).unwrap().as_bytes().len(), MAX_ROOM_ID_LEN);
    }

    #[test]
    fn serialization_round_trips() {
        let id = RoomId::new(b"room-7").unwrap();
        let mut w = BinaryWriter::new();
        id.write_to(&mut w);
        let mut r = BinaryReader::new(w.as_bytes());
        assert_eq!(RoomId::read_from(&mut r).unwrap(), id);
    }

    #[test]
    fn decode_rejects_an_empty_id() {
        let mut w = BinaryWriter::new();
        w.write_byte_slice(b"");
        let mut r = BinaryReader::new(w.as_bytes());
        assert_eq!(
            RoomId::read_from(&mut r).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_truncation() {
        // A length prefix that overruns the buffer.
        let mut w = BinaryWriter::new();
        w.write_u32(10);
        w.write_u8(1);
        let mut r = BinaryReader::new(w.as_bytes());
        assert!(RoomId::read_from(&mut r).is_err());
    }
}
