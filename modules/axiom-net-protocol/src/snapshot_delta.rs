//! A deterministic, byte-exact **sparse byte-patch delta** between two opaque
//! snapshot payloads.
//!
//! Steady-state authoritative snapshots change little tick-to-tick, so shipping a
//! full payload every tick wastes bandwidth. This module computes a compact diff
//! that turns the client's last-acked snapshot (`base`) into the new one (`new`)
//! and reconstructs `new` from `(base, diff)` byte-for-byte. The full snapshot
//! stays the fallback and the keyframe (the first snapshot, or any time the client
//! lacks the matching base) — the delta is purely an optimization layered on top.
//!
//! ## Diff blob layout (all little-endian, base-independent to *decode*)
//! ```text
//!   u32                new_len                 (length of the reconstructed payload)
//!   u32                change_count            (positions that differ within the shared prefix)
//!   change_count reps:
//!     u32              offset                  (index < min(base_len, new_len))
//!     u8               byte                    (new[offset])
//!   u32                tail_len                (length-prefixed byte slice)
//!   u8 * tail_len      tail                    (new[min(base_len,new_len) .. new_len])
//! ```
//! Reconstruct: take `base[..common]` (where `common = min(base_len, new_len)`),
//! overwrite each `offset` with `byte`, then append `tail`. Growth rides in the
//! tail; shrink drops `base`'s excess by taking only the `common` prefix. The
//! diff is byte-exact and deterministic, and shrinks to near-nothing when the
//! payload barely changes — exactly the steady-state replication case.
//!
//! Decoding is fully bounds-checked: an over-`MAX_PAYLOAD_LEN` `new_len`, a
//! change_count beyond the shared prefix, an out-of-range offset, an inconsistent
//! `common + tail_len != new_len`, or a truncated blob each fail with a precise
//! [`KernelError`] rather than panicking. The whole blob rides inside a bounded
//! [`crate::opaque_payload::OpaquePayload`], so it can never exceed `MAX_PAYLOAD_LEN`.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
};

use crate::opaque_payload::MAX_PAYLOAD_LEN;

/// Compute the diff blob that turns `base` into `new` (see the module layout).
pub(crate) fn diff(base: &[u8], new: &[u8]) -> Vec<u8> {
    let common = base.len().min(new.len());
    let changes: Vec<(u32, u8)> = (0..common)
        .filter(|&i| base[i] != new[i])
        .map(|i| (i as u32, new[i]))
        .collect();
    let mut w = BinaryWriter::new();
    w.write_u32(new.len() as u32);
    w.write_u32(changes.len() as u32);
    changes.iter().for_each(|&(offset, byte)| {
        w.write_u32(offset);
        w.write_u8(byte);
    });
    w.write_byte_slice(&new[common..]);
    w.into_bytes()
}

/// Reconstruct the new payload from `base` and a diff `blob` produced by [`diff`].
/// Validates every field; a blob that does not consistently describe a payload
/// `<= MAX_PAYLOAD_LEN` (or that references a base of the wrong shape) is rejected.
pub(crate) fn apply(base: &[u8], blob: &[u8]) -> KernelResult<Vec<u8>> {
    let mut r = BinaryReader::new(blob);
    r.read_u32().and_then(|new_len| {
        let new_len = new_len as usize;
        validate_len(new_len).and_then(|()| {
            let common = base.len().min(new_len);
            read_changes(&mut r, common).and_then(|changes| {
                r.read_byte_slice()
                    .and_then(|tail| build(base, new_len, common, &changes, &tail))
            })
        })
    })
}

/// Reject a reconstructed length beyond the opaque-payload bound.
fn validate_len(new_len: usize) -> KernelResult<()> {
    (new_len <= MAX_PAYLOAD_LEN)
        .then_some(())
        .ok_or_else(|| delta_error("delta declares a payload over the maximum length"))
}

/// Read the change list, rejecting a count beyond the shared prefix and any offset
/// outside `[0, common)`.
fn read_changes(r: &mut BinaryReader<'_>, common: usize) -> KernelResult<Vec<(usize, u8)>> {
    r.read_u32().and_then(|count| {
        (count as usize <= common)
            .then_some(())
            .ok_or_else(|| delta_error("delta declares more changes than the shared prefix holds"))
            .and_then(|()| {
                (0..count).try_fold(Vec::with_capacity(count as usize), |mut acc, _| {
                    r.read_u32().and_then(|offset| {
                        r.read_u8().and_then(|byte| {
                            ((offset as usize) < common)
                                .then(|| {
                                    acc.push((offset as usize, byte));
                                    acc
                                })
                                .ok_or_else(|| delta_error("delta change offset is out of range"))
                        })
                    })
                })
            })
    })
}

/// Apply the validated changes and tail to `base[..common]`, checking that the
/// shared prefix plus the tail reconstructs exactly `new_len` bytes.
fn build(
    base: &[u8],
    new_len: usize,
    common: usize,
    changes: &[(usize, u8)],
    tail: &[u8],
) -> KernelResult<Vec<u8>> {
    (common + tail.len() == new_len)
        .then_some(())
        .ok_or_else(|| delta_error("delta tail length is inconsistent with the declared length"))
        .map(|()| {
            let mut result = base[..common].to_vec();
            changes
                .iter()
                .for_each(|&(offset, byte)| result[offset] = byte);
            result.extend_from_slice(tail);
            result
        })
}

fn delta_error(message: &'static str) -> KernelError {
    KernelError::new(
        KernelErrorScope::Message,
        KernelErrorCode::OutOfBounds,
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(base: &[u8], new: &[u8]) {
        let blob = diff(base, new);
        assert_eq!(
            apply(base, &blob).unwrap(),
            new,
            "base={base:?} new={new:?}"
        );
    }

    #[test]
    fn round_trips_for_equal_changed_grown_shrunk_and_empty() {
        round_trip(b"hello world", b"hello world");
        round_trip(b"hello world", b"hELLo world");
        round_trip(b"hello", b"hello, longer tail");
        round_trip(b"hello, longer tail", b"hi");
        round_trip(b"", b"from empty");
        round_trip(b"to empty", b"");
        round_trip(b"", b"");
    }

    #[test]
    fn an_unchanged_payload_diffs_to_a_tiny_blob() {
        let payload = vec![7u8; 4096];
        let blob = diff(&payload, &payload);
        assert!(
            blob.len() < payload.len(),
            "delta must beat the full payload"
        );
        assert_eq!(apply(&payload, &blob).unwrap(), payload);
    }

    #[test]
    fn apply_rejects_a_truncated_blob() {
        let blob = diff(b"abc", b"abd");
        (0..blob.len()).for_each(|k| {
            assert!(apply(b"abc", &blob[..k]).is_err(), "prefix {k} must fail");
        });
        assert!(apply(b"abc", &blob).is_ok());
    }

    #[test]
    fn apply_rejects_an_over_max_declared_length() {
        let mut w = BinaryWriter::new();
        w.write_u32(MAX_PAYLOAD_LEN as u32 + 1);
        let blob = w.into_bytes();
        assert_eq!(
            apply(b"", &blob).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn apply_rejects_a_change_count_beyond_the_prefix() {
        let mut w = BinaryWriter::new();
        w.write_u32(2);
        w.write_u32(3);
        let blob = w.into_bytes();
        assert!(apply(b"xy", &blob).is_err());
    }

    #[test]
    fn apply_rejects_an_out_of_range_offset() {
        let mut w = BinaryWriter::new();
        w.write_u32(2);
        w.write_u32(1);
        w.write_u32(5);
        w.write_u8(b'Z');
        w.write_byte_slice(b"");
        let blob = w.into_bytes();
        assert_eq!(
            apply(b"xy", &blob).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn apply_rejects_an_inconsistent_tail_length() {
        let mut w = BinaryWriter::new();
        w.write_u32(2);
        w.write_u32(0);
        w.write_byte_slice(b"extra");
        let blob = w.into_bytes();
        assert!(apply(b"xy", &blob).is_err());
    }
}
