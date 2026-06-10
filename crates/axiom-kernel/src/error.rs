//! The kernel error value: a `(scope, code)` machine identity plus optional text.

use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;

/// A deterministic kernel error.
///
/// The *identity* of an error is the pair `(scope, code)`. Equality is defined
/// purely on that pair — the `&'static str` message exists for humans and never
/// participates in comparison. This guarantees two errors built from the same
/// scope and code compare equal regardless of message, which keeps error
/// handling deterministic and replayable.
#[derive(Debug, Clone, Copy)]
pub struct KernelError {
    scope: KernelErrorScope,
    code: KernelErrorCode,
    message: &'static str,
}

impl KernelError {
    /// Construct an error from its machine identity and a static human message.
    pub const fn new(
        scope: KernelErrorScope,
        code: KernelErrorCode,
        message: &'static str,
    ) -> Self {
        Self {
            scope,
            code,
            message,
        }
    }

    /// The subsystem this error came from.
    pub const fn scope(&self) -> KernelErrorScope {
        self.scope
    }

    /// The precise machine-readable reason.
    pub const fn code(&self) -> KernelErrorCode {
        self.code
    }

    /// The human-readable message. Never used for comparison.
    pub const fn message(&self) -> &'static str {
        self.message
    }
}

/// Equality is defined on machine identity only, deliberately ignoring the
/// human message so error comparisons stay deterministic.
impl PartialEq for KernelError {
    fn eq(&self, other: &Self) -> bool {
        self.scope == other.scope && self.code == other.code
    }
}

impl Eq for KernelError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ignores_message() {
        let a = KernelError::new(
            KernelErrorScope::Binary,
            KernelErrorCode::OutOfBounds,
            "read past end of buffer",
        );
        let b = KernelError::new(
            KernelErrorScope::Binary,
            KernelErrorCode::OutOfBounds,
            "totally different wording",
        );
        assert_eq!(a, b, "same (scope, code) must compare equal");
    }

    #[test]
    fn different_code_is_not_equal() {
        let a = KernelError::new(KernelErrorScope::Binary, KernelErrorCode::OutOfBounds, "x");
        let b = KernelError::new(
            KernelErrorScope::Binary,
            KernelErrorCode::TruncatedData,
            "x",
        );
        assert_ne!(a, b);
    }

    #[test]
    fn different_scope_is_not_equal() {
        let a = KernelError::new(
            KernelErrorScope::Memory,
            KernelErrorCode::RangeOverflow,
            "x",
        );
        let b = KernelError::new(KernelErrorScope::Time, KernelErrorCode::RangeOverflow, "x");
        assert_ne!(a, b);
    }

    #[test]
    fn accessors_return_constructed_parts() {
        let e = KernelError::new(
            KernelErrorScope::Layer,
            KernelErrorCode::SelfImport,
            "layer imported itself",
        );
        assert_eq!(e.scope(), KernelErrorScope::Layer);
        assert_eq!(e.code(), KernelErrorCode::SelfImport);
        assert_eq!(e.message(), "layer imported itself");
    }
}

#[cfg(test)]
mod cov {
    use super::*;

    #[test]
    fn eq_covers_both_short_circuit_sides() {
        let base = KernelError::new(
            KernelErrorScope::Memory,
            KernelErrorCode::RangeOverflow,
            "x",
        );
        let same = KernelError::new(
            KernelErrorScope::Memory,
            KernelErrorCode::RangeOverflow,
            "y",
        );
        let diff_scope =
            KernelError::new(KernelErrorScope::Time, KernelErrorCode::RangeOverflow, "z");
        let diff_code = KernelError::new(KernelErrorScope::Memory, KernelErrorCode::InvalidId, "w");
        assert!(base == same); // scope eq true, code eq true
        assert!(base != diff_scope); // scope eq false (short-circuits)
        assert!(base != diff_code); // scope eq true, code eq false
    }
}
