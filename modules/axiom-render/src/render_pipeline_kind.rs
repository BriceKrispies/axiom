//! Pipeline kind markers the render contract selects between, per object.

/// The pipeline markers a [`crate::RenderObject`] can select. Each is a `u32`
/// constant so callers switch on it without naming any render-internal enum, and
/// so the per-object `pipeline` id rides through the command stream as plain
/// data a backend maps to a concrete GPU pipeline. `BASIC_LIT` is the default.
///
/// **`UNLIT` now has something behind it.** For most of this module's life it
/// was a marker nothing selected: an app could hand an object the number, the
/// command builder run-length-encoded a `SetPipeline` for it, and the value died
/// at the `axiom_host::FramePacket` boundary, which carries no pipeline lane.
/// It is now *derived* — [`crate::draw_order`] selects it for every draw whose
/// material's [`axiom_surface::LightingModel`] is `Unlit` — and the GPU backend
/// derives the identical marker from the identical discriminant on its own side,
/// from the surface the draw's `surface_program` digest already names. So the
/// two ends of the seam agree because they read the same layer's type, not
/// because two numbers were guessed twice.
///
/// The marker stays a *selection*, not a promise of a second shader: a backend
/// is free to answer it with one program that gates its lighting internally,
/// which is exactly what `axiom-gpu-backend` does, and is why three lighting
/// models cost it zero additional pipelines.
#[derive(Debug, Clone, Copy)]
pub struct RenderPipelineKind;

impl RenderPipelineKind {
    /// The basic-lit forward pipeline marker (the default) — selected by
    /// `axiom_surface::LightingModel::Lambert` and `LambertSpecular`.
    pub const BASIC_LIT: u32 = 1;

    /// The unlit/emissive forward pipeline marker — selected by
    /// `axiom_surface::LightingModel::Unlit`.
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
