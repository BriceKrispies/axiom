//! Shared per-player acknowledgement-list framing.
//!
//! Both per-player server→client snapshots — the full
//! [`crate::server_snapshot_for::ServerSnapshotFor`] and the delta
//! [`crate::server_snapshot_for_delta::ServerSnapshotForDelta`] — carry the same
//! bounded list of `(player, sequence)` acknowledgements, so a client running one
//! of several seats learns which of *its* intents the authority accepted. This
//! module owns that one encoding (count-prefixed, bounded by [`MAX_ACKS`]) so the
//! two frames can never drift in how they frame acks — the lowest correct place
//! for a shape both messages share.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
};

/// The maximum number of per-player acknowledgements a single snapshot may carry.
/// A snapshot acks at most one sequence per seated player, so this bounds the ack
/// list the way [`crate::opaque_payload::MAX_PAYLOAD_LEN`] bounds the body — a
/// frame can never declare an unbounded list. Enforced at construction and decode.
pub(crate) const MAX_ACKS: usize = 4096;

/// Reject an ack list longer than [`MAX_ACKS`], branchlessly.
pub(crate) fn validate_ack_len(len: usize) -> KernelResult<()> {
    (len <= MAX_ACKS).then_some(()).ok_or_else(too_many_acks_error)
}

/// Write the count-prefixed `(player, sequence)` ack list.
pub(crate) fn write_acks(writer: &mut BinaryWriter, acks: &[(u64, u64)]) {
    writer.write_u32(acks.len() as u32);
    acks.iter().for_each(|&(player, sequence)| {
        writer.write_u64(player);
        writer.write_u64(sequence);
    });
}

/// Read the length-prefixed ack list, re-validating the [`MAX_ACKS`] bound and
/// folding the declared count of `(player, sequence)` pairs without a loop.
pub(crate) fn read_acks(reader: &mut BinaryReader<'_>) -> KernelResult<Vec<(u64, u64)>> {
    reader.read_u32().and_then(|count| {
        validate_ack_len(count as usize).and_then(|()| {
            (0..count).try_fold(Vec::with_capacity(count as usize), |mut acc, _| {
                reader.read_u64().and_then(|player| {
                    reader.read_u64().map(|sequence| {
                        acc.push((player, sequence));
                        acc
                    })
                })
            })
        })
    })
}

fn too_many_acks_error() -> KernelError {
    KernelError::new(
        KernelErrorScope::Message,
        KernelErrorCode::OutOfBounds,
        "server snapshot ack list exceeds the maximum count",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(acks: &[(u64, u64)]) -> Vec<(u64, u64)> {
        let mut w = BinaryWriter::new();
        write_acks(&mut w, acks);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        read_acks(&mut r).unwrap()
    }

    #[test]
    fn acks_round_trip_including_empty_and_max() {
        assert_eq!(round_trip(&[]), Vec::<(u64, u64)>::new());
        assert_eq!(round_trip(&[(7, 5), (9, 3)]), vec![(7, 5), (9, 3)]);
        let full: Vec<(u64, u64)> = (0..MAX_ACKS as u64).map(|p| (p, p + 1)).collect();
        assert_eq!(round_trip(&full), full);
    }

    #[test]
    fn validate_rejects_an_over_bound_length() {
        assert!(validate_ack_len(MAX_ACKS).is_ok());
        let err = validate_ack_len(MAX_ACKS + 1).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Message);
        assert_eq!(err.code(), KernelErrorCode::OutOfBounds);
    }

    #[test]
    fn read_rejects_a_declared_count_over_the_bound_before_allocating() {
        let mut w = BinaryWriter::new();
        w.write_u32(MAX_ACKS as u32 + 1);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(
            read_acks(&mut r).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn read_rejects_truncation_mid_list() {
        let mut w = BinaryWriter::new();
        w.write_u32(2); // declares two acks
        w.write_u64(1); // only a partial first ack present
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert!(read_acks(&mut r).is_err());
    }
}
