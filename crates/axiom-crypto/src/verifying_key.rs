//! A public verifying key: checks signatures and identifies a signer on the wire.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
};

use crate::signature::Signature;

/// The public half of a keypair. It verifies signatures produced by the matching
/// [`crate::SigningKey`] and serializes onto the wire so peers can publish and
/// pin each other's identities (a roster of `VerifyingKey`s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyingKey(ed25519_dalek::VerifyingKey);

impl VerifyingKey {
    /// The fixed on-wire length of a verifying key, in bytes.
    pub const LEN: usize = ed25519_dalek::PUBLIC_KEY_LENGTH;

    /// Wrap a raw dalek key (crate-internal — derived from a signing key).
    pub(crate) fn from_dalek(inner: ed25519_dalek::VerifyingKey) -> Self {
        VerifyingKey(inner)
    }

    /// Whether `signature` is a valid signature of `message` under this key.
    ///
    /// Uses strict verification (rejecting non-canonical / malleable encodings),
    /// so a given `(key, message)` accepts exactly one signature.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        self.0.verify_strict(message, signature.as_dalek()).is_ok()
    }

    /// The raw [`Self::LEN`]-byte encoding — convenient for an app that publishes
    /// this key over a wire it controls (e.g. the browser pubkey handshake).
    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        self.0.to_bytes()
    }

    /// Reconstruct a key from its raw bytes, or `None` if they are not a valid
    /// curve point.
    pub fn try_from_bytes(bytes: &[u8; Self::LEN]) -> Option<Self> {
        ed25519_dalek::VerifyingKey::from_bytes(bytes)
            .ok()
            .map(VerifyingKey)
    }

    /// Serialize as exactly [`Self::LEN`] raw bytes (no length prefix).
    pub fn write_to(&self, writer: &mut BinaryWriter) {
        for byte in self.to_bytes() {
            writer.write_u8(byte);
        }
    }

    /// Read a verifying key previously written with [`Self::write_to`].
    ///
    /// Fails the reader's `TruncatedData`/`OutOfBounds` if fewer than
    /// [`Self::LEN`] bytes remain, or `Binary`/`InvalidDiscriminant` if the bytes
    /// are not a valid curve point.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        let mut bytes = [0u8; Self::LEN];
        for slot in bytes.iter_mut() {
            *slot = reader.read_u8()?;
        }
        Self::try_from_bytes(&bytes).ok_or_else(|| {
            KernelError::new(
                KernelErrorScope::Binary,
                KernelErrorCode::InvalidDiscriminant,
                "verifying key bytes are not a valid curve point",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SigningKey;

    #[test]
    fn verifies_a_genuine_signature_and_rejects_others() {
        let key = SigningKey::from_seed([3u8; 32]);
        let vk = key.verifying_key();
        let sig = key.sign(b"hello");
        assert!(vk.verify(b"hello", &sig));
        // Wrong message under the right key, and the right message under a
        // different key, both fail.
        assert!(!vk.verify(b"hella", &sig));
        let other = SigningKey::from_seed([4u8; 32]).verifying_key();
        assert!(!other.verify(b"hello", &sig));
    }

    #[test]
    fn serialization_round_trips() {
        let vk = SigningKey::from_seed([9u8; 32]).verifying_key();
        let mut w = BinaryWriter::new();
        vk.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), VerifyingKey::LEN);
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(VerifyingKey::read_from(&mut r).unwrap(), vk);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let vk = SigningKey::from_seed([9u8; 32]).verifying_key();
        let mut w = BinaryWriter::new();
        vk.write_to(&mut w);
        let bytes = w.into_bytes();
        for k in 0..bytes.len() {
            let mut r = BinaryReader::new(&bytes[..k]);
            assert!(VerifyingKey::read_from(&mut r).is_err(), "prefix {k} fails");
        }
    }

    #[test]
    fn raw_bytes_round_trip() {
        let vk = SigningKey::from_seed([12u8; 32]).verifying_key();
        let bytes = vk.to_bytes();
        assert_eq!(bytes.len(), VerifyingKey::LEN);
        assert_eq!(VerifyingKey::try_from_bytes(&bytes), Some(vk));
        // Non-point bytes yield None.
        assert_eq!(
            VerifyingKey::try_from_bytes(&[2u8; VerifyingKey::LEN]),
            None
        );
    }

    #[test]
    fn non_point_bytes_fail_to_decode() {
        // `[2; 32]` is a y-coordinate with no corresponding x on the curve, so it
        // does not decompress to a valid point and must be rejected as malformed.
        let bytes = [2u8; VerifyingKey::LEN];
        let mut r = BinaryReader::new(&bytes);
        let err = VerifyingKey::read_from(&mut r).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Binary);
        assert_eq!(err.code(), KernelErrorCode::InvalidDiscriminant);
    }
}
