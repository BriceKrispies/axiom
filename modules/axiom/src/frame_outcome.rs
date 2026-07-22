//! The deterministic result of one engine frame.

use std::collections::HashMap;

use axiom_host::{FrameAmbient, FramePostProcess, SdfScene};

/// One drawn object: its wgpu-ready model-view-projection matrix and its
/// world (model) matrix (both column-major, 16 floats), its linear RGBA colour,
/// and the ids of the mesh it draws and the material it uses. The world matrix
/// rides alongside the MVP so the fragment shader can recover each pixel's world
/// position for point-light distance/direction; draws still group into
/// per-`(mesh, material)` instance batches for the matching albedo texture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawData {
    mvp: [f32; 16],
    world: [f32; 16],
    color: [f32; 4],
    mesh_id: u64,
    material_id: u64,
    casts_contact_shadow: bool,
}

impl DrawData {
    pub(crate) const fn new(
        mvp: [f32; 16],
        world: [f32; 16],
        color: [f32; 4],
        mesh_id: u64,
        material_id: u64,
        casts_contact_shadow: bool,
    ) -> Self {
        DrawData {
            mvp,
            world,
            color,
            mesh_id,
            material_id,
            casts_contact_shadow,
        }
    }

    /// The column-major model-view-projection matrix.
    pub const fn mvp(&self) -> [f32; 16] {
        self.mvp
    }

    /// The column-major world (model) matrix.
    pub const fn world(&self) -> [f32; 16] {
        self.world
    }

    /// The linear RGBA colour.
    pub const fn color(&self) -> [f32; 4] {
        self.color
    }

    /// The id of the mesh this object draws.
    pub const fn mesh_id(&self) -> u64 {
        self.mesh_id
    }

    /// The id of the material this object uses (selects its albedo texture).
    pub const fn material_id(&self) -> u64 {
        self.material_id
    }

    /// Whether this draw is a discrete dynamic object the scene marked as a
    /// contact-shadow caster (level geometry is `false`). A grounding backend
    /// (the software canvas) projects a shadow only for the `true` draws.
    pub const fn casts_contact_shadow(&self) -> bool {
        self.casts_contact_shadow
    }
}

/// One resolved light for a frame: a kind (`0` directional / `1` point), a
/// world-space geometry vector (to-light direction for directional, world
/// position for point), a linear-RGB colour, and an intensity. Plain data the
/// live backend uploads into its lighting uniform each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightData {
    kind: u32,
    vec: [f32; 3],
    color: [f32; 3],
    intensity: f32,
}

impl LightData {
    pub(crate) const fn new(kind: u32, vec: [f32; 3], color: [f32; 3], intensity: f32) -> Self {
        LightData {
            kind,
            vec,
            color,
            intensity,
        }
    }

    /// `0` = directional, `1` = point.
    pub const fn kind(&self) -> u32 {
        self.kind
    }

    /// World to-light direction (directional) or world position (point).
    pub const fn vec(&self) -> [f32; 3] {
        self.vec
    }

    /// Linear-RGB colour.
    pub const fn color(&self) -> [f32; 3] {
        self.color
    }

    /// Light intensity (a non-negative scalar multiplier).
    pub const fn intensity(&self) -> f32 {
        self.intensity
    }
}

/// One **skinned** draw: a mesh deformed by a per-draw joint-matrix palette
/// (linear blend skinning). Unlike [`DrawData`] a skinned draw cannot be
/// instanced — each carries its own palette — so skinned draws are collected
/// separately and rendered one draw per entry. `joints` is the column-major joint
/// palette the vertex shader blends by the mesh's per-vertex weights.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinnedDraw {
    mvp: [f32; 16],
    world: [f32; 16],
    color: [f32; 4],
    mesh_id: u64,
    material_id: u64,
    joints: Vec<[f32; 16]>,
}

impl SkinnedDraw {
    pub(crate) fn new(
        mvp: [f32; 16],
        world: [f32; 16],
        color: [f32; 4],
        mesh_id: u64,
        material_id: u64,
        joints: Vec<[f32; 16]>,
    ) -> Self {
        SkinnedDraw {
            mvp,
            world,
            color,
            mesh_id,
            material_id,
            joints,
        }
    }

    /// The column-major model-view-projection matrix.
    pub const fn mvp(&self) -> [f32; 16] {
        self.mvp
    }

    /// The column-major world (model) matrix.
    pub const fn world(&self) -> [f32; 16] {
        self.world
    }

    /// The linear RGBA colour.
    pub const fn color(&self) -> [f32; 4] {
        self.color
    }

    /// The id of the skinned mesh this draws.
    pub const fn mesh_id(&self) -> u64 {
        self.mesh_id
    }

    /// The id of the material this uses.
    pub const fn material_id(&self) -> u64 {
        self.material_id
    }

    /// The column-major joint-matrix palette blended by the per-vertex weights.
    pub fn joints(&self) -> &[[f32; 16]] {
        &self.joints
    }
}

/// The deterministic summary of one [`crate::prelude::App`] frame: the tick, the
/// GPU command count, the clear colour, the per-object draw data, and the
/// backend flags. Equal inputs at the same tick produce an equal `FrameOutcome`.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameOutcome {
    tick: u64,
    command_count: usize,
    clear_color: [f32; 4],
    draws: Vec<DrawData>,
    skinned_draws: Vec<SkinnedDraw>,
    lights: Vec<LightData>,
    light_view_proj: [f32; 16],
    camera_view_proj: [f32; 16],
    /// The frame's backend-neutral SDF scene, if it carries any SDF shapes and a
    /// camera — the raymarched primitives a live/canvas backend composites with
    /// the meshes. `None` when the frame has no SDF content.
    sdf: Option<SdfScene>,
    /// The frame's hemisphere ambient — the sky/ground fill every backend lights
    /// unlit faces with. Carried on the frame (like the lights and clear colour)
    /// so the offscreen capture and the live present arm light identically from
    /// the app's authored value, instead of each hardcoding a dim default.
    ambient: FrameAmbient,
    /// The frame's tonemap/colour grade — the exposure/white-balance/contrast/
    /// saturation post-process every backend applies to its presented pixels.
    /// Carried on the frame like the ambient (the app's authored render-look) so
    /// the offscreen capture and the live present arm grade identically from the
    /// app's authored value; `None` presents untonemapped, exactly as before.
    postprocess: Option<FramePostProcess>,
    presented: bool,
    recorded: bool,
}

impl FrameOutcome {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        tick: u64,
        command_count: usize,
        clear_color: [f32; 4],
        draws: Vec<DrawData>,
        lights: Vec<LightData>,
        light_view_proj: [f32; 16],
        camera_view_proj: [f32; 16],
        sdf: Option<SdfScene>,
        presented: bool,
        recorded: bool,
    ) -> Self {
        FrameOutcome {
            tick,
            command_count,
            clear_color,
            draws,
            skinned_draws: Vec::new(),
            lights,
            light_view_proj,
            camera_view_proj,
            sdf,
            // Default to the engine hemisphere; `with_ambient` overrides it with
            // the app's authored value. A frame that never sets ambient renders
            // exactly as before.
            ambient: FrameAmbient::default_hemisphere(),
            // No grade by default; `with_postprocess` overrides it with the app's
            // authored grade. A frame that never sets one presents untonemapped.
            postprocess: None,
            presented,
            recorded,
        }
    }

    /// Attach the frame's skinned draws (each a mesh + its own joint palette).
    /// Empty on a frame with no skinned meshes.
    pub(crate) fn with_skinned_draws(mut self, skinned_draws: Vec<SkinnedDraw>) -> Self {
        self.skinned_draws = skinned_draws;
        self
    }

    /// Set the frame's hemisphere ambient (the app's authored sky/ground fill).
    pub(crate) fn with_ambient(mut self, ambient: FrameAmbient) -> Self {
        self.ambient = ambient;
        self
    }

    /// The frame's hemisphere ambient — the sky/ground fill lighting unlit faces.
    pub const fn ambient(&self) -> FrameAmbient {
        self.ambient
    }

    /// Set the frame's tonemap/colour grade (the app's authored render-look post
    /// process). Carried as an `Option` so an ungraded frame threads `None`
    /// through unchanged.
    pub(crate) fn with_postprocess(mut self, postprocess: Option<FramePostProcess>) -> Self {
        self.postprocess = postprocess;
        self
    }

    /// The frame's tonemap/colour grade, or `None` when the app authored none (the
    /// backend then presents untonemapped). Every backend — the offscreen capture
    /// and the live present arm — applies this to its presented pixels.
    pub const fn postprocess(&self) -> Option<FramePostProcess> {
        self.postprocess
    }

    /// The frame's skinned draws, in submission order.
    pub fn skinned_draws(&self) -> &[SkinnedDraw] {
        &self.skinned_draws
    }

    /// The identity matrix as a column-major array (the no-shadow light VP).
    const IDENTITY_MAT4: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    /// A simulation-only outcome (rendering disabled): no commands, no draws, no
    /// lights.
    pub(crate) fn simulation_only(tick: u64, clear_color: [f32; 4]) -> Self {
        FrameOutcome::new(
            tick,
            0,
            clear_color,
            Vec::new(),
            Vec::new(),
            Self::IDENTITY_MAT4,
            Self::IDENTITY_MAT4,
            None,
            false,
            false,
        )
    }

    /// The tick this outcome was produced at.
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// The number of GPU commands the frame submitted.
    pub const fn command_count(&self) -> usize {
        self.command_count
    }

    /// The frame's clear colour.
    pub const fn clear_color(&self) -> [f32; 4] {
        self.clear_color
    }

    /// The per-object draw data, in submission order.
    pub fn draws(&self) -> &[DrawData] {
        &self.draws
    }

    /// The frame's resolved lights, in scene order.
    pub fn lights(&self) -> &[LightData] {
        &self.lights
    }

    /// The directional shadow caster's wgpu-ready light view-projection
    /// (column-major, 16 floats). The live backend renders a shadow map through
    /// this and re-projects fragments into it; identity disables shadows.
    pub fn light_view_proj(&self) -> [f32; 16] {
        self.light_view_proj
    }

    /// The camera's column-major view-projection (`projection * view`, with the
    /// backend depth remap baked in — the same matrix used to build each draw's
    /// `mvp`). A backend that needs to rasterize world-space geometry it derives
    /// itself (e.g. the canvas planar-shadow projection of an object onto the
    /// ground) projects through this. Identity in a simulation-only frame.
    pub fn camera_view_proj(&self) -> [f32; 16] {
        self.camera_view_proj
    }

    /// The frame's backend-neutral SDF scene, if it carries SDF shapes and a
    /// camera. A live/canvas backend attaches this to its `FramePacket`
    /// (`FramePacket::with_sdf`) to march and composite the raymarched shapes
    /// against the rasterized meshes; `None` means no SDF content this frame.
    pub fn sdf_scene(&self) -> Option<&SdfScene> {
        self.sdf.as_ref()
    }

    /// Whether the backend presented real pixels.
    pub const fn presented(&self) -> bool {
        self.presented
    }

    /// Whether a recording backend produced this outcome.
    pub const fn recorded(&self) -> bool {
        self.recorded
    }

    /// Pack the per-object draws into the live backend's instance layout: each
    /// draw contributes its 16 MVP floats, then its 16 world-matrix floats, then
    /// its 4 colour floats (36 floats per instance), in submission order. The
    /// world matrix lets the shader recover world position for point lighting.
    /// This is the plain data the windowing run loop presents each frame.
    pub fn instance_floats(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.draws.len() * 36);
        self.draws.iter().for_each(|draw| {
            out.extend_from_slice(&draw.mvp);
            out.extend_from_slice(&draw.world);
            out.extend_from_slice(&draw.color);
        });
        out
    }

    /// Group the per-object draws into **per-`(mesh, material)` instance batches**
    /// for the multi-mesh, multi-material live backend: `(mesh_id, material_id,
    /// [mvp(16), world(16), colour(4)] per instance, count)`, one entry per
    /// distinct `(mesh, material)` pair in first-appearance order. This is the
    /// plain data the multi-mesh run loop presents each frame; the backend draws
    /// each batch against the matching uploaded mesh with the material's albedo
    /// bound.
    pub fn mesh_batches(&self) -> Vec<(u64, u64, Vec<f32>, u32)> {
        let mut order: Vec<(u64, u64)> = Vec::new();
        let mut packed: HashMap<(u64, u64), Vec<f32>> = HashMap::new();
        self.draws.iter().for_each(|draw| {
            let key = (draw.mesh_id, draw.material_id);
            let floats = packed.entry(key).or_insert_with(|| {
                order.push(key);
                Vec::new()
            });
            floats.extend_from_slice(&draw.mvp);
            floats.extend_from_slice(&draw.world);
            floats.extend_from_slice(&draw.color);
        });
        order
            .into_iter()
            .map(|(mesh_id, material_id)| {
                let floats = packed.remove(&(mesh_id, material_id)).unwrap_or_default();
                let count = (floats.len() / 36) as u32;
                (mesh_id, material_id, floats, count)
            })
            .collect()
    }

    /// The per-instance `casts_contact_shadow` flags in the SAME order
    /// [`Self::mesh_batches`] lays its instances out (each `(mesh, material)`
    /// batch in first-appearance order, instances within it in draw order). A
    /// backend that expands the batches back into per-object draws (the canvas
    /// path) indexes this by the running instance position to recover each draw's
    /// caster mark, which the float-packed batches cannot carry.
    pub fn mesh_batch_casters(&self) -> Vec<bool> {
        let mut order: Vec<(u64, u64)> = Vec::new();
        let mut packed: HashMap<(u64, u64), Vec<bool>> = HashMap::new();
        self.draws.iter().for_each(|draw| {
            let key = (draw.mesh_id, draw.material_id);
            let casts = packed.entry(key).or_insert_with(|| {
                order.push(key);
                Vec::new()
            });
            casts.push(draw.casts_contact_shadow);
        });
        order
            .into_iter()
            .flat_map(|key| packed.remove(&key).unwrap_or_default())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_floats_pack_mvp_world_then_colour_per_draw() {
        let outcome = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            vec![
                DrawData::new([1.0; 16], [9.0; 16], [0.1, 0.2, 0.3, 1.0], 1, 1, false),
                DrawData::new([2.0; 16], [8.0; 16], [0.4, 0.5, 0.6, 1.0], 1, 1, true),
            ],
            Vec::new(),
            [0.0; 16],
            [4.0; 16],
            None,
            false,
            true,
        );
        assert_eq!(outcome.camera_view_proj(), [4.0; 16]);
        assert!(!outcome.draws()[0].casts_contact_shadow());
        assert!(outcome.draws()[1].casts_contact_shadow());
        let floats = outcome.instance_floats();
        assert_eq!(floats.len(), 72); // 2 draws x (16 mvp + 16 world + 4 colour)
        assert_eq!(&floats[0..16], &[1.0; 16]);
        assert_eq!(&floats[16..32], &[9.0; 16]);
        assert_eq!(&floats[32..36], &[0.1, 0.2, 0.3, 1.0]);
        assert_eq!(&floats[36..52], &[2.0; 16]);
        assert_eq!(&floats[52..68], &[8.0; 16]);
        assert_eq!(&floats[68..72], &[0.4, 0.5, 0.6, 1.0]);
    }

    #[test]
    fn ambient_defaults_to_hemisphere_and_with_ambient_overrides() {
        let base = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            Vec::new(),
            Vec::new(),
            [0.0; 16],
            [0.0; 16],
            None,
            false,
            false,
        );
        // A frame that never sets ambient carries the engine default hemisphere,
        // so existing apps render byte-identically.
        assert_eq!(base.ambient(), FrameAmbient::default_hemisphere());
        // `with_ambient` overrides it with the app's authored sky/ground fill.
        let daylight = FrameAmbient::new([0.66, 0.71, 0.80], [0.45, 0.42, 0.37]);
        let lit = base.with_ambient(daylight);
        assert_eq!(lit.ambient(), daylight);
        assert_ne!(lit.ambient(), FrameAmbient::default_hemisphere());
    }

    #[test]
    fn postprocess_defaults_to_none_and_with_postprocess_overrides() {
        let base = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            Vec::new(),
            Vec::new(),
            [0.0; 16],
            [0.0; 16],
            None,
            false,
            false,
        );
        // A frame that never sets a grade presents untonemapped, so existing apps
        // render byte-identically.
        assert_eq!(base.postprocess(), None);
        // `with_postprocess` overrides it with the app's authored grade, and
        // threading `None` back through leaves the frame ungraded.
        let graded = base.clone().with_postprocess(Some(FramePostProcess::cinematic()));
        assert_eq!(graded.postprocess(), Some(FramePostProcess::cinematic()));
        assert_eq!(base.with_postprocess(None).postprocess(), None);
    }

    #[test]
    fn sdf_scene_round_trips_present_and_absent() {
        let scene = SdfScene::new(
            Vec::new(),
            [0.0; 16],
            [0.0; 16],
            [1.0, 2.0, 3.0],
            [100.0, 0.001, 0.0, 0.0],
        );
        let with = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            Vec::new(),
            Vec::new(),
            [0.0; 16],
            [0.0; 16],
            Some(scene.clone()),
            false,
            false,
        );
        assert_eq!(with.sdf_scene(), Some(&scene));
        assert!(FrameOutcome::simulation_only(0, [0.0; 4])
            .sdf_scene()
            .is_none());
    }

    #[test]
    fn instance_floats_empty_when_no_draws() {
        assert!(FrameOutcome::simulation_only(3, [0.0; 4])
            .instance_floats()
            .is_empty());
        assert!(FrameOutcome::simulation_only(3, [0.0; 4])
            .mesh_batches()
            .is_empty());
        assert!(FrameOutcome::simulation_only(3, [0.0; 4])
            .lights()
            .is_empty());
    }

    #[test]
    fn mesh_batches_group_draws_by_mesh_and_material_in_first_appearance_order() {
        // Same mesh (7), two materials (5, 6): a batch keys on the (mesh,
        // material) pair, not the mesh alone.
        let outcome = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            vec![
                DrawData::new([1.0; 16], [9.0; 16], [0.1, 0.2, 0.3, 1.0], 7, 5, true),
                DrawData::new([2.0; 16], [8.0; 16], [0.4, 0.5, 0.6, 1.0], 7, 6, false),
                DrawData::new([3.0; 16], [7.0; 16], [0.7, 0.8, 0.9, 1.0], 7, 5, true),
            ],
            Vec::new(),
            [0.0; 16],
            [0.0; 16],
            None,
            true,
            false,
        );
        assert_eq!(outcome.draws()[0].mesh_id(), 7);
        assert_eq!(outcome.draws()[0].material_id(), 5);
        assert_eq!(outcome.draws()[1].material_id(), 6);
        assert_eq!(outcome.draws()[0].mvp(), [1.0; 16]);
        assert_eq!(outcome.draws()[0].world(), [9.0; 16]);
        assert_eq!(outcome.draws()[0].color(), [0.1, 0.2, 0.3, 1.0]);

        let batches = outcome.mesh_batches();
        assert_eq!(batches.len(), 2);
        // First-appearance order: (7,5) first (2 instances), then (7,6) (1).
        assert_eq!((batches[0].0, batches[0].1), (7, 5));
        assert_eq!(batches[0].3, 2);
        assert_eq!(batches[0].2.len(), 72); // 2 instances x 36 floats
        assert_eq!(&batches[0].2[0..16], &[1.0; 16]);
        assert_eq!(&batches[0].2[36..52], &[3.0; 16]);
        assert_eq!((batches[1].0, batches[1].1), (7, 6));
        assert_eq!(batches[1].3, 1);
        assert_eq!(&batches[1].2[0..16], &[2.0; 16]);

        // The caster flags follow the same expansion order as the batches above.
        assert_eq!(outcome.mesh_batch_casters(), vec![true, true, false]);
    }

    #[test]
    fn lights_round_trip_through_the_outcome() {
        let outcome = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            Vec::new(),
            vec![
                LightData::new(0, [-0.3, 1.0, -0.4], [1.0, 1.0, 1.0], 1.0),
                LightData::new(1, [2.0, 3.0, -4.0], [1.0, 0.0, 0.0], 2.5),
            ],
            [5.0; 16],
            [0.0; 16],
            None,
            false,
            true,
        );
        assert_eq!(outcome.light_view_proj(), [5.0; 16]);
        assert_eq!(outcome.lights().len(), 2);
        assert_eq!(outcome.lights()[0].kind(), 0);
        assert_eq!(outcome.lights()[0].vec(), [-0.3, 1.0, -0.4]);
        assert_eq!(outcome.lights()[1].kind(), 1);
        assert_eq!(outcome.lights()[1].vec(), [2.0, 3.0, -4.0]);
        assert_eq!(outcome.lights()[1].color(), [1.0, 0.0, 0.0]);
        assert_eq!(outcome.lights()[1].intensity(), 2.5);
    }
}
