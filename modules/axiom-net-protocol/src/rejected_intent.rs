//! `RejectedIntent` — the server's refusal of one client intent (server → client).

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::frame;

/// The server's refusal of a single client intent, identified by its
/// `client_sequence`. The `reason_code` is **machine-readable** (a stable
/// `u32`), never a human string, so the client can react deterministically. The
/// client drops exactly the named pending intent (owned by `axiom-client-core`).
///
/// Reason codes are an open `u32` space; a few well-known values are defined as
/// constants and re-exported on the facade. `0` is reserved as "unspecified".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RejectedIntent {
    client_sequence: u64,
    reason_code: u32,
}

/// Reason: unspecified / generic refusal.
pub(crate) const REASON_UNSPECIFIED: u32 = 0;
/// Reason: the intent was malformed or violated the protocol.
pub(crate) const REASON_MALFORMED: u32 = 1;
/// Reason: the intent arrived out of order or too late to apply.
pub(crate) const REASON_OUT_OF_ORDER: u32 = 2;
/// Reason: the client is not a member of the room it addressed.
pub(crate) const REASON_NOT_IN_ROOM: u32 = 3;

impl RejectedIntent {
    /// Construct a `RejectedIntent`. Always succeeds: any sequence and any
    /// machine-readable reason code is representable.
    pub(crate) fn new(client_sequence: u64, reason_code: u32) -> Self {
        RejectedIntent {
            client_sequence,
            reason_code,
        }
    }

    /// The sequence id of the refused intent.
    pub(crate) fn client_sequence(&self) -> u64 {
        self.client_sequence
    }

    /// The machine-readable refusal reason.
    pub(crate) fn reason_code(&self) -> u32 {
        self.reason_code
    }

    /// Encode to a complete wire frame (header + body).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        frame::write_header(&mut w, frame::KIND_REJECTED_INTENT);
        w.write_u64(self.client_sequence);
        w.write_u32(self.reason_code);
        w.into_bytes()
    }

    /// Decode a complete wire frame previously produced by [`Self::encode`].
    pub(crate) fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        frame::read_expected_kind(&mut r, frame::KIND_REJECTED_INTENT)
            .and_then(|()| r.read_u64())
            .and_then(|client_sequence| {
                r.read_u32().map(|reason_code| RejectedIntent {
                    client_sequence,
                    reason_code,
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::KernelErrorCode;

    fn sample() -> RejectedIntent {
        RejectedIntent::new(5, REASON_OUT_OF_ORDER)
    }

    #[test]
    fn accessors_return_the_fields() {
        let m = sample();
        assert_eq!(m.client_sequence(), 5);
        assert_eq!(m.reason_code(), REASON_OUT_OF_ORDER);
    }

    #[test]
    fn round_trips() {
        assert_eq!(RejectedIntent::decode(&sample().encode()).unwrap(), sample());
    }

    #[test]
    fn encodes_to_the_cross_language_golden_bytes() {
        // This exact byte vector is also asserted by the TypeScript package's
        // protocol test, locking the two codecs to one wire format:
        // version major=1, minor=0, kind=6, client_sequence=5 (u64 LE),
        // reason_code=2 (u32 LE).
        let bytes = RejectedIntent::new(5, REASON_OUT_OF_ORDER).encode();
        assert_eq!(
            bytes,
            vec![1, 0, 0, 0, 6, 5, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0]
        );
    }

    #[test]
    fn well_known_reason_codes_are_distinct() {
        let codes = [
            REASON_UNSPECIFIED,
            REASON_MALFORMED,
            REASON_OUT_OF_ORDER,
            REASON_NOT_IN_ROOM,
        ];
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len());
    }

    #[test]
    fn decode_rejects_a_wrong_kind() {
        let other = crate::server_event::ServerEvent::new(1, b"").unwrap().encode();
        assert_eq!(
            RejectedIntent::decode(&other).unwrap_err().code(),
            KernelErrorCode::InvalidDiscriminant
        );
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = sample().encode();
        (0..bytes.len()).for_each(|k| {
            assert!(RejectedIntent::decode(&bytes[..k]).is_err(), "prefix {k} must fail");
        });
        assert!(RejectedIntent::decode(&bytes).is_ok());
    }
}
