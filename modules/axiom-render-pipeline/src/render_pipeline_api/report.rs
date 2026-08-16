//! The render **report** accessors: reading back what a submitted frame drew.
//!
//! Split out of `render_pipeline_api.rs` rather than trimmed to fit. That file
//! sat at 996 lines against the `engine_no_large_files` hard cap of 1000, and
//! adding the one test that closed a coverage hole in `report_draw_specular`
//! pushed it to 1003. Shrinking a comment to squeeze back under would have left
//! the next edit facing the same wall with less room, so the file gave up the
//! piece that was most separable instead.
//!
//! This family is exactly that piece: seventeen total accessors over
//! [`RenderReport`], every one of them a field read or a `Vec` index, with no
//! dependency on the submit path that produces the report. Moving them costs the
//! reader nothing — a caller reaching for `report_draw_color` was never going to
//! find it by reading `submit` — and it leaves the pipeline file holding only the
//! work that actually resolves a frame.
//!
//! No public surface changes: these are further `impl RenderPipelineApi` methods
//! on the module's single facade, the same shape `axiom-windowing` uses for its
//! platform arm.

use super::{RenderPipelineApi, RenderReport};
use axiom_host::SdfScene;
use axiom_math::Mat4;

impl RenderPipelineApi {

    pub fn report_command_count(&self, report: &RenderReport) -> usize {
        report.command_count
    }

    pub fn report_clear_color(&self, report: &RenderReport) -> [f32; 4] {
        report.clear_color
    }

    /// The view-projection: multiply by an object's world matrix to get its
    /// model-view-projection. **Note (M2):** this currently bakes the wgpu depth
    /// remap (`GL_TO_WGPU_DEPTH`) even though the same report also feeds the
    /// Canvas2D backend; the backend-neutral end-state applies that remap in the
    /// wgpu consumer (as `axiom-webgpu`'s live present does). See
    /// `GL_TO_WGPU_DEPTH`'s doc for why neutralizing it here is a coordinated
    /// cross-module follow-up.
    pub fn report_view_projection(&self, report: &RenderReport) -> Mat4 {
        report.view_projection
    }

    pub fn report_draw_count(&self, report: &RenderReport) -> usize {
        report.draws.len()
    }

    /// The world matrix of the `i`-th drawn object, if present.
    pub fn report_draw_world(&self, report: &RenderReport, i: usize) -> Option<Mat4> {
        report.draws.get(i).map(|(world, ..)| *world)
    }

    /// The colour of the `i`-th drawn object, if present.
    pub fn report_draw_color(&self, report: &RenderReport, i: usize) -> Option<[f32; 4]> {
        report.draws.get(i).map(|(_, color, ..)| *color)
    }

    /// The linear-RGB self-illumination of the `i`-th drawn object, if present —
    /// its material's `emissive`, carried as its own per-draw term (never folded
    /// into the colour, which every backend modulates by light). `[0, 0, 0]` for
    /// a non-emissive material, so this is a no-op for existing frames.
    pub fn report_draw_emissive(&self, report: &RenderReport, i: usize) -> Option<[f32; 3]> {
        report.draws.get(i).map(|(_, _, e, ..)| *e)
    }

    /// How strongly the `i`-th drawn object catches a view-dependent specular
    /// highlight, if present — its material's authored `roughness`, inverted
    /// (`0` matte … `1` mirror-smooth). Carried as its own per-draw term for the
    /// same reason as emissive: it is not a reflectance, so it cannot be folded
    /// into the colour. `0` for a fully-rough material, so this is a no-op for a
    /// frame whose materials never set roughness.
    pub fn report_draw_specular(&self, report: &RenderReport, i: usize) -> Option<f32> {
        report.draws.get(i).map(|(_, _, _, s, ..)| *s)
    }

    /// The mesh id of the `i`-th drawn object, if present. Lets a caller group
    /// draws by mesh for per-mesh instance batching.
    pub fn report_draw_mesh_id(&self, report: &RenderReport, i: usize) -> Option<u64> {
        report
            .draws
            .get(i)
            .map(|(_, _, _, _, mesh_id, _, _, _)| *mesh_id)
    }

    /// The material id of the `i`-th drawn object, if present. Lets a caller
    /// group draws by `(mesh, material)` and bind the matching texture.
    pub fn report_draw_material_id(&self, report: &RenderReport, i: usize) -> Option<u64> {
        report
            .draws
            .get(i)
            .map(|(_, _, _, _, _, material_id, _, _)| *material_id)
    }

    /// Whether the `i`-th drawn object is a contact-shadow caster (a discrete
    /// dynamic object the scene marked), if present. A grounding backend shadows
    /// only the `true` draws; level geometry is `false`.
    pub fn report_draw_casts_shadow(&self, report: &RenderReport, i: usize) -> Option<bool> {
        report
            .draws
            .get(i)
            .map(|(_, _, _, _, _, _, _, casts)| *casts)
    }

    /// The appearance program the `i`-th drawn object's material names, if
    /// present — the content digest of an authored surface description, or `0`
    /// for the engine's built-in fixed material path. Carried per draw for the
    /// same reason as the mesh and material ids: it is a batching key a
    /// consumer groups by, never a per-instance value.
    pub fn report_draw_surface_program(&self, report: &RenderReport, i: usize) -> Option<u64> {
        report
            .draws
            .get(i)
            .map(|(_, _, _, _, _, _, program, _)| *program)
    }

    /// The directional shadow caster's wgpu-ready light view-projection
    /// (column-major, 16 floats). The backend renders a shadow map through this
    /// and re-projects fragments into it; identity disables shadows.
    pub fn report_light_view_proj(&self, report: &RenderReport) -> [f32; 16] {
        report.light_view_proj.as_cols_array()
    }

    /// How many lights this frame resolved.
    pub fn report_light_count(&self, report: &RenderReport) -> usize {
        report.lights.len()
    }

    /// The `i`-th resolved light: `(kind, vec, colour, intensity)` — `kind` is
    /// `0` directional / `1` point; `vec` is the world to-light direction
    /// (directional) or world position (point). `None` if `i` is out of range.
    pub fn report_light_at(
        &self,
        report: &RenderReport,
        i: usize,
    ) -> Option<(u32, [f32; 3], [f32; 3], f32)> {
        report.lights.get(i).copied()
    }

    /// The frame's backend-neutral SDF scene, if it carries SDF shapes and a
    /// camera. A live/canvas backend attaches this to its `FramePacket`
    /// (`FramePacket::with_sdf`) to march and composite the shapes against the
    /// meshes; `None` means the frame has no SDF content to march.
    pub fn report_sdf_scene<'a>(&self, report: &'a RenderReport) -> Option<&'a SdfScene> {
        report.sdf.as_ref()
    }

    pub fn report_presented(&self, report: &RenderReport) -> bool {
        report.presented
    }

    pub fn report_recorded(&self, report: &RenderReport) -> bool {
        report.recorded
    }
}
