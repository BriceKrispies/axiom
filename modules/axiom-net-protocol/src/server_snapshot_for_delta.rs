//! `ServerSnapshotForDelta` — a per-player-acknowledged **delta** snapshot
//! (server → client).
//!
//! The delta twin of [`crate::server_snapshot_for::ServerSnapshotFor`]: same
//! per-player `(player, sequence)` acks, but the body is a [`crate::snapshot_delta`]
//! diff against the client's last-acked snapshot (identified by `base_tick`) instead
//! of a full payload. A client reconstructs the full new payload from
//! `(its base snapshot, this delta)`. It is a *new* message kind
//! ([`frame::KIND_SERVER_SNAPSHOT_FOR_DELTA`]); the full `ServerSnapshotFor` stays
//! the fallback and the keyframe (the first snapshot, or whenever the diff would not
//! be smaller, or the client lacks the matching base).

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::acks::{read_acks, validate_ack_len, write_acks};
use crate::frame;
use crate::opaque_payload::OpaquePayload;
use crate::snapshot_delta;

/// A per-player-acked authoritative snapshot carried as a diff against `base_tick`.
///
/// - `server_tick` — the authoritative tick this snapshot describes.
/// - `base_tick` — the tick of the snapshot the diff is *against* (the client's
///   last-acked snapshot); a client applies the delta only to that exact base.
/// - `acks` — the bounded `(player, last_accepted_client_sequence)` pairs.
/// - `delta` — the opaque, bounded diff blob ([`crate::snapshot_delta`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerSnapshotForDelta {
    server_tick: u64,
    base_tick: u64,
    acks: Vec<(u64, u64)>,
    delta: OpaquePayload,
}

impl ServerSnapshotForDelta {
    /// Construct by diffing `base_payload` → `new_payload`. Fails if the ack list
    /// exceeds the bound, or if the resulting diff blob would exceed the payload
    /// bound (the signal to fall back to a full `ServerSnapshotFor`).
    pub(crate) fn from_payloads(
        server_tick: u64,
        acks: &[(u64, u64)],
        base_tick: u64,
        base_payload: &[u8],
        new_payload: &[u8],
    ) -> KernelResult<Self> {
        validate_ack_len(acks.len()).and_then(|()| {
            let blob = snapshot_delta::diff(base_payload, new_payload);
            OpaquePayload::new(&blob).map(|delta| ServerSnapshotForDelta {
                server_tick,
                base_tick,
                acks: acks.to_vec(),
                delta,
            })
        })
    }

    /// The authoritative tick this snapshot describes.
    pub(crate) fn server_tick(&self) -> u64 {
        self.server_tick
    }

    /// The tick of the base snapshot this diff is against.
    pub(crate) fn base_tick(&self) -> u64 {
        self.base_tick
    }

    /// The per-player acknowledgements `(player, sequence)`.
    pub(crate) fn acks(&self) -> &[(u64, u64)] {
        &self.acks
    }

    /// The opaque diff blob (apply with [`crate::snapshot_delta::apply`]).
    pub(crate) fn delta(&self) -> &[u8] {
        self.delta.as_bytes()
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_SERVER_SNAPSHOT_FOR_DELTA);
        w.write_u64(self.server_tick);
        w.write_u64(self.base_tick);
        write_acks(&mut w, &self.acks);
        self.delta.write_to(&mut w);
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_SERVER_SNAPSHOT_FOR_DELTA)
            .and_then(|()| r.read_u64())
            .and_then(|server_tick| {
                r.read_u64().and_then(|base_tick| {
                    read_acks(&mut r).and_then(|acks| {
                        OpaquePayload::read_from(&mut r).map(|delta| ServerSnapshotForDelta {
                            server_tick,
                            base_tick,
                            acks,
                            delta,
                        })
                    })
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::KernelErrorCode;
    use crate::opaque_payload::MAX_PAYLOAD_LEN;

    const BASE: &[u8] = b"the authoritative snapshot payload, tick N";
    const NEW: &[u8] = b"the authoritative snapshot PAYLOAD, tick N+1!";

    fn sample() -> ServerSnapshotForDelta {
        ServerSnapshotForDelta::from_payloads(43, &[(7, 5), (9, 3)], 42, BASE, NEW).unwrap()
    }

    #[test]
    fn accessors_return_the_fields() {
        let m = sample();
        assert_eq!(m.server_tick(), 43);
        assert_eq!(m.base_tick(), 42);
        assert_eq!(m.acks(), &[(7, 5), (9, 3)]);
        assert_eq!(snapshot_delta::apply(BASE, m.delta()).unwrap(), NEW);
    }

    #[test]
    fn round_trips_and_reconstructs_the_full_payload() {
        let decoded = ServerSnapshotForDelta::decode(&sample().encode()).unwrap();
        assert_eq!(decoded, sample());
        assert_eq!(snapshot_delta::apply(BASE, decoded.delta()).unwrap(), NEW);
    }

    #[test]
    fn round_trips_with_an_empty_ack_list() {
        let m = ServerSnapshotForDelta::from_payloads(1, &[], 0, b"", b"x").unwrap();
        let decoded = ServerSnapshotForDelta::decode(&m.encode()).unwrap();
        assert_eq!(decoded, m);
        assert_eq!(decoded.acks(), &[] as &[(u64, u64)]);
        assert_eq!(snapshot_delta::apply(b"", decoded.delta()).unwrap(), b"x");
    }

    #[test]
    fn construction_rejects_too_many_acks() {
        let acks: Vec<(u64, u64)> = (0..=crate::acks::MAX_ACKS as u64).map(|p| (p, p)).collect();
        assert_eq!(
            ServerSnapshotForDelta::from_payloads(0, &acks, 0, b"", b"")
                .unwrap_err()
                .code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn construction_rejects_an_over_size_diff_blob() {
        let big = vec![0u8; MAX_PAYLOAD_LEN];
        assert_eq!(
            ServerSnapshotForDelta::from_payloads(0, &[], 0, b"", &big)
                .unwrap_err()
                .code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        let other = crate::server_snapshot_for::ServerSnapshotFor::new(1, &[], b"")
            .unwrap()
            .encode();
        assert_eq!(
            ServerSnapshotForDelta::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(
                ServerSnapshotForDelta::decode(&bytes[..k]).is_err(),
                "prefix {k} must fail"
            );
        });
        assert!(ServerSnapshotForDelta::decode(&bytes).is_ok());
    }

    #[test]
    fn cross_language_byte_parity_fixture() {
        // The exact bytes the `@axiom/client` TS twin must reproduce
        // (`snapshot-delta.test.ts` asserts the identical literal).
        let bytes =
            ServerSnapshotForDelta::from_payloads(43, &[(1, 9)], 42, b"abc", b"abd")
                .unwrap()
                .encode();
        let expected: &[u8] = &[
            1, 0, 0, 0, 9, 43, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0,
            0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 17, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0,
            100, 0, 0, 0, 0,
        ];
        assert_eq!(bytes, expected);
    }
}
