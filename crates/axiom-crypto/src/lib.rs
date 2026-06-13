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

mod signature;
mod signing_key;
mod verifying_key;

pub use signature::Signature;
pub use signing_key::SigningKey;
pub use verifying_key::VerifyingKey;
