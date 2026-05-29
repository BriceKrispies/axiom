//! A byte offset into a linear address space.

/// A position, in bytes, within a linear address space.
///
/// A newtype over `u64` so offsets can't be silently confused with lengths or
/// arbitrary integers in kernel APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ByteOffset(u64);

impl ByteOffset {
    /// Construct an offset from a raw byte position.
    pub const fn new(raw: u64) -> Self {
        ByteOffset(raw)
    }

    /// The raw byte position.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_raw_round_trip() {
        assert_eq!(ByteOffset::new(64).raw(), 64);
    }

    #[test]
    fn default_is_zero() {
        assert_eq!(ByteOffset::default(), ByteOffset::new(0));
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(ByteOffset::new(8) < ByteOffset::new(16));
    }
}
