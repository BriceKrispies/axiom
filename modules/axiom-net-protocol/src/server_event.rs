//! `ServerEvent` — a discrete authoritative event for a tick (server → client).

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::frame;
use crate::opaque_payload::OpaquePayload;

/// A discrete authoritative event the server pushes outside the steady snapshot
/// stream (e.g. a one-shot notification). Like a snapshot it is authoritative
/// and applied as data; unlike a snapshot it carries no acknowledgement cursor.
///
/// - `server_tick` — the authoritative tick the event belongs to.
/// - `payload` — the opaque, bounded event body, interpreted only by the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerEvent {
    server_tick: u64,
    payload: OpaquePayload,
}

impl ServerEvent {
    /// Validate and construct a `ServerEvent`. Fails only if the payload
    /// exceeds the payload bound.
    pub(crate) fn new(server_tick: u64, payload: &[u8]) -> KernelResult<Self> {
        OpaquePayload::new(payload).map(|payload| ServerEvent {
            server_tick,
            payload,
        })
    }

    pub(crate) fn server_tick(&self) -> u64 {
        self.server_tick
    }

    pub(crate) fn payload(&self) -> &[u8] {
        self.payload.as_bytes()
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_SERVER_EVENT);
        w.write_u64(self.server_tick);
        self.payload.write_to(&mut w);
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_SERVER_EVENT)
            .and_then(|()| r.read_u64())
            .and_then(|server_tick| {
                OpaquePayload::read_from(&mut r).map(|payload| ServerEvent {
                    server_tick,
                    payload,
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::KernelErrorCode;

    fn sample() -> ServerEvent {
        ServerEvent::new(9, b"event-bytes").unwrap()
    }

    #[test]
    fn accessors_return_the_fields() {
        let m = sample();
        assert_eq!(m.server_tick(), 9);
        assert_eq!(m.payload(), b"event-bytes");
    }

    #[test]
    fn round_trips() {
        assert_eq!(ServerEvent::decode(&sample().encode()).unwrap(), sample());
    }

    #[test]
    fn construction_rejects_an_over_size_payload() {
        let big = vec![0u8; crate::opaque_payload::MAX_PAYLOAD_LEN + 1];
        assert_eq!(
            ServerEvent::new(0, &big).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        let other = crate::server_snapshot::ServerSnapshot::new(1, 1, b"")
            .unwrap()
            .encode();
        assert_eq!(
            ServerEvent::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(
                ServerEvent::decode(&bytes[..k]).is_err(),
                "prefix {k} must fail"
            );
        });
        assert!(ServerEvent::decode(&bytes).is_ok());
    }
}
