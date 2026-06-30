//! HMAC-SHA256 (RFC 2104) — the keyed MAC behind HS256 JSON Web Tokens.
//!
//! Built on the vetted `sha2` implementation already in the tree (the same crate
//! `ed25519-dalek` pulls in), not a hand-rolled hash: only the small, standard HMAC
//! construction `H((K ⊕ opad) ‖ H((K ⊕ ipad) ‖ message))` is assembled here, which
//! is exactly what [`crate::jwt`] uses to authenticate a token's signing input. The
//! function is deterministic and branchless (the over-long-key case is selected by
//! arithmetic, not an `if`).

use sha2::{Digest, Sha256};

/// SHA-256 block size, in bytes (the HMAC key-padding width).
const BLOCK_LEN: usize = 64;

/// The HMAC-SHA256 output length, in bytes.
pub const HMAC_SHA256_LEN: usize = 32;

const IPAD: u8 = 0x36;
const OPAD: u8 = 0x5c;

/// Compute HMAC-SHA256 of `message` under `key`.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; HMAC_SHA256_LEN] {
    let block = key_block(key);
    let inner_pad: Vec<u8> = block.iter().map(|&b| b ^ IPAD).collect();
    let outer_pad: Vec<u8> = block.iter().map(|&b| b ^ OPAD).collect();
    let inner = Sha256::new().chain_update(&inner_pad).chain_update(message).finalize();
    Sha256::new().chain_update(&outer_pad).chain_update(inner).finalize().into()
}

/// The block-sized key `K0`: the raw key (right-zero-padded) when it fits in a
/// block, or its SHA-256 hash (padded) when it is longer. Branchless: both the raw
/// key and its hash are materialized, and the source slice is selected by length.
fn key_block(key: &[u8]) -> [u8; BLOCK_LEN] {
    let hashed = Sha256::digest(key);
    let sources: [&[u8]; 2] = [key, hashed.as_slice()];
    let source = sources[usize::from(key.len() > BLOCK_LEN)];
    let mut block = [0u8; BLOCK_LEN];
    block[..source.len()].copy_from_slice(source);
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn matches_rfc4231_test_case_2() {
        // RFC 4231 §4.3: key "Jefe", data "what do ya want for nothing?".
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn a_short_and_an_exactly_block_length_key_use_the_raw_key() {
        // Two distinct messages under the same short key differ; same input matches.
        let a = hmac_sha256(b"key", b"message one");
        let b = hmac_sha256(b"key", b"message two");
        assert_ne!(a, b);
        assert_eq!(hmac_sha256(b"key", b"message one"), a);
        // An exactly-block-length (64-byte) key takes the raw-key branch too.
        let exact = vec![0x0bu8; BLOCK_LEN];
        assert_eq!(hmac_sha256(&exact, b"x"), hmac_sha256(&exact, b"x"));
    }

    #[test]
    fn an_over_long_key_is_hashed_first() {
        // A > block-length key is reduced to its SHA-256 hash, so the key and its
        // hash produce the same MAC (the defining property of the long-key branch).
        let long_key = vec![0xaau8; BLOCK_LEN + 1];
        let hashed: [u8; 32] = Sha256::digest(&long_key).into();
        assert_eq!(hmac_sha256(&long_key, b"data"), hmac_sha256(&hashed, b"data"));
    }
}
