//! Host admission policy (SPEC-13 §16.4) — the **tool-tier** half of JWT auth.
//!
//! The cryptographic verification (HS256 signature + expiry, every malformed arm)
//! lives in the `axiom-crypto` layer ([`axiom_crypto::verify_jwt_hs256`]), pure and
//! 100% covered. This module owns the *policy*: where the secret comes from
//! (`AXIOM_JWT_SECRET`), what "the current time" is (the wall clock, the one bit of
//! nondeterminism, isolated to the admission edge), and what an invalid token means
//! (no seat, no `Welcome`). It is the seam the contract calls out: the *seam* is the
//! engine's, the *policy* is the app/tool's.

use std::time::{SystemTime, UNIX_EPOCH};

use axiom_crypto::verify_jwt_hs256;

/// A room's admission policy: open (no token required) or HS256-secret-gated.
pub struct JwtPolicy {
    secret: Option<Vec<u8>>,
}

impl std::fmt::Debug for JwtPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret; only whether one is configured.
        f.debug_struct("JwtPolicy")
            .field("secret_configured", &self.secret.is_some())
            .finish()
    }
}

impl JwtPolicy {
    /// Open admission — every well-formed `JoinRoom` is admitted (the dev default).
    pub fn open() -> Self {
        JwtPolicy { secret: None }
    }

    /// Require a valid HS256 JWT signed with `secret`.
    pub fn with_secret(secret: Vec<u8>) -> Self {
        JwtPolicy {
            secret: Some(secret),
        }
    }

    /// Read the policy from `AXIOM_JWT_SECRET`: secret-gated when set, open otherwise.
    pub fn from_env() -> Self {
        std::env::var("AXIOM_JWT_SECRET")
            .ok()
            .map(|secret| Self::with_secret(secret.into_bytes()))
            .unwrap_or_else(Self::open)
    }

    /// Whether `token` is admitted at `now_unix_secs`. An open policy admits any
    /// token; a secret-gated policy admits only a token whose signature and expiry
    /// verify. Pure in its arguments (time injected) so it is directly testable.
    pub fn admits_at(&self, token: &[u8], now_unix_secs: u64) -> bool {
        self.secret
            .as_deref()
            .map(|secret| verify_jwt_hs256(token, secret, now_unix_secs).is_ok())
            .unwrap_or(true)
    }

    /// Whether `token` is admitted right now (wall clock). The single bit of
    /// nondeterminism, deliberately isolated here at the admission edge.
    pub fn admits(&self, token: &[u8]) -> bool {
        self.admits_at(token, unix_now_secs())
    }
}

/// The current Unix time in seconds (saturating; a pre-epoch clock reads 0).
fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A valid HS256 token minted for secret "test-admission-secret",
    // claims {"sub":"player-1","exp":2000000000} (~year 2033).
    const VALID_TOKEN: &[u8] = b"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJwbGF5ZXItMSIsImV4cCI6MjAwMDAwMDAwMH0.dVGsRVl8Ddx2bykSrwiJyHO678JVJLfbdmHkpi16XLg";
    const SECRET: &[u8] = b"test-admission-secret";

    #[test]
    fn an_open_policy_admits_any_token() {
        let policy = JwtPolicy::open();
        assert!(policy.admits_at(b"", 0));
        assert!(policy.admits_at(b"garbage", 0));
        assert!(policy.admits(VALID_TOKEN));
    }

    #[test]
    fn a_secret_policy_admits_a_valid_unexpired_token() {
        let policy = JwtPolicy::with_secret(SECRET.to_vec());
        assert!(policy.admits_at(VALID_TOKEN, 1_000));
        // And via the wall-clock path (the token expires in 2033).
        assert!(policy.admits(VALID_TOKEN));
    }

    #[test]
    fn a_secret_policy_rejects_invalid_expired_and_garbage_tokens() {
        let policy = JwtPolicy::with_secret(SECRET.to_vec());
        // Expired: now is past the token's exp.
        assert!(!policy.admits_at(VALID_TOKEN, 2_000_000_001));
        // Garbage / wrong-secret tokens are refused.
        assert!(!policy.admits_at(b"not-a-token", 1_000));
        assert!(!policy.admits_at(b"", 1_000));
        // A token signed with a different secret is refused by this policy.
        let other = JwtPolicy::with_secret(b"a-different-secret".to_vec());
        assert!(!other.admits_at(VALID_TOKEN, 1_000));
    }

    #[test]
    fn debug_does_not_leak_the_secret() {
        let shown = format!("{:?}", JwtPolicy::with_secret(b"super-secret".to_vec()));
        assert!(shown.contains("secret_configured: true"));
        assert!(!shown.contains("super-secret"));
        assert!(format!("{:?}", JwtPolicy::open()).contains("secret_configured: false"));
    }
}
