//! # Axiom Crypto — authenticated identity
//!
//! A small, deterministic signing primitive built on a vetted ed25519
//! implementation. It answers exactly one question: **did the holder of this
//! private key author these bytes?** Higher layers and modules use it to bind a
//! message to its author so the author cannot be forged — for example,
//! `axiom-netcode` signs every input frame, so a compromised peer (or relay)
//! cannot inject inputs under another peer's identity.
//!
//! The surface is intentionally tiny and fully deterministic:
//!
//! * [`SigningKey`] — a private key, constructed from a 32-byte seed
//!   ([`SigningKey::from_seed`]). It [`sign`](SigningKey::sign)s bytes and yields
//!   its [`VerifyingKey`](SigningKey::verifying_key). OS entropy is *not* taken
//!   here — an app generates a random seed at the edge and passes it in, keeping
//!   this layer free of nondeterminism and untestable entropy-failure arms.
//! * [`VerifyingKey`] — the public key. It [`verify`](VerifyingKey::verify)s a
//!   signature against bytes and serializes onto the wire / into a roster.
//! * [`Signature`] — a 64-byte detached signature, wire-serializable.
//!
//! ed25519 signing is deterministic (RFC 8032): the same key over the same bytes
//! always yields the same signature, so signed traffic stays replayable.
//!
//! ## Shared-secret authentication (HMAC / JWT)
//!
//! Alongside public-key authorship, the layer offers symmetric, shared-secret
//! authentication for **host admission** (SPEC-13 §16.4): a join token whose holder
//! proves knowledge of a server's secret.
//!
//! * [`hmac_sha256`] — HMAC-SHA256 (RFC 2104) over the vetted `sha2` hash.
//! * [`verify_jwt_hs256`] — verify a compact-JWS / JWT (HS256 only) signature and
//!   expiry against a secret, returning [`JwtClaims`] or a typed [`JwtError`]. The
//!   current time is injected, so verification stays pure and deterministic; auth
//!   *policy* (which secret, what to do on reject) lives at the app/tool edge.

mod base64url;
mod hmac_sha256;
mod jwt;
mod signature;
mod signing_key;
mod verifying_key;

pub use hmac_sha256::{hmac_sha256, HMAC_SHA256_LEN};
pub use jwt::{verify_jwt_hs256, JwtClaims, JwtError};
pub use signature::Signature;
pub use signing_key::SigningKey;
pub use verifying_key::VerifyingKey;
