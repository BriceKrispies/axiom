//! The application protocol version negotiated in `JoinRoom` / `Welcome`.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
};

/// The application-level protocol version a client announces and a server
/// confirms. It is distinct from the wire-codec [`crate::frame::WIRE_VERSION`]:
/// this number lets the *application* protocol evolve independently of the byte
/// framing.
///
/// A protocol version must be **nonzero** — `0` is reserved as "unset" and is
/// rejected at construction, so every constructed value (and every decoded one)
/// is valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProtocolVersion(u32);

impl ProtocolVersion {
    /// Construct a protocol version, rejecting zero.
    pub(crate) fn new(value: u32) -> KernelResult<Self> {
        (value != 0)
            .then_some(ProtocolVersion(value))
            .ok_or_else(|| {
                KernelError::new(
                    KernelErrorScope::Message,
                    KernelErrorCode::InvalidId,
                    "protocol version must be nonzero",
                )
            })
    }

    pub(crate) fn raw(self) -> u32 {
        self.0
    }

    /// Serialize as a little-endian `u32`.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_u32(self.0);
    }

    /// Read a protocol version, re-validating it is nonzero.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        reader.read_u32().and_then(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonzero_is_accepted() {
        assert_eq!(ProtocolVersion::new(1).unwrap().raw(), 1);
    }

    #[test]
    fn zero_is_rejected() {
        let err = ProtocolVersion::new(0).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Message);
        assert_eq!(err.code(), KernelErrorCode::InvalidId);
    }

    #[test]
    fn serialization_round_trips() {
        let v = ProtocolVersion::new(7).unwrap();
        let mut w = BinaryWriter::new();
        v.write_to(&mut w);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(ProtocolVersion::read_from(&mut r).unwrap(), v);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn decode_rejects_a_zero_version() {
        let mut w = BinaryWriter::new();
        w.write_u32(0);
        let mut r = BinaryReader::new(w.as_bytes());
        assert_eq!(
            ProtocolVersion::read_from(&mut r).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
    }

    #[test]
    fn decode_rejects_truncation() {
        let mut r = BinaryReader::new(&[1u8, 2u8]);
        assert!(ProtocolVersion::read_from(&mut r).is_err());
    }
}
