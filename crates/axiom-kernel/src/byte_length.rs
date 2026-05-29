//! A length, in bytes, of a span of memory.

/// A size, in bytes. A newtype over `u64` so lengths can't be confused with
/// offsets or arbitrary integers in kernel APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ByteLength(u64);

impl ByteLength {
    /// Construct a length from a raw byte count.
    pub const fn new(raw: u64) -> Self {
        ByteLength(raw)
    }

    /// The raw byte count.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Whether this length is zero.
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_raw_round_trip() {
        assert_eq!(ByteLength::new(128).raw(), 128);
    }

    #[test]
    fn zero_is_detected() {
        assert!(ByteLength::new(0).is_zero());
        assert!(!ByteLength::new(1).is_zero());
    }

    #[test]
    fn default_is_zero() {
        assert_eq!(ByteLength::default(), ByteLength::new(0));
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(ByteLength::new(1) < ByteLength::new(2));
    }
}
