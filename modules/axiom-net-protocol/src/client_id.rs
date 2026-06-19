//! The server-assigned identity of a connected client.

use axiom_kernel::{
    BinaryReader, BinaryWriter, HandleId, KernelError, KernelErrorCode, KernelErrorScope,
    KernelResult,
};

/// A client's identity, assigned by the server and carried in `Welcome`.
///
/// Backed by a kernel [`HandleId`] so it is stable, ordered, and serializes as a
/// little-endian `u64`. A client id must be **nonzero**: `0` is the kernel's
/// reserved null handle and is rejected at construction, satisfying "client id
/// must be nonzero if represented numerically".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClientId(HandleId);

impl ClientId {
    /// Construct a client id from its raw value, rejecting the null id (`0`).
    pub(crate) fn new(value: u64) -> KernelResult<Self> {
        let id = HandleId::from_raw(value);
        id.is_valid().then_some(ClientId(id)).ok_or_else(|| {
            KernelError::new(
                KernelErrorScope::Message,
                KernelErrorCode::InvalidId,
                "client id must be nonzero",
            )
        })
    }

    /// The raw `u64` value.
    pub(crate) fn raw(self) -> u64 {
        self.0.raw()
    }

    /// Serialize as a little-endian `u64`.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        self.0.write_to(writer);
    }

    /// Read a client id, re-validating it is nonzero.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        reader.read_u64().and_then(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonzero_is_accepted() {
        assert_eq!(ClientId::new(42).unwrap().raw(), 42);
    }

    #[test]
    fn zero_is_rejected() {
        let err = ClientId::new(0).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Message);
        assert_eq!(err.code(), KernelErrorCode::InvalidId);
    }

    #[test]
    fn serialization_round_trips() {
        let id = ClientId::new(0x1122_3344).unwrap();
        let mut w = BinaryWriter::new();
        id.write_to(&mut w);
        let mut r = BinaryReader::new(w.as_bytes());
        assert_eq!(ClientId::read_from(&mut r).unwrap(), id);
    }

    #[test]
    fn decode_rejects_a_zero_id() {
        let mut w = BinaryWriter::new();
        w.write_u64(0);
        let mut r = BinaryReader::new(w.as_bytes());
        assert_eq!(
            ClientId::read_from(&mut r).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
    }

    #[test]
    fn decode_rejects_truncation() {
        let mut r = BinaryReader::new(&[0u8, 1u8, 2u8]);
        assert!(ClientId::read_from(&mut r).is_err());
    }
}
