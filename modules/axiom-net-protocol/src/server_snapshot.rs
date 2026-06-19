//! `ServerSnapshot` — an authoritative state snapshot for a tick (server → client).

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::frame;
use crate::opaque_payload::OpaquePayload;

/// The server's authoritative state for a tick. Snapshots — not client opinion —
/// are the source of truth: the client applies them as data.
///
/// - `server_tick` — the authoritative tick this snapshot describes.
/// - `last_accepted_client_sequence` — the newest client intent the server has
///   accepted from this client, so the client can drop acknowledged pending
///   intents (owned by `axiom-client-core`).
/// - `payload` — the opaque, bounded snapshot body, interpreted only by the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerSnapshot {
    server_tick: u64,
    last_accepted_client_sequence: u64,
    payload: OpaquePayload,
}

impl ServerSnapshot {
    /// Validate and construct a `ServerSnapshot`. Fails only if the payload
    /// exceeds the payload bound.
    pub(crate) fn new(
        server_tick: u64,
        last_accepted_client_sequence: u64,
        payload: &[u8],
    ) -> KernelResult<Self> {
        OpaquePayload::new(payload).map(|payload| ServerSnapshot {
            server_tick,
            last_accepted_client_sequence,
            payload,
        })
    }

    /// The authoritative tick this snapshot describes.
    pub(crate) fn server_tick(&self) -> u64 {
        self.server_tick
    }

    /// The newest client intent the server has accepted.
    pub(crate) fn last_accepted_client_sequence(&self) -> u64 {
        self.last_accepted_client_sequence
    }

    /// The opaque snapshot payload.
    pub(crate) fn payload(&self) -> &[u8] {
        self.payload.as_bytes()
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_SERVER_SNAPSHOT);
        w.write_u64(self.server_tick);
        w.write_u64(self.last_accepted_client_sequence);
        self.payload.write_to(&mut w);
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_SERVER_SNAPSHOT)
            .and_then(|()| r.read_u64())
            .and_then(|server_tick| {
                r.read_u64().and_then(|last_accepted_client_sequence| {
                    OpaquePayload::read_from(&mut r).map(|payload| ServerSnapshot {
                        server_tick,
                        last_accepted_client_sequence,
                        payload,
                    })
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::KernelErrorCode;

    fn sample() -> ServerSnapshot {
        ServerSnapshot::new(42, 5, b"state-bytes").unwrap()
    }

    #[test]
    fn accessors_return_the_fields() {
        let m = sample();
        assert_eq!(m.server_tick(), 42);
        assert_eq!(m.last_accepted_client_sequence(), 5);
        assert_eq!(m.payload(), b"state-bytes");
    }

    #[test]
    fn round_trips() {
        assert_eq!(ServerSnapshot::decode(&sample().encode()).unwrap(), sample());
    }

    #[test]
    fn construction_rejects_an_over_size_payload() {
        let big = vec![0u8; crate::opaque_payload::MAX_PAYLOAD_LEN + 1];
        assert_eq!(
            ServerSnapshot::new(0, 0, &big).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        let other = crate::server_event::ServerEvent::new(1, b"").unwrap().encode();
        assert_eq!(
            ServerSnapshot::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(ServerSnapshot::decode(&bytes[..k]).is_err(), "prefix {k} must fail");
        });
        assert!(ServerSnapshot::decode(&bytes).is_ok());
    }
}
