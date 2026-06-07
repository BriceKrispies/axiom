//! One externally-supplied host frame pulse as explicit data.

use crate::host_viewport::HostViewport;

/// One externally-supplied host frame pulse.
///
/// **Layer 03 never reads the clock.** Every timing value here is supplied
/// by the host. `elapsed_nanos` is a `u64` so a negative elapsed time cannot
/// be expressed by the type at all; the optional `presentation_nanos` is
/// likewise an explicit monotonic value picked by the host adapter (e.g. a
/// future browser layer that wants to anchor on `requestAnimationFrame`
/// timestamps).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostFrameInput {
    sequence: u64,
    elapsed_nanos: u64,
    presentation_nanos: Option<u64>,
    viewport: HostViewport,
}

impl HostFrameInput {
    /// Construct a frame input from a strictly-monotone host sequence
    /// number, the host's measured elapsed time since the previous frame,
    /// and the current validated viewport.
    ///
    /// `elapsed_nanos == 0` is intentionally allowed: a host adapter may
    /// pulse the boundary with no time delta (e.g. on a paused frame, or on
    /// a forced re-layout), and the boundary's planning logic correctly
    /// produces zero runtime steps for it.
    pub const fn new(sequence: u64, elapsed_nanos: u64, viewport: HostViewport) -> Self {
        HostFrameInput {
            sequence,
            elapsed_nanos,
            presentation_nanos: None,
            viewport,
        }
    }

    /// Attach an explicit presentation timestamp supplied by the host.
    pub const fn with_presentation_nanos(mut self, presentation_nanos: u64) -> Self {
        self.presentation_nanos = Some(presentation_nanos);
        self
    }

    /// The host's monotonically-increasing frame sequence number.
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Elapsed nanoseconds since the previous host frame, supplied by the
    /// host. Never read from a system clock.
    pub const fn elapsed_nanos(&self) -> u64 {
        self.elapsed_nanos
    }

    /// Optional presentation timestamp, if the host adapter provided one.
    pub const fn presentation_nanos(&self) -> Option<u64> {
        self.presentation_nanos
    }

    /// The validated viewport in effect for this frame.
    pub const fn viewport(&self) -> &HostViewport {
        &self.viewport
    }

    /// Whether `next` is a strictly-later frame than `self`. Used by the
    /// step driver to reject out-of-order host pulses.
    pub const fn precedes(&self, next: &HostFrameInput) -> bool {
        self.sequence < next.sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Ratio;

    fn vp() -> HostViewport {
        HostViewport::new(800, 600, Ratio::new(1.0).unwrap()).unwrap()
    }

    #[test]
    fn valid_frame_input_creation() {
        let f = HostFrameInput::new(1, 16_666_667, vp());
        assert_eq!(f.sequence(), 1);
        assert_eq!(f.elapsed_nanos(), 16_666_667);
        assert!(f.presentation_nanos().is_none());
        assert_eq!(f.viewport(), &vp());
    }

    #[test]
    fn zero_elapsed_is_representable() {
        let f = HostFrameInput::new(1, 0, vp());
        assert_eq!(f.elapsed_nanos(), 0);
    }

    #[test]
    fn negative_elapsed_is_impossible_by_type() {
        // The field is `u64`, so this is a compile-time proof: trying to
        // construct with a negative value cannot type-check.
        let f = HostFrameInput::new(1, u64::MAX, vp());
        assert_eq!(f.elapsed_nanos(), u64::MAX);
    }

    #[test]
    fn with_presentation_nanos_attaches_value() {
        let f = HostFrameInput::new(1, 16_666_667, vp()).with_presentation_nanos(1_234_567);
        assert_eq!(f.presentation_nanos(), Some(1_234_567));
    }

    #[test]
    fn precedes_orders_by_sequence() {
        let a = HostFrameInput::new(1, 16_666_667, vp());
        let b = HostFrameInput::new(2, 16_666_667, vp());
        assert!(a.precedes(&b));
        assert!(!b.precedes(&a));
        assert!(!a.precedes(&a));
    }

    #[test]
    fn invalid_viewport_fails_before_frame_input_exists() {
        // A frame input cannot wrap a viewport that did not validate — the
        // viewport constructor is the choke point. This test pins that
        // contract.
        let err = HostViewport::new(0, 100, Ratio::new(1.0).unwrap()).unwrap_err();
        assert_eq!(
            err.code(),
            crate::host_error_code::HostErrorCode::InvalidViewportDimensions
        );
    }

    #[test]
    fn same_inputs_produce_equal_frame_inputs() {
        let a = HostFrameInput::new(5, 100, vp()).with_presentation_nanos(9);
        let b = HostFrameInput::new(5, 100, vp()).with_presentation_nanos(9);
        assert_eq!(a, b);
    }
}
