//! A bounded, opaque byte payload — the body of intents, snapshots and events.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
};

/// The maximum payload length, in bytes. Every opaque payload the protocol
/// carries — an intent payload, a snapshot payload, an event payload, or a join
/// token — is bounded by this, so a single frame can never declare an unbounded
/// body. The bound is documented here and enforced at construction and decode.
pub(crate) const MAX_PAYLOAD_LEN: usize = 64 * 1024;

/// An opaque, length-bounded byte buffer.
///
/// This is the single representation behind the protocol's three payload roles
/// (`IntentPayload`, `SnapshotPayload`, `EventPayload`) and the optional join
/// token. The protocol does **not** interpret these bytes — a future schema
/// layer will. The only guarantee is the [`MAX_PAYLOAD_LEN`] bound; an empty
/// payload is valid (e.g. an absent token).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpaquePayload(Vec<u8>);

impl OpaquePayload {
    /// Construct a payload, rejecting anything over [`MAX_PAYLOAD_LEN`].
    pub(crate) fn new(bytes: &[u8]) -> KernelResult<Self> {
        (bytes.len() <= MAX_PAYLOAD_LEN)
            .then(|| OpaquePayload(bytes.to_vec()))
            .ok_or_else(|| {
                KernelError::new(
                    KernelErrorScope::Message,
                    KernelErrorCode::OutOfBounds,
                    "opaque payload exceeds the maximum byte length",
                )
            })
    }

    /// The payload bytes.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Serialize as a length-prefixed byte slice.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_byte_slice(&self.0);
    }

    /// Read a payload, re-validating the maximum-length bound.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        reader.read_byte_slice().and_then(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_is_valid() {
        assert_eq!(OpaquePayload::new(b"").unwrap().as_bytes(), b"");
    }

    #[test]
    fn bounded_payload_is_accepted() {
        assert_eq!(OpaquePayload::new(&[1, 2, 3]).unwrap().as_bytes(), &[1, 2, 3]);
    }

    #[test]
    fn max_length_payload_is_accepted() {
        let exact = vec![0u8; MAX_PAYLOAD_LEN];
        assert_eq!(OpaquePayload::new(&exact).unwrap().as_bytes().len(), MAX_PAYLOAD_LEN);
    }

    #[test]
    fn over_max_payload_is_rejected() {
        let too_big = vec![0u8; MAX_PAYLOAD_LEN + 1];
        let err = OpaquePayload::new(&too_big).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Message);
        assert_eq!(err.code(), KernelErrorCode::OutOfBounds);
    }

    #[test]
    fn serialization_round_trips() {
        let p = OpaquePayload::new(&[9, 8, 7]).unwrap();
        let mut w = BinaryWriter::new();
        p.write_to(&mut w);
        let mut r = BinaryReader::new(w.as_bytes());
        assert_eq!(OpaquePayload::read_from(&mut r).unwrap(), p);
    }

    #[test]
    fn decode_rejects_an_over_max_payload() {
        // A well-formed length prefix that exceeds the maximum, with the bytes
        // actually present, so it is the size bound (not truncation) that fires.
        let body = vec![0u8; MAX_PAYLOAD_LEN + 1];
        let mut w = BinaryWriter::new();
        w.write_byte_slice(&body);
        let mut r = BinaryReader::new(w.as_bytes());
        assert_eq!(
            OpaquePayload::read_from(&mut r).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_truncation() {
        let mut w = BinaryWriter::new();
        w.write_u32(8); // declares 8 body bytes
        w.write_u8(1); // but only one present
        let mut r = BinaryReader::new(w.as_bytes());
        assert!(OpaquePayload::read_from(&mut r).is_err());
    }
}
