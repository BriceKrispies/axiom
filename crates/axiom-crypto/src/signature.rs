//! A detached ed25519 signature, wire-serializable.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

/// A 64-byte ed25519 signature over some bytes, produced by [`crate::SigningKey`]
/// and checked by [`crate::VerifyingKey`].
///
/// It is opaque: construct it only by signing or by decoding a previously
/// encoded signature. A structurally-malformed (wrong-length) buffer fails to
/// decode; a well-formed-but-wrong signature is not detected here — it simply
/// fails [`crate::VerifyingKey::verify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature(ed25519_dalek::Signature);

impl Signature {
    /// The fixed on-wire length of a signature, in bytes.
    pub const LEN: usize = ed25519_dalek::SIGNATURE_LENGTH;

    /// Wrap a raw dalek signature (crate-internal — produced by signing).
    pub(crate) fn from_dalek(inner: ed25519_dalek::Signature) -> Self {
        Signature(inner)
    }

    /// The underlying dalek signature (crate-internal — consumed by verifying).
    pub(crate) fn as_dalek(&self) -> &ed25519_dalek::Signature {
        &self.0
    }

    /// Serialize as exactly [`Self::LEN`] raw bytes (no length prefix — the
    /// length is fixed and known).
    pub fn write_to(&self, writer: &mut BinaryWriter) {
        for &byte in self.0.to_bytes().iter() {
            writer.write_u8(byte);
        }
    }

    /// Read a signature previously written with [`Self::write_to`]. Fails (the
    /// reader's `TruncatedData`/`OutOfBounds`) if fewer than [`Self::LEN`] bytes
    /// remain.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        let mut bytes = [0u8; Self::LEN];
        for slot in bytes.iter_mut() {
            *slot = reader.read_u8()?;
        }
        Ok(Signature(ed25519_dalek::Signature::from_bytes(&bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SigningKey;

    fn sample() -> Signature {
        SigningKey::from_seed([7u8; 32]).sign(b"payload")
    }

    #[test]
    fn serialization_round_trips() {
        let sig = sample();
        let mut w = BinaryWriter::new();
        sig.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), Signature::LEN);
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(Signature::read_from(&mut r).unwrap(), sig);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let mut w = BinaryWriter::new();
        sample().write_to(&mut w);
        let bytes = w.into_bytes();
        for k in 0..bytes.len() {
            let mut r = BinaryReader::new(&bytes[..k]);
            assert!(
                Signature::read_from(&mut r).is_err(),
                "prefix {k} must fail"
            );
        }
        assert!(Signature::read_from(&mut BinaryReader::new(&bytes)).is_ok());
    }

    #[test]
    fn distinct_signatures_are_unequal() {
        let key = SigningKey::from_seed([1u8; 32]);
        assert_ne!(key.sign(b"a"), key.sign(b"b"));
    }
}
