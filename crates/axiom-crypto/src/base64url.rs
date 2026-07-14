//! Unpadded base64url decoding (RFC 4648 §5) — the encoding compact JWS uses for
//! its `header.payload.signature` segments.
//! This is *encoding*, not cryptography: it turns the ASCII base64url segments of a
//! JWT into their raw bytes so [`crate::jwt`] can HMAC-verify the signing input and
//! read the claims. It is deterministic and branchless: each input byte is mapped
//! to its 6-bit value by arithmetic (no lookup table built with a loop), an invalid
//! byte or an impossible length (`len % 4 == 1`) yields `None`, and the 6-bit
//! groups are folded into output bytes through a bit accumulator.

/// Decode unpadded base64url `input` to its raw bytes, or `None` if it contains a
/// non-alphabet byte or has an impossible length (`len % 4 == 1`).
pub(crate) fn decode(input: &[u8]) -> Option<Vec<u8>> {
    let length_ok = input.len() % 4 != 1;
    let decoded = input.iter().fold(Accumulator::default(), |state, &byte| {
        state.push(decode_byte(byte))
    });
    (length_ok & !decoded.bad).then_some(decoded.out)
}

/// Map one ASCII byte to its 6-bit base64url value, or `0xFF` if it is not in the
/// alphabet. Pure arithmetic over mutually-exclusive class flags — no branches.
fn decode_byte(byte: u8) -> u8 {
    let upper = u8::from((byte >= b'A') & (byte <= b'Z'));
    let lower = u8::from((byte >= b'a') & (byte <= b'z'));
    let digit = u8::from((byte >= b'0') & (byte <= b'9'));
    let dash = u8::from(byte == b'-');
    let under = u8::from(byte == b'_');
    let value = upper * byte.wrapping_sub(b'A')
        + lower * byte.wrapping_sub(b'a').wrapping_add(26)
        + digit * byte.wrapping_sub(b'0').wrapping_add(52)
        + dash * 62
        + under * 63;
    let valid = upper | lower | digit | dash | under;
    // valid -> value (<= 63); invalid -> 0xFF. `value` is 0 when invalid, so the OR
    // selects cleanly without a branch or an index.
    value | (0xFFu8 * (1 - valid))
}

/// A 6-bit-group → byte bit accumulator. `bad` latches if any sextet was invalid.
#[derive(Default)]
struct Accumulator {
    acc: u32,
    bits: u32,
    out: Vec<u8>,
    bad: bool,
}

impl Accumulator {
    /// Fold one 6-bit `sextet` in, emitting a byte whenever 8 bits are available.
    fn push(mut self, sextet: u8) -> Self {
        self.bad |= (sextet & 0xC0) != 0;
        self.acc = (self.acc << 6) | (u32::from(sextet) & 0x3F);
        self.bits += 6;
        let emit = self.bits >= 8;
        let residual = self.bits - 8 * u32::from(emit);
        // When emitting, the top 8 bits are the next output byte; when not, this is
        // 0 (the accumulator holds only `bits` < 8 significant bits) and is dropped.
        let byte = ((self.acc >> residual) & 0xFF) as u8;
        emit.then(|| self.out.push(byte));
        self.bits = residual;
        self.acc &= (1u32 << self.bits).wrapping_sub(1);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A branchy, test-only encoder used to mint inputs whose decode we then check.
    fn encode(input: &[u8]) -> Vec<u8> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = Vec::new();
        let mut acc = 0u32;
        let mut bits = 0u32;
        for &byte in input {
            acc = (acc << 8) | u32::from(byte);
            bits += 8;
            while bits >= 6 {
                bits -= 6;
                out.push(ALPHABET[((acc >> bits) & 0x3F) as usize]);
            }
        }
        if bits > 0 {
            out.push(ALPHABET[((acc << (6 - bits)) & 0x3F) as usize]);
        }
        out
    }

    #[test]
    fn round_trips_every_length_class() {
        for len in 0..40usize {
            let raw: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let encoded = encode(&raw);
            assert_eq!(
                decode(&encoded).as_deref(),
                Some(raw.as_slice()),
                "len {len}"
            );
        }
    }

    #[test]
    fn decodes_each_alphabet_class() {
        assert_eq!(decode_byte(b'A'), 0);
        assert_eq!(decode_byte(b'Z'), 25);
        assert_eq!(decode_byte(b'a'), 26);
        assert_eq!(decode_byte(b'z'), 51);
        assert_eq!(decode_byte(b'0'), 52);
        assert_eq!(decode_byte(b'9'), 61);
        assert_eq!(decode_byte(b'-'), 62);
        assert_eq!(decode_byte(b'_'), 63);
        assert_eq!(decode_byte(b'+'), 0xFF);
        assert_eq!(decode_byte(b'/'), 0xFF);
        assert_eq!(decode_byte(b'='), 0xFF); // padding is rejected (unpadded)
    }

    #[test]
    fn rejects_a_non_alphabet_byte() {
        assert!(decode(b"AAAA").is_some());
        assert!(decode(b"AA+A").is_none());
    }

    #[test]
    fn rejects_an_impossible_length() {
        // len % 4 == 1 is not a producible base64 length.
        assert!(decode(b"A").is_none());
        assert!(decode(b"AAAAA").is_none());
        assert!(decode(b"").is_some());
        assert!(decode(b"AA").is_some());
        assert!(decode(b"AAA").is_some());
    }
}
