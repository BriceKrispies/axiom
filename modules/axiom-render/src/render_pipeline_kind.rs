//! Pipeline kind markers used by the vertical slice.

/// The vertical slice supports a single pipeline today: the basic-lit
/// forward shader. The marker is a `u32` constant so callers can
/// switch on it without naming any render-internal enum.
#[derive(Debug, Clone, Copy)]
pub struct RenderPipelineKind;

impl RenderPipelineKind {
    /// The basic-lit forward pipeline marker.
    pub const BASIC_LIT: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_lit_marker_is_stable() {
        assert_eq!(RenderPipelineKind::BASIC_LIT, 1);
    }
}
