//! The versioned frame envelope shared by every protocol message.
//!
//! Every message on the wire is prefixed with the same two-part header — a
//! [`SchemaVersion`] then a one-byte **message-kind** discriminant — before its
//! body. This module owns that envelope: the wire version, the stable kind
//! bytes, and the branchless read/write/peek helpers the message modules and the
//! [`crate::net_protocol_api::NetProtocolApi`] facade build on. Keeping the
//! header in one place means the in-memory kind and the on-wire byte can never
//! drift across the seven messages.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
    SchemaVersion,
};

/// The wire-codec format version. Compatibility is by major (see
/// [`SchemaVersion`]): a peer rejects a frame whose major differs from ours.
/// This is the *encoding* version, distinct from the application
/// [`crate::protocol_version::ProtocolVersion`] negotiated inside `JoinRoom` /
/// `Welcome`.
pub(crate) const WIRE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// Stable one-byte message-kind discriminants. The values are contiguous from
/// `0`, which lets [`validate_known_kind`] reject an unknown kind with a single
/// bound check. These bytes are part of the wire contract and must never be
/// renumbered.
pub(crate) const KIND_JOIN_ROOM: u8 = 0;
pub(crate) const KIND_LEAVE_ROOM: u8 = 1;
pub(crate) const KIND_CLIENT_INTENT: u8 = 2;
pub(crate) const KIND_WELCOME: u8 = 3;
pub(crate) const KIND_SERVER_SNAPSHOT: u8 = 4;
pub(crate) const KIND_SERVER_EVENT: u8 = 5;
pub(crate) const KIND_REJECTED_INTENT: u8 = 6;

/// The largest valid kind byte — the upper bound of the contiguous range.
pub(crate) const KIND_MAX: u8 = KIND_REJECTED_INTENT;

/// Write the frame header (`WIRE_VERSION` then `kind`) to `writer`. Each message
/// encoder calls this before writing its body.
pub(crate) fn write_header(writer: &mut BinaryWriter, kind: u8) {
    WIRE_VERSION.write_to(writer);
    writer.write_u8(kind);
}

/// Read and verify the header, requiring the kind to be exactly `expected`.
///
/// Fails with `SchemaVersionMismatch` for an incompatible major or
/// `InvalidDiscriminant` if the kind byte is not `expected` (which includes any
/// unknown kind), threading the reader's `OutOfBounds` for a truncated header.
pub(crate) fn read_expected_kind(reader: &mut BinaryReader<'_>, expected: u8) -> KernelResult<()> {
    read_compatible_version(reader)
        .and_then(|()| reader.read_u8())
        .and_then(|kind| {
            (kind == expected)
                .then_some(())
                .ok_or_else(unknown_kind_error)
        })
}

/// Peek the message kind of an encoded frame without decoding its body: verify
/// the version, read the kind byte, and reject any kind outside the known range.
/// Lets a dispatcher (an app, or the TypeScript package) route a frame to the
/// right decoder.
pub(crate) fn peek_kind(bytes: &[u8]) -> KernelResult<u8> {
    let mut reader = BinaryReader::new(bytes);
    read_compatible_version(&mut reader)
        .and_then(|()| reader.read_u8())
        .and_then(validate_known_kind)
}

/// Read the schema version and require a compatible major.
fn read_compatible_version(reader: &mut BinaryReader<'_>) -> KernelResult<()> {
    SchemaVersion::read_from(reader).and_then(|version| {
        version
            .is_compatible_with(WIRE_VERSION)
            .then_some(())
            .ok_or_else(version_error)
    })
}

/// Accept a kind byte only if it is one of the known, contiguous discriminants.
fn validate_known_kind(kind: u8) -> KernelResult<u8> {
    (kind <= KIND_MAX).then_some(kind).ok_or_else(unknown_kind_error)
}

fn version_error() -> KernelError {
    KernelError::new(
        KernelErrorScope::Binary,
        KernelErrorCode::SchemaVersionMismatch,
        "net-protocol wire version is incompatible with this peer",
    )
}

fn unknown_kind_error() -> KernelError {
    KernelError::new(
        KernelErrorScope::Binary,
        KernelErrorCode::InvalidDiscriminant,
        "net-protocol frame has an unknown or unexpected message kind",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trips_to_the_written_kind() {
        let mut w = BinaryWriter::new();
        write_header(&mut w, KIND_WELCOME);
        let bytes = w.into_bytes();
        assert_eq!(peek_kind(&bytes).unwrap(), KIND_WELCOME);
    }

    #[test]
    fn read_expected_kind_accepts_the_matching_kind() {
        let mut w = BinaryWriter::new();
        write_header(&mut w, KIND_CLIENT_INTENT);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert!(read_expected_kind(&mut r, KIND_CLIENT_INTENT).is_ok());
    }

    #[test]
    fn read_expected_kind_rejects_a_mismatched_kind() {
        let mut w = BinaryWriter::new();
        write_header(&mut w, KIND_JOIN_ROOM);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        let err = read_expected_kind(&mut r, KIND_WELCOME).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Binary);
        assert_eq!(err.code(), KernelErrorCode::InvalidDiscriminant);
    }

    #[test]
    fn peek_kind_rejects_an_unknown_kind() {
        let mut w = BinaryWriter::new();
        WIRE_VERSION.write_to(&mut w);
        w.write_u8(KIND_MAX + 1);
        let err = peek_kind(&w.into_bytes()).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::InvalidDiscriminant);
    }

    #[test]
    fn read_expected_kind_rejects_an_unknown_kind_too() {
        let mut w = BinaryWriter::new();
        WIRE_VERSION.write_to(&mut w);
        w.write_u8(200); // far outside the known range
        let mut r = BinaryReader::new(w.as_bytes());
        assert_eq!(
            read_expected_kind(&mut r, KIND_JOIN_ROOM).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn incompatible_major_is_rejected() {
        let mut w = BinaryWriter::new();
        SchemaVersion::new(WIRE_VERSION.major() + 1, 0).write_to(&mut w);
        w.write_u8(KIND_JOIN_ROOM);
        let bytes = w.into_bytes();
        assert_eq!(
            peek_kind(&bytes).unwrap_err().code(),
            KernelErrorCode::SchemaVersionMismatch
        );
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(
            read_expected_kind(&mut r, KIND_JOIN_ROOM).unwrap_err().code(),
            KernelErrorCode::SchemaVersionMismatch
        );
    }

    #[test]
    fn a_truncated_header_is_rejected() {
        // Empty buffer: the version read fails before any kind byte.
        assert!(peek_kind(&[]).is_err());
        let mut r = BinaryReader::new(&[]);
        assert!(read_expected_kind(&mut r, KIND_JOIN_ROOM).is_err());
    }

    #[test]
    fn the_full_kind_range_peeks_back() {
        // Every known discriminant round-trips through the header + peek.
        let kinds = [
            KIND_JOIN_ROOM,
            KIND_LEAVE_ROOM,
            KIND_CLIENT_INTENT,
            KIND_WELCOME,
            KIND_SERVER_SNAPSHOT,
            KIND_SERVER_EVENT,
            KIND_REJECTED_INTENT,
        ];
        kinds.iter().for_each(|&kind| {
            let mut w = BinaryWriter::new();
            write_header(&mut w, kind);
            assert_eq!(peek_kind(w.as_bytes()).unwrap(), kind);
        });
    }
}
