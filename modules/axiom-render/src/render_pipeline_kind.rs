//! Pipeline kind markers the render contract selects between, per object.

/// The pipeline markers a [`crate::RenderObject`] can select. Each is a `u32`
/// constant so callers switch on it without naming any render-internal enum, and
/// so the per-object `pipeline` id rides through the command stream as plain
/// data a backend maps to a concrete GPU pipeline. `BASIC_LIT` is the default;
/// `UNLIT` is carried so the contract can express a second pipeline today (the
/// render module emits the selection; wiring a distinct shader behind `UNLIT` is
/// a backend's job).
#[derive(Debug, Clone, Copy)]
pub struct RenderPipelineKind;

impl RenderPipelineKind {
    /// The basic-lit forward pipeline marker (the default).
    pub const BASIC_LIT: u32 = 1;

    /// The unlit/emissive forward pipeline marker.
    pub const UNLIT: u32 = 2;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_are_stable_and_distinct() {
        assert_eq!(RenderPipelineKind::BASIC_LIT, 1);
        assert_eq!(RenderPipelineKind::UNLIT, 2);
        assert_ne!(RenderPipelineKind::BASIC_LIT, RenderPipelineKind::UNLIT);
    }
}
