//! Machine-readable error scope: which kernel subsystem produced an error.

/// The kernel subsystem an error originated in.
///
/// Scopes are part of an error's *machine identity*: deterministic comparisons
/// rely on `(scope, code)`, never on human-readable text. Each scope has a
/// stable `#[repr(u16)]` discriminant so it can be serialized and compared
/// across builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum KernelErrorScope {
    Time = 1,
    Id = 2,
    Memory = 3,
    Message = 4,
    Binary = 5,
    Scalar = 7,
}

impl KernelErrorScope {
    /// The stable numeric discriminant of this scope.
    pub const fn raw(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable_and_distinct() {
        assert_eq!(KernelErrorScope::Time.raw(), 1);
        assert_eq!(KernelErrorScope::Id.raw(), 2);
        assert_eq!(KernelErrorScope::Memory.raw(), 3);
        assert_eq!(KernelErrorScope::Message.raw(), 4);
        assert_eq!(KernelErrorScope::Binary.raw(), 5);
        assert_eq!(KernelErrorScope::Scalar.raw(), 7);
    }

    #[test]
    fn equality_is_by_value() {
        assert_eq!(KernelErrorScope::Memory, KernelErrorScope::Memory);
        assert_ne!(KernelErrorScope::Memory, KernelErrorScope::Binary);
    }

    #[test]
    fn ordering_follows_discriminant() {
        assert!(KernelErrorScope::Time < KernelErrorScope::Scalar);
    }
}
