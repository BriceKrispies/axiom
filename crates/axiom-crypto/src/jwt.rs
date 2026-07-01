//! Compact JWS / JSON Web Token verification, HS256 only (RFC 7519 / 7515).
//! A host-admission seam: an authority verifies a join token's **signature** and
//! **expiry** against a configured shared secret before admitting a player. It is
//! pure and deterministic — the current time is *injected* (`now_unix_secs`), never
//! read from a wall clock here — so it stays branchless, fully testable, and free of
//! the entropy/clock nondeterminism the rest of this layer also keeps at the edge.
//! It verifies one thing and trusts nothing in the header: the algorithm is fixed to
//! HS256 (the token's `alg` field is never consulted), so an `alg: none` /
//! algorithm-confusion token is simply HMAC-checked and rejected — it cannot bypass
//! verification. Every failure is a typed [`JwtError`] value, never a panic:
//! * [`JwtError::Malformed`] — not three dot-separated segments, a non-base64url
//!   segment, claims that are not JSON, or claims lacking an integer `exp`;
//! * [`JwtError::BadSignature`] — the HMAC-SHA256 over `header.payload` does not
//!   match the signature segment (constant-time compared);
//! * [`JwtError::Expired`] — `exp` is strictly before `now_unix_secs`.

use crate::base64url;
use crate::hmac_sha256::hmac_sha256;

/// Why a token was not admitted. A normal value surfaced to the caller (which maps
/// it to a connection rejection), never a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwtError {
    /// Structurally invalid: wrong segment count, non-base64url, non-JSON claims,
    /// or claims without an integer `exp`.
    Malformed,
    /// The signature does not match the signing input under the secret.
    BadSignature,
    /// The token's `exp` is before the supplied current time.
    Expired,
}

/// The verified, accepted claims of a token: the expiry and (optionally) the
/// subject. Only fields the admission seam needs are surfaced; the raw claim set is
/// intentionally not widened into this layer's contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JwtClaims {
    exp: u64,
    subject: Option<String>,
}

impl JwtClaims {
    /// The token's expiry, in Unix seconds.
    pub fn expiry_unix_secs(&self) -> u64 {
        self.exp
    }

    /// The token's `sub` (subject) claim, if present.
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }
}

/// Verify an HS256 compact JWS `token` against `secret`, treating `now_unix_secs` as
/// the current time. Returns the accepted [`JwtClaims`], or a [`JwtError`] for an
/// invalid, mis-signed, or expired token.
pub fn verify_jwt_hs256(
    token: &[u8],
    secret: &[u8],
    now_unix_secs: u64,
) -> Result<JwtClaims, JwtError> {
    split_signature(token)
        .ok_or(JwtError::Malformed)
        .and_then(|(signing_input, signature_b64)| {
            payload_segment(signing_input)
                .ok_or(JwtError::Malformed)
                .and_then(|payload_b64| {
                    verify_signature(secret, signing_input, signature_b64)
                        .and_then(|()| decode_claims(payload_b64))
                        .and_then(|claims| check_expiry(claims, now_unix_secs))
                })
        })
}

/// Split `token` into `(signing_input, signature_b64)` at the **last** `.`.
fn split_signature(token: &[u8]) -> Option<(&[u8], &[u8])> {
    token
        .iter()
        .rposition(|&b| b == b'.')
        .and_then(|index| token.get(..index).zip(token.get(index + 1..)))
}

/// The payload segment of a `header.payload` signing input — present only when the
/// signing input contains exactly one `.` (so the whole token had exactly three
/// segments).
fn payload_segment(signing_input: &[u8]) -> Option<&[u8]> {
    let dots = signing_input.iter().filter(|&&b| b == b'.').count();
    (dots == 1).then_some(()).and_then(|()| {
        signing_input
            .iter()
            .position(|&b| b == b'.')
            .and_then(|index| signing_input.get(index + 1..))
    })
}

/// Verify the HMAC-SHA256 of `signing_input` under `secret` matches the decoded
/// signature, in constant time.
fn verify_signature(
    secret: &[u8],
    signing_input: &[u8],
    signature_b64: &[u8],
) -> Result<(), JwtError> {
    base64url::decode(signature_b64)
        .ok_or(JwtError::Malformed)
        .and_then(|signature| {
            let mac = hmac_sha256(secret, signing_input);
            constant_time_eq(&signature, &mac)
                .then_some(())
                .ok_or(JwtError::BadSignature)
        })
}

/// Decode the base64url claims segment and parse its JSON into [`JwtClaims`].
fn decode_claims(payload_b64: &[u8]) -> Result<JwtClaims, JwtError> {
    base64url::decode(payload_b64)
        .ok_or(JwtError::Malformed)
        .and_then(|claims_bytes| parse_claims(&claims_bytes).ok_or(JwtError::Malformed))
}

/// Parse the claims JSON, requiring an integer `exp`; reads the optional `sub`.
fn parse_claims(bytes: &[u8]) -> Option<JwtClaims> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| {
            value
                .get("exp")
                .and_then(serde_json::Value::as_u64)
                .map(|exp| JwtClaims {
                    exp,
                    subject: value
                        .get("sub")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                })
        })
}

/// Accept the claims only if not yet expired (`exp >= now`).
fn check_expiry(claims: JwtClaims, now_unix_secs: u64) -> Result<JwtClaims, JwtError> {
    (claims.exp >= now_unix_secs)
        .then_some(claims)
        .ok_or(JwtError::Expired)
}

/// Constant-time byte-slice equality (equal length, no early-out on mismatch).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let length_equal = a.len() == b.len();
    let difference = a
        .iter()
        .zip(b.iter())
        .fold(0u8, |acc, (&x, &y)| acc | (x ^ y));
    length_equal & (difference == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A branchy, test-only base64url encoder used to mint tokens.
    fn b64(bytes: &[u8]) -> Vec<u8> {
        const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = Vec::new();
        let mut acc = 0u32;
        let mut bits = 0u32;
        for &byte in bytes {
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

    const SECRET: &[u8] = b"a-shared-256-bit-admission-secret";

    fn mint(header: &[u8], payload: &[u8], secret: &[u8]) -> Vec<u8> {
        let mut signing_input = b64(header);
        signing_input.push(b'.');
        signing_input.extend_from_slice(&b64(payload));
        let signature = hmac_sha256(secret, &signing_input);
        let mut token = signing_input;
        token.push(b'.');
        token.extend_from_slice(&b64(&signature));
        token
    }

    const HEADER: &[u8] = br#"{"alg":"HS256","typ":"JWT"}"#;

    #[test]
    fn accepts_a_valid_unexpired_token() {
        let token = mint(HEADER, br#"{"sub":"player-7","exp":2000}"#, SECRET);
        let claims = verify_jwt_hs256(&token, SECRET, 1000).unwrap();
        assert_eq!(claims.expiry_unix_secs(), 2000);
        assert_eq!(claims.subject(), Some("player-7"));
    }

    #[test]
    fn accepts_a_token_expiring_exactly_now_and_without_a_subject() {
        let token = mint(HEADER, br#"{"exp":1500}"#, SECRET);
        let claims = verify_jwt_hs256(&token, SECRET, 1500).unwrap();
        assert_eq!(claims.expiry_unix_secs(), 1500);
        assert_eq!(claims.subject(), None);
    }

    #[test]
    fn rejects_an_expired_token() {
        let token = mint(HEADER, br#"{"exp":1000}"#, SECRET);
        assert_eq!(verify_jwt_hs256(&token, SECRET, 1001), Err(JwtError::Expired));
    }

    #[test]
    fn rejects_a_wrong_secret() {
        // A well-formed token whose HMAC was computed under a different secret: the
        // signature segment decodes fine, but the recomputed MAC differs, so it is a
        // BadSignature (a value, never a panic).
        let token = mint(HEADER, br#"{"exp":2000}"#, SECRET);
        assert_eq!(
            verify_jwt_hs256(&token, b"the-wrong-secret", 1000),
            Err(JwtError::BadSignature)
        );
        // The same token under its real secret verifies — proving the rejection above
        // is the secret mismatch, not a malformed token.
        assert!(verify_jwt_hs256(&token, SECRET, 1000).is_ok());
    }

    #[test]
    fn rejects_wrong_segment_counts() {
        assert_eq!(verify_jwt_hs256(b"no-dots", SECRET, 0), Err(JwtError::Malformed));
        assert_eq!(verify_jwt_hs256(b"only.two", SECRET, 0), Err(JwtError::Malformed));
        assert_eq!(verify_jwt_hs256(b"a.b.c.d", SECRET, 0), Err(JwtError::Malformed));
    }

    #[test]
    fn rejects_a_non_base64url_signature() {
        let mut token = b64(HEADER);
        token.push(b'.');
        token.extend_from_slice(&b64(br#"{"exp":2000}"#));
        token.extend_from_slice(b".not+base64");
        assert_eq!(verify_jwt_hs256(&token, SECRET, 0), Err(JwtError::Malformed));
    }

    #[test]
    fn rejects_a_non_base64url_payload_even_when_signed() {
        // The payload segment is non-base64url; sign over the literal signing input
        // so the signature verifies and the payload-decode failure is what fires.
        let mut signing_input = b64(HEADER);
        signing_input.extend_from_slice(b".not+base64");
        let signature = hmac_sha256(SECRET, &signing_input);
        let mut token = signing_input;
        token.push(b'.');
        token.extend_from_slice(&b64(&signature));
        assert_eq!(verify_jwt_hs256(&token, SECRET, 0), Err(JwtError::Malformed));
    }

    #[test]
    fn rejects_non_json_and_exp_less_claims() {
        let not_json = mint(HEADER, b"not json at all", SECRET);
        assert_eq!(verify_jwt_hs256(&not_json, SECRET, 0), Err(JwtError::Malformed));
        let no_exp = mint(HEADER, br#"{"sub":"x"}"#, SECRET);
        assert_eq!(verify_jwt_hs256(&no_exp, SECRET, 0), Err(JwtError::Malformed));
    }

    #[test]
    fn error_and_claims_values_are_debuggable_and_comparable() {
        assert_eq!(JwtError::Expired, JwtError::Expired);
        assert_ne!(JwtError::Expired, JwtError::Malformed);
        assert!(format!("{:?}", JwtError::BadSignature).contains("BadSignature"));
        let claims = JwtClaims { exp: 1, subject: None };
        assert!(format!("{claims:?}").contains("JwtClaims"));
        assert_eq!(claims.clone(), claims);
    }
}
