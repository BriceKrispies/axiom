//! Byte order, and the byte order the kernel serializes in.

/// A byte order.
///
/// The kernel serializes **exclusively** in little-endian so that bytes written
/// on any host reproduce identically everywhere (including `wasm32`). This enum
/// names that choice explicitly — [`Endian::KERNEL`] is the canonical kernel
/// order — and exists so higher layers can reason about byte order without
/// re-deriving the convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Endian {
    Little,
    Big,
}

impl Endian {
    /// The byte order the kernel serializes in. Always little-endian.
    pub const KERNEL: Endian = Endian::Little;

    /// Whether this is little-endian order.
    pub const fn is_little(self) -> bool {
        (self as u8) == (Endian::Little as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_order_is_little_endian() {
        assert_eq!(Endian::KERNEL, Endian::Little);
        assert!(Endian::KERNEL.is_little());
    }

    #[test]
    fn is_little_distinguishes_variants() {
        assert!(Endian::Little.is_little());
        assert!(!Endian::Big.is_little());
    }
}
