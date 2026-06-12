//! Machine-readable error code: the precise reason an operation failed.

/// A precise, stable reason an operation failed.
///
/// Codes are the primary machine identity of a [`crate::error::KernelError`].
/// Two errors are equal when their scope and code match; the human message is
/// metadata only. Each code has a fixed `#[repr(u16)]` discriminant so it is
/// safe to serialize and compare deterministically across builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum KernelErrorCode {
    /// A fixed-step magnitude was zero or otherwise invalid.
    InvalidFixedStep = 1,
    /// Integer arithmetic on ticks, offsets or lengths overflowed.
    RangeOverflow = 2,
    /// An identifier was the null/invalid value where a valid one was required.
    InvalidId = 3,
    /// An alignment was zero or not a power of two.
    InvalidAlignment = 4,
    /// A read or index fell outside the bounds of the available data.
    OutOfBounds = 5,
    /// Serialized data ended before the requested value could be read.
    TruncatedData = 6,
    /// A schema version was incompatible with the reader.
    SchemaVersionMismatch = 7,
    /// A layer declared the same dependency more than once.
    DuplicateDependency = 8,
    /// A layer declared the same capability more than once.
    DuplicateCapability = 9,
    /// A layer attempted to import itself.
    SelfImport = 10,
    /// A layer attempted to import a higher (future) layer.
    ForwardImport = 11,
    /// The kernel layer (index 0) declared a dependency; it must import nothing.
    KernelMustNotImport = 12,
    /// A dimensioned scalar quantity was built from a non-finite value (NaN or
    /// infinity).
    NonFiniteScalar = 13,
    /// A serialized tagged value carried a discriminant the reader does not
    /// recognize (e.g. an unknown enum tag byte in an otherwise well-formed
    /// buffer).
    InvalidDiscriminant = 14,
}

impl KernelErrorCode {
    /// The stable numeric discriminant of this code.
    pub const fn raw(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(KernelErrorCode::InvalidFixedStep.raw(), 1);
        assert_eq!(KernelErrorCode::OutOfBounds.raw(), 5);
        assert_eq!(KernelErrorCode::KernelMustNotImport.raw(), 12);
        assert_eq!(KernelErrorCode::NonFiniteScalar.raw(), 13);
        assert_eq!(KernelErrorCode::InvalidDiscriminant.raw(), 14);
    }

    #[test]
    fn codes_are_distinct() {
        assert_ne!(KernelErrorCode::SelfImport, KernelErrorCode::ForwardImport);
    }

    #[test]
    fn ordering_follows_discriminant() {
        assert!(KernelErrorCode::InvalidFixedStep < KernelErrorCode::RangeOverflow);
    }
}
