//! A validated power-of-two memory alignment.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::result::KernelResult;

/// A memory alignment, guaranteed at construction to be a power of two.
///
/// Validating the power-of-two invariant once, at construction, means every
/// `Alignment` value is usable for fast `offset % align == 0` checks without
/// re-validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Alignment(u64);

impl Alignment {
    /// Construct an alignment.
    ///
    /// Returns [`KernelErrorCode::InvalidAlignment`] unless `value` is a power
    /// of two (which also rejects zero).
    pub const fn new(value: u64) -> KernelResult<Self> {
        // `is_power_of_two` is true only for non-zero powers of two — exactly
        // the original `value != 0 && (value & (value - 1)) == 0` validity,
        // computed without a branch or a possible `0 - 1` underflow.
        let valid = value.is_power_of_two();
        [
            Err(KernelError::new(
                KernelErrorScope::Memory,
                KernelErrorCode::InvalidAlignment,
                "alignment must be a non-zero power of two",
            )),
            Ok(Alignment(value)),
        ][valid as usize]
    }

    /// The raw alignment value.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Whether `offset` lies on this alignment boundary.
    pub const fn is_aligned(self, offset: u64) -> bool {
        offset.is_multiple_of(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powers_of_two_are_accepted() {
        for p in [1u64, 2, 4, 8, 16, 4096] {
            assert_eq!(Alignment::new(p).unwrap().raw(), p);
        }
    }

    #[test]
    fn zero_is_rejected() {
        let err = Alignment::new(0).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Memory);
        assert_eq!(err.code(), KernelErrorCode::InvalidAlignment);
    }

    #[test]
    fn non_power_of_two_is_rejected() {
        assert_eq!(
            Alignment::new(3).unwrap_err().code(),
            KernelErrorCode::InvalidAlignment
        );
        assert_eq!(
            Alignment::new(6).unwrap_err().code(),
            KernelErrorCode::InvalidAlignment
        );
    }

    #[test]
    fn alignment_check_is_correct() {
        let align = Alignment::new(8).unwrap();
        assert!(align.is_aligned(0));
        assert!(align.is_aligned(16));
        assert!(!align.is_aligned(4));
    }
}

#[cfg(test)]
mod cov {
    use super::*;

    #[test]
    fn new_covers_both_sides_of_the_validity_check() {
        assert!(Alignment::new(0).is_err()); // value == 0 (left of ||)
        assert!(Alignment::new(3).is_err()); // non-zero, not power of two (right of ||)
        assert!(Alignment::new(8).is_ok()); // valid power of two (both false)
    }
}
