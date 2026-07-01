//! `ClientIntentFor` — a per-player-addressed client intent (client → server).
//!
//! This is the per-player twin of [`crate::client_intent::ClientIntent`]: the
//! same intent body, prefixed with the **originating player id** so an authority
//! running one room can fan intents out to the right seat. It is a *new* message
//! kind ([`frame::KIND_CLIENT_INTENT_FOR`]), so the existing anonymous
//! `ClientIntent` and its bytes are untouched.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::frame;
use crate::opaque_payload::OpaquePayload;

/// One unit of client input addressed to a specific player seat. As with
/// `ClientIntent`, the client sends *intents*, never state — the server stays
/// authoritative and decides what an intent does.
///
/// - `player` — the seat the intent originates from, stable within a room.
/// - `client_sequence` — the per-client monotonically increasing id (owned by
///   `axiom-client-core`).
/// - `predicted_client_tick` — the tick the client believed it was on.
/// - `last_seen_server_tick` — the newest authoritative tick the client applied.
/// - `payload` — the opaque, bounded intent body, interpreted only by the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientIntentFor {
    player: u64,
    client_sequence: u64,
    predicted_client_tick: u64,
    last_seen_server_tick: u64,
    payload: OpaquePayload,
}

impl ClientIntentFor {
    /// Validate and construct a `ClientIntentFor`. Fails only if the payload
    /// exceeds the payload bound; every counter accepts any value.
    pub(crate) fn new(
        player: u64,
        client_sequence: u64,
        predicted_client_tick: u64,
        last_seen_server_tick: u64,
        payload: &[u8],
    ) -> KernelResult<Self> {
        OpaquePayload::new(payload).map(|payload| ClientIntentFor {
            player,
            client_sequence,
            predicted_client_tick,
            last_seen_server_tick,
            payload,
        })
    }

    pub(crate) fn player(&self) -> u64 {
        self.player
    }

    pub(crate) fn client_sequence(&self) -> u64 {
        self.client_sequence
    }

    pub(crate) fn predicted_client_tick(&self) -> u64 {
        self.predicted_client_tick
    }

    pub(crate) fn last_seen_server_tick(&self) -> u64 {
        self.last_seen_server_tick
    }

    pub(crate) fn payload(&self) -> &[u8] {
        self.payload.as_bytes()
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_CLIENT_INTENT_FOR);
        w.write_u64(self.player);
        w.write_u64(self.client_sequence);
        w.write_u64(self.predicted_client_tick);
        w.write_u64(self.last_seen_server_tick);
        self.payload.write_to(&mut w);
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_CLIENT_INTENT_FOR)
            .and_then(|()| r.read_u64())
            .and_then(|player| {
                r.read_u64().and_then(|client_sequence| {
                    r.read_u64().and_then(|predicted_client_tick| {
                        r.read_u64().and_then(|last_seen_server_tick| {
                            OpaquePayload::read_from(&mut r).map(|payload| ClientIntentFor {
                                player,
                                client_sequence,
                                predicted_client_tick,
                                last_seen_server_tick,
                                payload,
                            })
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

    fn sample() -> ClientIntentFor {
        ClientIntentFor::new(7, 5, 100, 98, b"move-left").unwrap()
    }

    #[test]
    fn accessors_return_the_fields() {
        let m = sample();
        assert_eq!(m.player(), 7);
        assert_eq!(m.client_sequence(), 5);
        assert_eq!(m.predicted_client_tick(), 100);
        assert_eq!(m.last_seen_server_tick(), 98);
        assert_eq!(m.payload(), b"move-left");
    }

    #[test]
    fn round_trips() {
        assert_eq!(
            ClientIntentFor::decode(&sample().encode()).unwrap(),
            sample()
        );
    }

    #[test]
    fn construction_rejects_an_over_size_payload() {
        let big = vec![0u8; crate::opaque_payload::MAX_PAYLOAD_LEN + 1];
        assert_eq!(
            ClientIntentFor::new(1, 1, 0, 0, &big).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        // The anonymous ClientIntent is a *different* kind: decoding it here fails.
        let other = crate::client_intent::ClientIntent::new(5, 100, 98, b"x")
            .unwrap()
            .encode();
        assert_eq!(
            ClientIntentFor::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(
                ClientIntentFor::decode(&bytes[..k]).is_err(),
                "prefix {k} must fail"
            );
        });
        assert!(ClientIntentFor::decode(&bytes).is_ok());
    }
}
