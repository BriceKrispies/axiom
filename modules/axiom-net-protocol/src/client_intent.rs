//! `ClientIntent` — a client's input for a tick (client → server).

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::frame;
use crate::opaque_payload::OpaquePayload;

/// One unit of client input. The client sends *intents*, never state: the
/// server stays authoritative and decides what an intent does.
///
/// - `client_sequence` — a per-client monotonically increasing id, so the
///   server can acknowledge intents and the client can track which are still
///   pending (owned by `axiom-client-core`).
/// - `predicted_client_tick` — the tick the client believed it was on when it
///   produced the intent (a hint for the server; no prediction is implied yet).
/// - `last_seen_server_tick` — the newest authoritative tick the client has
///   applied, so the server knows how current the client is.
/// - `payload` — the opaque, bounded intent body, interpreted only by the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientIntent {
    client_sequence: u64,
    predicted_client_tick: u64,
    last_seen_server_tick: u64,
    payload: OpaquePayload,
}

impl ClientIntent {
    /// Validate and construct a `ClientIntent`. Fails only if the payload
    /// exceeds the payload bound; the three counters accept any value.
    pub(crate) fn new(
        client_sequence: u64,
        predicted_client_tick: u64,
        last_seen_server_tick: u64,
        payload: &[u8],
    ) -> KernelResult<Self> {
        OpaquePayload::new(payload).map(|payload| ClientIntent {
            client_sequence,
            predicted_client_tick,
            last_seen_server_tick,
            payload,
        })
    }

    /// The per-client monotonically increasing sequence id.
    pub(crate) fn client_sequence(&self) -> u64 {
        self.client_sequence
    }

    /// The client-predicted tick at which the intent was produced.
    pub(crate) fn predicted_client_tick(&self) -> u64 {
        self.predicted_client_tick
    }

    /// The newest authoritative server tick the client had applied.
    pub(crate) fn last_seen_server_tick(&self) -> u64 {
        self.last_seen_server_tick
    }

    /// The opaque intent payload.
    pub(crate) fn payload(&self) -> &[u8] {
        self.payload.as_bytes()
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_CLIENT_INTENT);
        w.write_u64(self.client_sequence);
        w.write_u64(self.predicted_client_tick);
        w.write_u64(self.last_seen_server_tick);
        self.payload.write_to(&mut w);
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_CLIENT_INTENT)
            .and_then(|()| r.read_u64())
            .and_then(|client_sequence| {
                r.read_u64().and_then(|predicted_client_tick| {
                    r.read_u64().and_then(|last_seen_server_tick| {
                        OpaquePayload::read_from(&mut r).map(|payload| ClientIntent {
                            client_sequence,
                            predicted_client_tick,
                            last_seen_server_tick,
                            payload,
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

    fn sample() -> ClientIntent {
        ClientIntent::new(5, 100, 98, b"move-left").unwrap()
    }

    #[test]
    fn accessors_return_the_fields() {
        let m = sample();
        assert_eq!(m.client_sequence(), 5);
        assert_eq!(m.predicted_client_tick(), 100);
        assert_eq!(m.last_seen_server_tick(), 98);
        assert_eq!(m.payload(), b"move-left");
    }

    #[test]
    fn round_trips() {
        assert_eq!(ClientIntent::decode(&sample().encode()).unwrap(), sample());
    }

    #[test]
    fn construction_rejects_an_over_size_payload() {
        let big = vec![0u8; crate::opaque_payload::MAX_PAYLOAD_LEN + 1];
        assert_eq!(
            ClientIntent::new(1, 0, 0, &big).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        let other = crate::leave_room::LeaveRoom::new(b"r").unwrap().encode();
        assert_eq!(
            ClientIntent::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(ClientIntent::decode(&bytes[..k]).is_err(), "prefix {k} must fail");
        });
        assert!(ClientIntent::decode(&bytes).is_ok());
    }
}
