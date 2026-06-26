//! Deterministic [`KernelError`] values for every recording failure path.
//!
//! All recording failures are memory/bounds problems, so they share the
//! `(Memory, OutOfBounds)` machine identity (`KernelError` equality is on
//! `(scope, code)` only — the human message is a non-comparable diagnostic).
//! Every fallible operation returns one of these rather than panicking.
//!
//! These are runtime constructors (not `const` items): each is built at the
//! moment a failure path is taken, so the construction is ordinary executed code
//! exercised by the tests that provoke each failure.

use axiom_kernel::{KernelError, KernelErrorCode, KernelErrorScope};

/// Build a recording error with a static diagnostic message.
fn mem_error(message: &'static str) -> KernelError {
    KernelError::new(
        KernelErrorScope::Memory,
        KernelErrorCode::OutOfBounds,
        message,
    )
}

/// `max_frames` was zero (a timeline must retain at least one frame).
pub(crate) fn zero_max_frames() -> KernelError {
    mem_error("recording: max_frames must be greater than zero")
}

/// `max_bytes` was zero (a timeline must have a positive memory budget).
pub(crate) fn zero_max_bytes() -> KernelError {
    mem_error("recording: max_bytes must be greater than zero")
}

/// A single capture is larger than the whole timeline budget; it can never fit.
pub(crate) fn capture_too_large() -> KernelError {
    mem_error("recording: capture byte length exceeds the timeline's max_bytes")
}

/// The requested frame is not present (never recorded, or already evicted).
pub(crate) fn frame_not_present() -> KernelError {
    mem_error("recording: frame is not present in the timeline (missing or evicted)")
}

/// A timeline operation needed at least one frame but the timeline is empty.
pub(crate) fn timeline_empty() -> KernelError {
    mem_error("recording: the timeline is empty")
}

/// Stepping the scrub selection backward when no earlier retained frame exists.
pub(crate) fn no_previous_frame() -> KernelError {
    mem_error("recording: no previous retained frame to step to")
}

/// Stepping the scrub selection forward when no later retained frame exists.
pub(crate) fn no_next_frame() -> KernelError {
    mem_error("recording: no next retained frame to step to")
}

/// Comparing two timelines whose lengths differ.
pub(crate) fn timeline_length_mismatch() -> KernelError {
    mem_error("recording: timelines have different lengths")
}

/// Comparing two timelines whose aligned frame indices differ.
pub(crate) fn timeline_frame_index_mismatch() -> KernelError {
    mem_error("recording: timelines have different frame indices at the same position")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_errors_carry_the_memory_out_of_bounds_identity() {
        let errors = [
            zero_max_frames(),
            zero_max_bytes(),
            capture_too_large(),
            frame_not_present(),
            timeline_empty(),
            no_previous_frame(),
            no_next_frame(),
            timeline_length_mismatch(),
            timeline_frame_index_mismatch(),
        ];
        errors.iter().for_each(|e| {
            assert_eq!(e.scope(), KernelErrorScope::Memory);
            assert_eq!(e.code(), KernelErrorCode::OutOfBounds);
            assert!(!e.message().is_empty());
        });
    }
}
