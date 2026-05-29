//! A validated fixed timestep magnitude, expressed in integer nanoseconds.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::result::KernelResult;

/// The duration of a single simulation step, in integer nanoseconds.
///
/// Integer nanoseconds (not floating point) keep stepping exactly reproducible.
/// A step must be strictly positive: a zero step would let a clock "advance"
/// without progressing, which is rejected at construction so every constructed
/// `FixedStep` is guaranteed valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FixedStep {
    nanos: u64,
}

impl FixedStep {
    /// Construct a fixed step of `nanos` nanoseconds.
    ///
    /// Returns [`KernelErrorCode::InvalidFixedStep`] if `nanos` is zero.
    pub const fn new(nanos: u64) -> KernelResult<Self> {
        if nanos == 0 {
            return Err(KernelError::new(
                KernelErrorScope::Time,
                KernelErrorCode::InvalidFixedStep,
                "fixed step must be greater than zero nanoseconds",
            ));
        }
        Ok(FixedStep { nanos })
    }

    /// The step duration in nanoseconds. Always greater than zero.
    pub const fn nanos(self) -> u64 {
        self.nanos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_step_is_accepted() {
        let step = FixedStep::new(16_666_667).unwrap();
        assert_eq!(step.nanos(), 16_666_667);
    }

    #[test]
    fn zero_step_is_rejected() {
        let err = FixedStep::new(0).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Time);
        assert_eq!(err.code(), KernelErrorCode::InvalidFixedStep);
    }

    #[test]
    fn equal_steps_compare_equal() {
        assert_eq!(FixedStep::new(1000).unwrap(), FixedStep::new(1000).unwrap());
    }
}
