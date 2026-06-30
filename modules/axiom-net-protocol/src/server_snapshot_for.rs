//! `ServerSnapshotFor` — a per-player-acknowledged authoritative snapshot
//! (server → client).
//!
//! This is the per-player twin of [`crate::server_snapshot::ServerSnapshot`]: the
//! same opaque snapshot body, carrying a **bounded list of per-player
//! acknowledgements** `(player, sequence)` instead of a single anonymous acked
//! sequence, so a client running one of several seats learns which of *its* own
//! intents the authority accepted. It is a *new* message kind
//! ([`frame::KIND_SERVER_SNAPSHOT_FOR`]); the anonymous `ServerSnapshot` bytes are
//! untouched.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::acks::{read_acks, validate_ack_len, write_acks};
use crate::frame;
use crate::opaque_payload::OpaquePayload;

/// The maximum number of per-player acknowledgements a single snapshot may carry —
/// re-exported from the shared [`crate::acks`] framing both per-player snapshots use.
pub(crate) use crate::acks::MAX_ACKS;

/// The server's authoritative state for a tick, acknowledged per player.
///
/// - `server_tick` — the authoritative tick this snapshot describes.
/// - `acks` — the bounded `(player, last_accepted_client_sequence)` pairs, so each
///   client can drop its own acknowledged pending intents (owned by
///   `axiom-client-core`).
/// - `payload` — the opaque, bounded snapshot body, interpreted only by the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerSnapshotFor {
    server_tick: u64,
    acks: Vec<(u64, u64)>,
    payload: OpaquePayload,
}

impl ServerSnapshotFor {
    /// Validate and construct a `ServerSnapshotFor`. Fails if the ack list
    /// exceeds [`MAX_ACKS`] or the payload exceeds the payload bound.
    pub(crate) fn new(
        server_tick: u64,
        acks: &[(u64, u64)],
        payload: &[u8],
    ) -> KernelResult<Self> {
        validate_ack_len(acks.len())
            .and_then(|()| OpaquePayload::new(payload))
            .map(|payload| ServerSnapshotFor {
                server_tick,
                acks: acks.to_vec(),
                payload,
            })
    }

    /// The authoritative tick this snapshot describes.
    pub(crate) fn server_tick(&self) -> u64 {
        self.server_tick
    }

    /// The per-player acknowledgements `(player, sequence)`.
    pub(crate) fn acks(&self) -> &[(u64, u64)] {
        &self.acks
    }

    /// The opaque snapshot payload.
    pub(crate) fn payload(&self) -> &[u8] {
        self.payload.as_bytes()
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_SERVER_SNAPSHOT_FOR);
        w.write_u64(self.server_tick);
        write_acks(&mut w, &self.acks);
        self.payload.write_to(&mut w);
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_SERVER_SNAPSHOT_FOR)
            .and_then(|()| r.read_u64())
            .and_then(|server_tick| {
                read_acks(&mut r).and_then(|acks| {
                    OpaquePayload::read_from(&mut r).map(|payload| ServerSnapshotFor {
                        server_tick,
                        acks,
                        payload,
                    })
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{KernelErrorCode, KernelErrorScope};

    fn sample() -> ServerSnapshotFor {
        ServerSnapshotFor::new(42, &[(7, 5), (9, 3)], b"state-bytes").unwrap()
    }

    #[test]
    fn accessors_return_the_fields() {
        let m = sample();
        assert_eq!(m.server_tick(), 42);
        assert_eq!(m.acks(), &[(7, 5), (9, 3)]);
        assert_eq!(m.payload(), b"state-bytes");
    }

    #[test]
    fn round_trips_with_multiple_acks() {
        assert_eq!(
            ServerSnapshotFor::decode(&sample().encode()).unwrap(),
            sample()
        );
    }

    #[test]
    fn round_trips_with_an_empty_ack_list() {
        let m = ServerSnapshotFor::new(1, &[], b"").unwrap();
        let decoded = ServerSnapshotFor::decode(&m.encode()).unwrap();
        assert_eq!(decoded, m);
        assert_eq!(decoded.acks(), &[] as &[(u64, u64)]);
    }

    #[test]
    fn round_trips_at_the_max_ack_count() {
        let acks: Vec<(u64, u64)> = (0..MAX_ACKS as u64).map(|p| (p, p + 1)).collect();
        let m = ServerSnapshotFor::new(3, &acks, b"x").unwrap();
        assert_eq!(ServerSnapshotFor::decode(&m.encode()).unwrap(), m);
    }

    #[test]
    fn construction_rejects_an_over_size_payload() {
        let big = vec![0u8; crate::opaque_payload::MAX_PAYLOAD_LEN + 1];
        assert_eq!(
            ServerSnapshotFor::new(0, &[], &big).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn construction_rejects_too_many_acks() {
        let acks: Vec<(u64, u64)> = (0..=MAX_ACKS as u64).map(|p| (p, p)).collect();
        let err = ServerSnapshotFor::new(0, &acks, b"").unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Message);
        assert_eq!(err.code(), KernelErrorCode::OutOfBounds);
    }

    #[test]
    fn decode_rejects_a_declared_ack_count_over_the_bound() {
        // Hand-build a frame whose declared ack count exceeds MAX_ACKS; the
        // bound must fire before any allocation or pair read.
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_SERVER_SNAPSHOT_FOR);
        w.write_u64(0); // server_tick
        w.write_u32(MAX_ACKS as u32 + 1); // too many acks declared
        assert_eq!(
            ServerSnapshotFor::decode(w.as_bytes()).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        let other = crate::server_snapshot::ServerSnapshot::new(1, 0, b"")
            .unwrap()
            .encode();
        assert_eq!(
            ServerSnapshotFor::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(
                ServerSnapshotFor::decode(&bytes[..k]).is_err(),
                "prefix {k} must fail"
            );
        });
        assert!(ServerSnapshotFor::decode(&bytes).is_ok());
    }
}
