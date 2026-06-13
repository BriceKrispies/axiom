//! A private signing key: the secret half of a keypair.

use core::fmt;

use ed25519_dalek::Signer;

use crate::signature::Signature;
use crate::verifying_key::VerifyingKey;

/// The private half of a keypair. It signs bytes and yields its public
/// [`VerifyingKey`]. Hold it secret: anyone with this key can sign as you.
///
/// Construct it from a 32-byte seed via [`Self::from_seed`]. OS entropy is *not*
/// gathered here — that keeps this layer deterministic and free of untestable
/// entropy-failure paths. An app generates a random seed at the edge (e.g. the
/// browser's Web Crypto) and passes it in.
#[derive(Clone)]
pub struct SigningKey(ed25519_dalek::SigningKey);

impl SigningKey {
    /// The seed length, in bytes.
    pub const SEED_LEN: usize = ed25519_dalek::SECRET_KEY_LENGTH;

    /// Build a signing key deterministically from a 32-byte seed. The same seed
    /// always yields the same key (and thus the same signatures).
    pub fn from_seed(seed: [u8; Self::SEED_LEN]) -> Self {
        SigningKey(ed25519_dalek::SigningKey::from_bytes(&seed))
    }

    /// This key's public verifying key, safe to publish.
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey::from_dalek(self.0.verifying_key())
    }

    /// Sign `message`, producing a detached [`Signature`] any holder of the
    /// matching [`VerifyingKey`] can check. Deterministic (RFC 8032).
    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature::from_dalek(self.0.sign(message))
    }
}

// A custom Debug that never prints the secret bytes.
impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKey").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_yields_same_key_and_signatures() {
        let a = SigningKey::from_seed([42u8; 32]);
        let b = SigningKey::from_seed([42u8; 32]);
        assert_eq!(a.verifying_key(), b.verifying_key());
        assert_eq!(a.sign(b"msg"), b.sign(b"msg"));
    }

    #[test]
    fn different_seeds_yield_different_keys() {
        let a = SigningKey::from_seed([1u8; 32]);
        let b = SigningKey::from_seed([2u8; 32]);
        assert_ne!(a.verifying_key(), b.verifying_key());
    }

    #[test]
    fn a_signature_verifies_under_its_own_key() {
        let key = SigningKey::from_seed([5u8; 32]);
        assert!(key.verifying_key().verify(b"abc", &key.sign(b"abc")));
    }

    #[test]
    fn debug_does_not_leak_the_secret() {
        // The Debug output is an opaque marker with no key material in it.
        let rendered = format!("{:?}", SigningKey::from_seed([0xAB; 32]));
        assert_eq!(rendered, "SigningKey { .. }");
    }

    #[test]
    fn cloned_key_signs_identically() {
        let key = SigningKey::from_seed([6u8; 32]);
        let twin = key.clone();
        assert_eq!(key.sign(b"x"), twin.sign(b"x"));
    }
}
