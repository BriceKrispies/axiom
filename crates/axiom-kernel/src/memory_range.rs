//! A half-open `[offset, offset + length)` range with checked math.

use crate::alignment::Alignment;
use crate::byte_length::ByteLength;
use crate::byte_offset::ByteOffset;
use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::result::KernelResult;

/// A half-open byte range `[offset, offset + length)`.
///
/// The invariant `offset + length <= u64::MAX` is checked at construction, so
/// [`Self::end`] can never overflow and every containment / overlap query is
/// total. Containment and overlap use half-open interval semantics, so a
/// zero-length range contains nothing and overlaps nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryRange {
    offset: ByteOffset,
    length: ByteLength,
}

impl MemoryRange {
    /// Construct a range from an offset and length.
    ///
    /// Returns [`KernelErrorCode::RangeOverflow`] if `offset + length` would
    /// exceed `u64::MAX`.
    pub const fn new(offset: ByteOffset, length: ByteLength) -> KernelResult<Self> {
        let fits = offset.raw().checked_add(length.raw()).is_some();
        [
            Err(KernelError::new(
                KernelErrorScope::Memory,
                KernelErrorCode::RangeOverflow,
                "memory range end offset overflows u64",
            )),
            Ok(MemoryRange { offset, length }),
        ][fits as usize]
    }

    /// The starting offset.
    pub const fn offset(self) -> ByteOffset {
        self.offset
    }

    /// The length in bytes.
    pub const fn length(self) -> ByteLength {
        self.length
    }

    /// The exclusive end offset (`offset + length`). Never overflows.
    pub const fn end(self) -> u64 {
        self.offset.raw() + self.length.raw()
    }

    /// Whether `offset` falls within `[start, end)`.
    pub const fn contains_offset(self, offset: u64) -> bool {
        (offset >= self.offset.raw()) & (offset < self.end())
    }

    /// Whether `other` is fully contained within `self`.
    ///
    /// A zero-length `other` is never contained (it occupies no byte position).
    pub const fn contains_range(self, other: MemoryRange) -> bool {
        (other.length.raw() != 0)
            & (other.offset.raw() >= self.offset.raw())
            & (other.end() <= self.end())
    }

    /// Whether `self` and `other` share at least one byte position.
    pub const fn overlaps(self, other: MemoryRange) -> bool {
        (self.offset.raw() < other.end()) & (other.offset.raw() < self.end())
    }

    /// Whether this range's start offset lies on the given alignment boundary.
    pub const fn is_aligned(self, alignment: Alignment) -> bool {
        alignment.is_aligned(self.offset.raw())
    }

    /// Return this range shifted forward by `delta` bytes.
    ///
    /// Returns [`KernelErrorCode::RangeOverflow`] if the shifted end would
    /// exceed `u64::MAX`.
    pub const fn checked_shift(self, delta: u64) -> KernelResult<MemoryRange> {
        let base = self.offset.raw();
        let shifted = base.wrapping_add(delta);
        // Unsigned addition wraps below `base` exactly when it overflowed.
        let overflowed = shifted < base;
        [
            MemoryRange::new(ByteOffset::new(shifted), self.length),
            Err(KernelError::new(
                KernelErrorScope::Memory,
                KernelErrorCode::RangeOverflow,
                "shifted memory range offset overflows u64",
            )),
        ][overflowed as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(offset: u64, length: u64) -> MemoryRange {
        MemoryRange::new(ByteOffset::new(offset), ByteLength::new(length)).unwrap()
    }

    #[test]
    fn accessors_return_constructed_parts() {
        let r = range(10, 5);
        assert_eq!(r.offset(), ByteOffset::new(10));
        assert_eq!(r.length(), ByteLength::new(5));
    }

    #[test]
    fn end_is_offset_plus_length() {
        assert_eq!(range(10, 5).end(), 15);
    }

    #[test]
    fn construction_overflow_is_rejected() {
        let err = MemoryRange::new(ByteOffset::new(u64::MAX), ByteLength::new(1)).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::RangeOverflow);
    }

    #[test]
    fn contains_offset_is_half_open() {
        let r = range(10, 5);
        assert!(!r.contains_offset(9));
        assert!(r.contains_offset(10));
        assert!(r.contains_offset(14));
        assert!(!r.contains_offset(15));
    }

    #[test]
    fn contains_range_requires_full_containment() {
        let outer = range(0, 100);
        assert!(outer.contains_range(range(10, 20)));
        assert!(outer.contains_range(range(0, 100)));
        assert!(!outer.contains_range(range(90, 20)));
        assert!(!outer.contains_range(range(50, 0)));
    }

    #[test]
    fn overlap_detection() {
        assert!(range(0, 10).overlaps(range(5, 10)));
        assert!(range(5, 10).overlaps(range(0, 10)));
        assert!(!range(0, 10).overlaps(range(10, 10)));
        assert!(!range(0, 10).overlaps(range(100, 10)));
    }

    #[test]
    fn alignment_validation() {
        let align = Alignment::new(16).unwrap();
        assert!(range(32, 4).is_aligned(align));
        assert!(!range(33, 4).is_aligned(align));
    }

    #[test]
    fn checked_shift_moves_and_detects_overflow() {
        assert_eq!(range(10, 5).checked_shift(90).unwrap(), range(100, 5));
        let err = range(10, 5).checked_shift(u64::MAX).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::RangeOverflow);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use crate::alignment::Alignment;
    use crate::byte_length::ByteLength;
    use crate::byte_offset::ByteOffset;

    #[test]
    fn covers_overflow_and_predicate_branches() {
        assert!(MemoryRange::new(ByteOffset::new(u64::MAX), ByteLength::new(1)).is_err());
        let r = MemoryRange::new(ByteOffset::new(10), ByteLength::new(10)).unwrap();
        assert!(!r.contains_offset(5));
        assert!(r.contains_offset(10));
        assert!(!r.contains_offset(20));
        let zero = MemoryRange::new(ByteOffset::new(12), ByteLength::new(0)).unwrap();
        assert!(!r.contains_range(zero));
        let inside = MemoryRange::new(ByteOffset::new(12), ByteLength::new(4)).unwrap();
        assert!(r.contains_range(inside));
        let beyond = MemoryRange::new(ByteOffset::new(18), ByteLength::new(10)).unwrap();
        assert!(!r.contains_range(beyond));
        let below = MemoryRange::new(ByteOffset::new(0), ByteLength::new(5)).unwrap();
        assert!(!r.contains_range(below));
        let over = MemoryRange::new(ByteOffset::new(15), ByteLength::new(10)).unwrap();
        assert!(r.overlaps(over));
        let disjoint = MemoryRange::new(ByteOffset::new(100), ByteLength::new(5)).unwrap();
        assert!(!r.overlaps(disjoint));
        assert!(r.checked_shift(u64::MAX).is_err());
        assert!(r.checked_shift(5).is_ok());
        let _ = r.is_aligned(Alignment::new(2).unwrap());
    }
}

#[cfg(test)]
mod cov2 {
    use super::*;
    use crate::byte_length::ByteLength;
    use crate::byte_offset::ByteOffset;

    #[test]
    fn overlaps_is_false_when_other_is_entirely_before_self() {
        let r = MemoryRange::new(ByteOffset::new(10), ByteLength::new(10)).unwrap();
        let before = MemoryRange::new(ByteOffset::new(0), ByteLength::new(5)).unwrap();
        assert!(!r.overlaps(before));
    }

    #[test]
    fn overlaps_is_false_when_self_start_touches_other_end() {
        // self.offset (10) == other.end (10): touching, not overlapping (half-open).
        let r = MemoryRange::new(ByteOffset::new(10), ByteLength::new(10)).unwrap();
        let touching = MemoryRange::new(ByteOffset::new(5), ByteLength::new(5)).unwrap();
        assert_eq!(touching.end(), 10);
        assert!(!r.overlaps(touching));
    }
}
