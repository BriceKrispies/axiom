//! A stable identifier for one participant in a netcode session.

use axiom_kernel::{BinaryReader, BinaryWriter, HandleId, KernelResult};

/// A stable, ordered identifier for a peer (a player/connection) in a session.
///
/// A thin newtype over a kernel [`HandleId`] so peers key the deterministic
/// timeline and hash collections in a stable order and serialize on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(HandleId);

impl PeerId {
    /// Construct a peer id from its raw value.
    pub const fn from_raw(raw: u64) -> Self {
        PeerId(HandleId::from_raw(raw))
    }

    /// The raw value backing this peer id.
    pub const fn raw(self) -> u64 {
        self.0.raw()
    }

    /// Serialize as a little-endian `u64`.
    pub fn write_to(self, writer: &mut BinaryWriter) {
        self.0.write_to(writer);
    }

    /// Read a peer id previously written with [`Self::write_to`].
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Ok(PeerId(HandleId::read_from(reader)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_raw_round_trips() {
        assert_eq!(PeerId::from_raw(7).raw(), 7);
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(PeerId::from_raw(1) < PeerId::from_raw(2));
        assert_eq!(PeerId::from_raw(3), PeerId::from_raw(3));
    }

    #[test]
    fn serialization_round_trips() {
        let id = PeerId::from_raw(0x1122_3344_5566_7788);
        let mut w = BinaryWriter::new();
        id.write_to(&mut w);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(PeerId::read_from(&mut r).unwrap(), id);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn read_from_rejects_truncation() {
        let mut r = BinaryReader::new(&[0u8, 1u8]);
        assert!(PeerId::read_from(&mut r).is_err());
    }
}
