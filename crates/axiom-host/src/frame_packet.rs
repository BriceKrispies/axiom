//! The backend-neutral, primitive-only frame presentation packet.
//!
//! `FramePacket` is the single artifact every render backend consumes. It is
//! derived from a render command list by `axiom-render` and handed to the GPU
//! backend now (and the Canvas 2D backend later), so both present the *same*
//! frame structure. It carries only primitives — no GPU, browser, DOM,
//! render-module, or scene types — so it is a stable presentation-boundary
//! contract any backend can name, store, and match on.
//!
//! Matrices are column-major 16-float arrays. The packet's matrices are
//! backend-neutral: `view_proj` is `projection * view` and `mvp` is
//! `projection * view * world`, with **no** backend-specific clip-space depth
//! remap baked in — applying that (e.g. the wgpu z∈[0,1] fix) is a backend
//! concern handled where the packet is consumed.

use axiom_kernel::Seconds;

use crate::sdf_scene::SdfScene;

/// The pixel dimensions of the frame's render target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameViewport {
    width: u32,
    height: u32,
}

impl FrameViewport {
    /// A viewport of `width` by `height` device pixels.
    pub const fn new(width: u32, height: u32) -> Self {
        FrameViewport { width, height }
    }

    /// The target width in device pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// The target height in device pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }
}

/// The frame's camera matrices, all column-major 16-float arrays. `view_proj`
/// is the backend-neutral `projection * view`; a backend applies its own
/// depth-range convention when it consumes the packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameCamera {
    view: [f32; 16],
    projection: [f32; 16],
    view_proj: [f32; 16],
}

impl FrameCamera {
    /// A camera from its column-major `view`, `projection`, and precomputed
    /// `view_proj` (`projection * view`) matrices.
    pub const fn new(view: [f32; 16], projection: [f32; 16], view_proj: [f32; 16]) -> Self {
        FrameCamera {
            view,
            projection,
            view_proj,
        }
    }

    /// The column-major view matrix.
    pub const fn view(&self) -> [f32; 16] {
        self.view
    }

    /// The column-major projection matrix.
    pub const fn projection(&self) -> [f32; 16] {
        self.projection
    }

    /// The column-major `projection * view` matrix.
    pub const fn view_proj(&self) -> [f32; 16] {
        self.view_proj
    }
}

/// One light for the frame: a kind (`0` directional, `1` point), a world-space
/// vector (to-light direction for directional, world position for point), and
/// the linear-RGB colour packed with its intensity as `[r, g, b, intensity]`.
///
/// Colour and intensity ride together in one `[f32; 4]` rather than as a
/// separate `[f32; 3]` colour and a naked `f32` intensity: a bare scalar `f32`
/// in a public engine API is forbidden (the `engine_no_unitless_float_public_api`
/// lint), and an array of floats is the sanctioned primitive form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameLight {
    kind: u32,
    vec: [f32; 3],
    color_intensity: [f32; 4],
}

impl FrameLight {
    /// A light with `kind` (`0` directional, `1` point), world `vec`, and
    /// `color_intensity` = `[r, g, b, intensity]`.
    pub const fn new(kind: u32, vec: [f32; 3], color_intensity: [f32; 4]) -> Self {
        FrameLight {
            kind,
            vec,
            color_intensity,
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

    /// The linear-RGB colour in `[0..3]` and the non-negative intensity in `[3]`.
    pub const fn color_intensity(&self) -> [f32; 4] {
        self.color_intensity
    }
}

/// One drawn object: a stable identity, the mesh and material it references (by
/// id, resolved against the backend's uploaded resource tables), its world and
/// model-view-projection matrices (column-major, 16 floats each), its linear
/// RGBA colour, its linear-RGB **emissive** (self-illumination) radiance, and
/// whether it casts a contact shadow (a discrete, dynamic object the scene
/// marked as a shadow-caster — level geometry leaves this `false`, so a backend
/// that grounds objects with shadows knows what to ground).
/// Objects appear in the packet in deterministic command-list draw order.
///
/// **Why emissive lives here.** `color` is a *reflectance*: every backend
/// multiplies it by the light that reaches the surface. Self-illumination is
/// not reflectance — it is radiance the surface adds regardless of the light —
/// so it cannot be folded into `color` without being wrongly scaled by N·L,
/// ambient and shadow. It is the material's second shading term, and the
/// packet is the one place both backends read a draw's shading terms from, so
/// it belongs on the draw item beside `color` rather than being re-derived per
/// backend.
///
/// **Why specular lives here too, and why it is one number.** A Lambert-only
/// surface cannot catch a highlight, so no amount of light tuning makes it read
/// as lit *by* something rather than merely bright — which is the third shading
/// term, alongside reflectance and self-illumination, and belongs in the same
/// place as those. It is a single strength rather than a strength *and* a gloss
/// exponent because the instance payload has exactly one free lane (the pad in
/// the emissive `vec4`); a second per-material lane would widen the instance
/// stride, which is a contract shared with the other packer. The engine
/// therefore has one gloss profile and materials differ in *how much* they
/// catch it, which is the axis that actually separates tarmac from paint from
/// chrome.
///
/// **Why `surface_program` is a bare `u64`, and why `0` is load-bearing.** A
/// draw may name an *appearance program* — an authored surface description —
/// instead of relying on the engine's one built-in fixed material path. The
/// packet cannot name the surface type itself: this contract is primitive-only
/// on purpose (see this module's header), and a backend must be able to store
/// and compare the identity without depending on the layer that defines it. So
/// the draw carries the surface's **content digest**, and `0` — the value
/// [`Self::new`] gives every draw — means *"the built-in fixed material path"*,
/// i.e. exactly what the engine did before surfaces existed. Every existing draw
/// therefore renders unchanged.
///
/// It is a *content* hash rather than a caller-assigned slot, so two identical
/// surfaces authored independently collapse to one program rather than
/// compiling twice. And it is a **batching key**, not an instance lane: the
/// per-draw instance payload has no free floats, so a consumer groups draws by
/// `(mesh_id, material_id, surface_program)` and leaves the stride alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameDrawItem {
    object_id: u64,
    mesh_id: u64,
    material_id: u64,
    world: [f32; 16],
    mvp: [f32; 16],
    color: [f32; 4],
    emissive: [f32; 3],
    specular: axiom_kernel::Ratio,
    surface_program: u64,
    casts_contact_shadow: bool,
}

impl FrameDrawItem {
    /// A draw item with its stable `object_id`, `mesh_id`, `material_id`,
    /// column-major `world` and `mvp` matrices, linear RGBA `color`, and whether
    /// it `casts_contact_shadow`. The draw is non-emissive; a self-illuminating
    /// material adds its radiance with [`Self::with_emissive`].
    pub const fn new(
        object_id: u64,
        mesh_id: u64,
        material_id: u64,
        world: [f32; 16],
        mvp: [f32; 16],
        color: [f32; 4],
        casts_contact_shadow: bool,
    ) -> Self {
        FrameDrawItem {
            object_id,
            mesh_id,
            material_id,
            world,
            mvp,
            color,
            emissive: [0.0; 3],
            specular: axiom_kernel::Ratio::finite_or_zero(0.0),
            surface_program: 0,
            casts_contact_shadow,
        }
    }

    /// This draw item with the material's linear-RGB self-illumination
    /// radiance, added to the shaded colour by every backend. `[0, 0, 0]` (the
    /// default) is an exact no-op, so a non-emissive draw renders unchanged.
    pub const fn with_emissive(mut self, emissive: [f32; 3]) -> Self {
        self.emissive = emissive;
        self
    }

    /// This draw item with the material's **specular strength** — how strongly
    /// the surface catches a view-dependent highlight from the frame's lights.
    /// Zero (the default) is an exact no-op, so a matte draw renders unchanged.
    /// Gated by [`crate::RenderCapability::Specular`].
    pub const fn with_specular(mut self, specular: axiom_kernel::Ratio) -> Self {
        self.specular = specular;
        self
    }

    /// This draw item with the **appearance program** its material names — the
    /// content digest of an authored surface description. `0` (the default) is
    /// the built-in fixed material path, so a draw that never calls this renders
    /// exactly as it did before surfaces existed.
    pub const fn with_surface_program(mut self, surface_program: u64) -> Self {
        self.surface_program = surface_program;
        self
    }

    /// The object's stable identity (for picking / hit-testing).
    pub const fn object_id(&self) -> u64 {
        self.object_id
    }

    /// The id of the mesh this object draws.
    pub const fn mesh_id(&self) -> u64 {
        self.mesh_id
    }

    /// The id of the material this object uses.
    pub const fn material_id(&self) -> u64 {
        self.material_id
    }

    /// The column-major world (model) matrix.
    pub const fn world(&self) -> [f32; 16] {
        self.world
    }

    /// The column-major model-view-projection matrix.
    pub const fn mvp(&self) -> [f32; 16] {
        self.mvp
    }

    /// The linear RGBA colour (reflectance — the light-modulated term).
    pub const fn color(&self) -> [f32; 4] {
        self.color
    }

    /// The linear-RGB self-illumination radiance this surface adds on top of
    /// its shaded colour, independent of any light. `[0, 0, 0]` = not emissive.
    pub const fn emissive(&self) -> [f32; 3] {
        self.emissive
    }

    /// How strongly this surface catches a view-dependent specular highlight.
    /// Zero = matte.
    pub const fn specular(&self) -> axiom_kernel::Ratio {
        self.specular
    }

    /// The appearance program this draw's material names — the content digest of
    /// an authored surface description, or `0` for the built-in fixed material
    /// path. A consumer treats it as a batching key alongside the mesh and
    /// material ids.
    pub const fn surface_program(&self) -> u64 {
        self.surface_program
    }

    /// Whether this draw is a discrete, dynamic object the scene marked as a
    /// contact-shadow caster. Level geometry (walls, floors) is `false`; a
    /// backend that grounds objects with shadows only shadows the `true` ones.
    pub const fn casts_contact_shadow(&self) -> bool {
        self.casts_contact_shadow
    }
}

/// Conservative per-frame feature metadata: which capabilities the frame relies
/// on, so a backend can report what it had to drop or approximate (e.g. a
/// software backend dropping shadows). Neutral booleans/counts only — no backend
/// semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameFeatureSet {
    uses_textures: bool,
    uses_shadows: bool,
    directional_lights: u32,
    point_lights: u32,
}

impl FrameFeatureSet {
    /// Feature metadata: whether any material samples an albedo texture, whether
    /// a directional caster wants shadows, and the directional/point light
    /// counts.
    pub const fn new(
        uses_textures: bool,
        uses_shadows: bool,
        directional_lights: u32,
        point_lights: u32,
    ) -> Self {
        FrameFeatureSet {
            uses_textures,
            uses_shadows,
            directional_lights,
            point_lights,
        }
    }

    /// Whether any material in the frame samples an albedo texture.
    pub const fn uses_textures(&self) -> bool {
        self.uses_textures
    }

    /// Whether the frame has a directional caster that wants shadows.
    pub const fn uses_shadows(&self) -> bool {
        self.uses_shadows
    }

    /// The number of directional lights in the frame.
    pub const fn directional_lights(&self) -> u32 {
        self.directional_lights
    }

    /// The number of point lights in the frame.
    pub const fn point_lights(&self) -> u32 {
        self.point_lights
    }
}

/// The backend-neutral frame packet: everything a backend needs to present one
/// frame, derived from a render command list and carrying only primitives. The
/// GPU backend consumes it today; the Canvas 2D backend will consume the same
/// type. Two packets are equal iff every field is equal.
#[derive(Debug, Clone, PartialEq)]
pub struct FramePacket {
    frame_index: u64,
    tick: u64,
    viewport: FrameViewport,
    clear_color: [f32; 4],
    camera: Option<FrameCamera>,
    draws: Vec<FrameDrawItem>,
    lights: Vec<FrameLight>,
    light_view_proj: [f32; 16],
    features: FrameFeatureSet,
    sdf: Option<SdfScene>,
    volumetrics: Option<crate::frame_volumetrics::FrameVolumetrics>,
    ambient: Option<crate::frame_ambient::FrameAmbient>,
    depth_fog: Option<crate::frame_depth_fog::FrameDepthFog>,
    postprocess: Option<crate::frame_postprocess::FramePostProcess>,
    sky: Option<crate::frame_sky::FrameSky>,
    bloom: Option<crate::frame_bloom::FrameBloom>,
    retro_32bit: Option<crate::frame_retro_32bit::FrameRetro32BitProfile>,
    time: Seconds,
}

impl FramePacket {
    /// Assemble a frame packet from its parts. `draws` are in deterministic
    /// command-list order; `light_view_proj` is the directional shadow caster's
    /// column-major light view-projection (identity disables shadows).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        frame_index: u64,
        tick: u64,
        viewport: FrameViewport,
        clear_color: [f32; 4],
        camera: Option<FrameCamera>,
        draws: Vec<FrameDrawItem>,
        lights: Vec<FrameLight>,
        light_view_proj: [f32; 16],
        features: FrameFeatureSet,
    ) -> Self {
        FramePacket {
            frame_index,
            tick,
            viewport,
            clear_color,
            camera,
            draws,
            lights,
            light_view_proj,
            features,
            sdf: None,
            volumetrics: None,
            ambient: None,
            depth_fog: None,
            postprocess: None,
            sky: None,
            bloom: None,
            retro_32bit: None,
            // The default is exactly zero, not "absent": a frame that supplies
            // no time still has one, and it is the same one every replay of it
            // has. See [`Self::with_time`].
            time: Seconds::finite_or_zero(0.0),
        }
    }

    /// Supply this frame's **presentation time** — the clock a time-varying
    /// authored surface samples.
    ///
    /// This is explicitly supplied engine time, never a wall clock. It is the
    /// only sanctioned route by which time enters an
    /// `axiom_field::FieldGraph`, and it is what makes wind and ripple
    /// replayable: the same tick presented twice produces the same displaced
    /// geometry, byte for byte.
    ///
    /// A packet that never calls this carries [`Seconds`] zero, which is why
    /// every existing frame is unchanged.
    #[must_use]
    pub const fn with_time(mut self, time: Seconds) -> Self {
        self.time = time;
        self
    }

    /// The frame's presentation time. Zero unless [`Self::with_time`] supplied
    /// one.
    pub const fn time(&self) -> Seconds {
        self.time
    }

    /// Attach an SDF raymarch scene to this packet. The raymarch pass is the
    /// peer of the triangle `draws`: a backend that renders both composites the
    /// marched SDF against the rasterized meshes through the shared depth
    /// buffer. A packet without an SDF scene (the default) renders meshes only.
    #[must_use]
    pub fn with_sdf(mut self, sdf: SdfScene) -> Self {
        self.sdf = Some(sdf);
        self
    }

    /// The frame's SDF raymarch scene, or `None` when the frame has no SDF
    /// content (meshes only).
    pub const fn sdf(&self) -> Option<&SdfScene> {
        self.sdf.as_ref()
    }

    /// Attach volumetric light (god-rays) to this frame. It is neutral frame data:
    /// every backend applies [`crate::apply_frame_volumetrics`] to its output, so the
    /// shafts render identically regardless of renderer. A packet without it (the
    /// default) has no shafts.
    #[must_use]
    pub fn with_volumetrics(
        mut self,
        volumetrics: crate::frame_volumetrics::FrameVolumetrics,
    ) -> Self {
        self.volumetrics = Some(volumetrics);
        self
    }

    /// The frame's volumetric-light parameters, or `None` when the frame has none.
    pub const fn volumetrics(&self) -> Option<&crate::frame_volumetrics::FrameVolumetrics> {
        self.volumetrics.as_ref()
    }

    /// Attach a hemisphere ambient to this frame. It is neutral frame data: every
    /// backend lights unlit faces with it, so ambient reads identically regardless of
    /// renderer. A packet without one (the default) uses
    /// [`crate::FrameAmbient::default_hemisphere`].
    #[must_use]
    pub fn with_ambient(mut self, ambient: crate::frame_ambient::FrameAmbient) -> Self {
        self.ambient = Some(ambient);
        self
    }

    /// Attach a sky to this frame. Neutral frame data: the sky's *definition* is
    /// [`crate::FrameSky::radiance`], so a backend either evaluates that same
    /// arithmetic or declares [`crate::RenderCapability::Sky`] dropped. A frame
    /// with no sky keeps its flat clear colour, unchanged.
    pub fn with_sky(mut self, sky: crate::frame_sky::FrameSky) -> Self {
        self.sky = Some(sky);
        self
    }

    /// The frame's sky, or `None` when the frame is cleared to a flat colour.
    pub const fn sky(&self) -> Option<&crate::frame_sky::FrameSky> {
        self.sky.as_ref()
    }

    /// Attach bloom to this frame. Neutral frame data, gated by
    /// [`crate::RenderCapability::PostProcess`]: a backend that cannot afford the
    /// extra render targets declares the drop rather than ignoring it.
    pub fn with_bloom(mut self, bloom: crate::frame_bloom::FrameBloom) -> Self {
        self.bloom = Some(bloom);
        self
    }

    /// The frame's bloom, or `None` when highlights are left to clip.
    pub const fn bloom(&self) -> Option<&crate::frame_bloom::FrameBloom> {
        self.bloom.as_ref()
    }

    /// The frame's hemisphere ambient, or `None` when the frame carries none (the
    /// backend then falls back to [`crate::FrameAmbient::default_hemisphere`]).
    pub const fn ambient(&self) -> Option<&crate::frame_ambient::FrameAmbient> {
        self.ambient.as_ref()
    }

    /// Attach atmospheric depth fog to this frame. It is neutral frame data: every
    /// backend mixes each pixel toward the fog colour by the *same* normalized-depth
    /// arithmetic, so aerial perspective reads identically regardless of renderer —
    /// the divergence [`crate::FrameDepthFog`] exists to close. A packet without one
    /// (the default) leaves each backend on its prior default, so no existing frame
    /// changes.
    #[must_use]
    pub fn with_depth_fog(mut self, depth_fog: crate::frame_depth_fog::FrameDepthFog) -> Self {
        self.depth_fog = Some(depth_fog);
        self
    }

    /// The frame's atmospheric depth fog, or `None` when the frame carries none.
    pub const fn depth_fog(&self) -> Option<&crate::frame_depth_fog::FrameDepthFog> {
        self.depth_fog.as_ref()
    }

    /// Attach a tonemap post-process to this frame. It is neutral frame data: every
    /// backend applies [`crate::apply_frame_postprocess`] to its output, so the filmic
    /// look reads identically regardless of renderer. A packet without one (the
    /// default) is presented untonemapped.
    #[must_use]
    pub fn with_postprocess(
        mut self,
        postprocess: crate::frame_postprocess::FramePostProcess,
    ) -> Self {
        self.postprocess = Some(postprocess);
        self
    }

    /// The frame's tonemap post-process parameters, or `None` when the frame carries
    /// none.
    pub const fn postprocess(&self) -> Option<&crate::frame_postprocess::FramePostProcess> {
        self.postprocess.as_ref()
    }

    /// Attach a retro 32-bit render profile to this frame. It is neutral frame data: the
    /// CPU-readback backends apply [`crate::apply_frame_retro_32bit`] (colour quantize +
    /// dither) to their output, and every backend reads the profile's fog / snap /
    /// internal-resolution fields for its geometry/target stages, so the
    /// retro 32-bit console look reads consistently regardless of renderer. A packet
    /// without one (the default) is presented at full fidelity.
    #[must_use]
    pub fn with_retro_32bit_profile(
        mut self,
        retro_32bit: crate::frame_retro_32bit::FrameRetro32BitProfile,
    ) -> Self {
        self.retro_32bit = Some(retro_32bit);
        self
    }

    /// The frame's retro 32-bit render profile, or `None` when the frame carries none.
    pub const fn retro_32bit(&self) -> Option<&crate::frame_retro_32bit::FrameRetro32BitProfile> {
        self.retro_32bit.as_ref()
    }

    /// The frame index this packet presents.
    pub const fn frame_index(&self) -> u64 {
        self.frame_index
    }

    /// The simulation tick this packet was produced at.
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// The render target dimensions.
    pub const fn viewport(&self) -> FrameViewport {
        self.viewport
    }

    /// The frame's clear colour (linear RGBA).
    pub const fn clear_color(&self) -> [f32; 4] {
        self.clear_color
    }

    /// The frame's camera, or `None` when the frame has no camera.
    pub const fn camera(&self) -> Option<FrameCamera> {
        self.camera
    }

    /// The per-object draws, in deterministic command-list order.
    pub fn draws(&self) -> &[FrameDrawItem] {
        &self.draws
    }

    /// The frame's lights, in input order.
    pub fn lights(&self) -> &[FrameLight] {
        &self.lights
    }

    /// The directional shadow caster's column-major light view-projection
    /// (identity disables shadows).
    pub const fn light_view_proj(&self) -> [f32; 16] {
        self.light_view_proj
    }

    /// The frame's conservative feature metadata.
    pub const fn features(&self) -> FrameFeatureSet {
        self.features
    }

    /// Whether any draw in this frame authored a non-zero specular strength.
    ///
    /// Derived from the draws rather than carried as a [`FrameFeatureSet`] flag,
    /// because it is already recorded per draw and a second, separately-authored
    /// copy could disagree with them — a backend would then report a drop for a
    /// frame with no highlights in it, or stay silent about one that had them.
    /// This is what a backend without [`crate::RenderCapability::Specular`]
    /// consults to decide whether it has something to declare.
    pub fn uses_specular(&self) -> bool {
        self.draws.iter().any(|d| d.specular().get() != 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdf_scene::{SdfPrimitive, SdfScene};

    fn mat(seed: f32) -> [f32; 16] {
        [seed; 16]
    }

    #[test]
    fn viewport_accessors_round_trip() {
        let v = FrameViewport::new(800, 600);
        assert_eq!(v.width(), 800);
        assert_eq!(v.height(), 600);
        assert_eq!(v, v);
        assert_eq!(v, FrameViewport::new(800, 600));
        assert_ne!(v, FrameViewport::new(640, 480));
        assert!(format!("{v:?}").contains("FrameViewport"));
    }

    #[test]
    fn camera_accessors_round_trip() {
        let c = FrameCamera::new(mat(1.0), mat(2.0), mat(3.0));
        assert_eq!(c.view(), mat(1.0));
        assert_eq!(c.projection(), mat(2.0));
        assert_eq!(c.view_proj(), mat(3.0));
        assert_eq!(c, FrameCamera::new(mat(1.0), mat(2.0), mat(3.0)));
        assert_ne!(c, FrameCamera::new(mat(1.0), mat(2.0), mat(9.0)));
        assert!(format!("{c:?}").contains("FrameCamera"));
    }

    #[test]
    fn light_accessors_round_trip() {
        let l = FrameLight::new(1, [2.0, 3.0, -4.0], [1.0, 0.0, 0.0, 2.5]);
        assert_eq!(l.kind(), 1);
        assert_eq!(l.vec(), [2.0, 3.0, -4.0]);
        assert_eq!(l.color_intensity(), [1.0, 0.0, 0.0, 2.5]);
        assert_ne!(
            l,
            FrameLight::new(0, [2.0, 3.0, -4.0], [1.0, 0.0, 0.0, 2.5])
        );
        assert!(format!("{l:?}").contains("FrameLight"));
    }

    #[test]
    fn draw_item_accessors_round_trip() {
        let d = FrameDrawItem::new(7, 11, 13, mat(9.0), mat(5.0), [0.1, 0.2, 0.3, 1.0], true);
        assert_eq!(d.object_id(), 7);
        assert_eq!(d.mesh_id(), 11);
        assert_eq!(d.material_id(), 13);
        assert_eq!(d.world(), mat(9.0));
        assert_eq!(d.mvp(), mat(5.0));
        assert_eq!(d.color(), [0.1, 0.2, 0.3, 1.0]);
        // A plain draw is non-emissive; `with_emissive` is the only way to add
        // self-illumination, and it leaves every other field untouched.
        assert_eq!(d.emissive(), [0.0; 3]);
        let e = d.with_emissive([4.0, 0.5, 0.25]);
        assert_eq!(e.emissive(), [4.0, 0.5, 0.25]);
        assert_eq!(e.color(), d.color());
        assert_ne!(e, d);
        // Same for specular: a plain draw is matte, and adding a highlight
        // strength disturbs nothing else.
        assert_eq!(d.specular().get(), 0.0);
        let s = d.with_specular(axiom_kernel::Ratio::finite_or_zero(0.8));
        assert_eq!(s.specular().get(), 0.8);
        assert_eq!(s.emissive(), d.emissive());
        assert_eq!(s.color(), d.color());
        assert_ne!(s, d);
        assert!(d.casts_contact_shadow());
        assert_ne!(
            d,
            FrameDrawItem::new(8, 11, 13, mat(9.0), mat(5.0), [0.1, 0.2, 0.3, 1.0], true)
        );
        assert_eq!(d.surface_program(), 0, "a plain draw takes the built-in path");
        assert_ne!(
            d,
            FrameDrawItem::new(7, 11, 13, mat(9.0), mat(5.0), [0.1, 0.2, 0.3, 1.0], false)
        );
        assert!(
            !FrameDrawItem::new(7, 11, 13, mat(9.0), mat(5.0), [0.1, 0.2, 0.3, 1.0], false)
                .casts_contact_shadow()
        );
        assert!(format!("{d:?}").contains("FrameDrawItem"));
    }

    /// **Seam 4 of 4 — the presentation boundary.** `0` is the built-in fixed
    /// material path, so every draw the engine has ever produced keeps taking
    /// it; naming a program changes only that one lane and only that draw's
    /// equality.
    #[test]
    fn draw_item_defaults_to_the_builtin_surface_program_and_carries_an_authored_one() {
        let plain = FrameDrawItem::new(7, 11, 13, mat(9.0), mat(5.0), [1.0; 4], false);
        assert_eq!(plain.surface_program(), 0);

        let authored = plain.with_surface_program(0xABCD_1234_5678_9001);
        assert_eq!(authored.surface_program(), 0xABCD_1234_5678_9001);
        // Nothing else moved.
        assert_eq!(authored.object_id(), plain.object_id());
        assert_eq!(authored.mesh_id(), plain.mesh_id());
        assert_eq!(authored.material_id(), plain.material_id());
        assert_eq!(authored.world(), plain.world());
        assert_eq!(authored.mvp(), plain.mvp());
        assert_eq!(authored.color(), plain.color());
        assert_eq!(authored.emissive(), plain.emissive());
        assert_eq!(authored.specular(), plain.specular());
        assert_eq!(
            authored.casts_contact_shadow(),
            plain.casts_contact_shadow()
        );
        // ...but the program is part of identity, so two draws that differ only
        // in their program are different draws.
        assert_ne!(authored, plain);
        assert_eq!(plain.with_surface_program(0), plain);
    }

    #[test]
    fn feature_set_accessors_round_trip() {
        let f = FrameFeatureSet::new(true, false, 2, 3);
        assert!(f.uses_textures());
        assert!(!f.uses_shadows());
        assert_eq!(f.directional_lights(), 2);
        assert_eq!(f.point_lights(), 3);
        assert_eq!(f, FrameFeatureSet::new(true, false, 2, 3));
        assert_ne!(f, FrameFeatureSet::new(false, false, 2, 3));
        assert!(format!("{f:?}").contains("FrameFeatureSet"));
    }

    fn sample_packet() -> FramePacket {
        FramePacket::new(
            4,
            240,
            FrameViewport::new(800, 600),
            [0.1, 0.2, 0.3, 1.0],
            Some(FrameCamera::new(mat(1.0), mat(2.0), mat(3.0))),
            vec![FrameDrawItem::new(
                7,
                11,
                13,
                mat(9.0),
                mat(5.0),
                [0.4, 0.5, 0.6, 1.0],
                false,
            )],
            vec![FrameLight::new(0, [0.0, -1.0, 0.0], [1.0, 1.0, 1.0, 1.0])],
            mat(7.0),
            FrameFeatureSet::new(false, true, 1, 0),
        )
    }

    #[test]
    fn packet_accessors_round_trip() {
        let p = sample_packet();
        assert_eq!(p.frame_index(), 4);
        assert_eq!(p.tick(), 240);
        assert_eq!(p.viewport(), FrameViewport::new(800, 600));
        assert_eq!(p.clear_color(), [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(
            p.camera(),
            Some(FrameCamera::new(mat(1.0), mat(2.0), mat(3.0)))
        );
        assert_eq!(p.draws().len(), 1);
        assert_eq!(p.draws()[0].object_id(), 7);
        assert_eq!(p.lights().len(), 1);
        assert_eq!(p.lights()[0].kind(), 0);
        assert_eq!(p.light_view_proj(), mat(7.0));
        assert_eq!(p.features(), FrameFeatureSet::new(false, true, 1, 0));
        assert!(p.sdf().is_none());
        assert!(format!("{p:?}").contains("FramePacket"));
    }

    /// A frame that supplies no time carries exactly zero, and one that supplies
    /// a time carries that one — the lane a time-varying authored surface reads,
    /// and the reason wind replays identically.
    #[test]
    fn a_packet_carries_the_presentation_time_it_was_supplied_and_zero_otherwise() {
        let quiet = sample_packet();
        assert_eq!(quiet.time(), Seconds::finite_or_zero(0.0));
        let timed = sample_packet().with_time(Seconds::finite_or_zero(2.75));
        assert_eq!(timed.time(), Seconds::finite_or_zero(2.75));
        // The time is part of the packet's identity: two frames of the same
        // draws at different times are different frames.
        assert_ne!(timed, quiet);
        assert_eq!(timed, sample_packet().with_time(Seconds::finite_or_zero(2.75)));
    }

    /// A backend without the Specular capability decides whether it has a drop
    /// to declare by asking the packet — so the answer has to follow the draws.
    #[test]
    fn uses_specular_follows_the_draws() {
        let matte = sample_packet();
        assert!(!matte.uses_specular(), "every draw is matte");

        let shiny = FramePacket::new(
            4,
            240,
            FrameViewport::new(800, 600),
            [0.0; 4],
            None,
            vec![
                FrameDrawItem::new(7, 11, 13, mat(9.0), mat(5.0), [1.0; 4], false),
                FrameDrawItem::new(8, 11, 13, mat(9.0), mat(5.0), [1.0; 4], false)
                    .with_specular(axiom_kernel::Ratio::finite_or_zero(0.4)),
            ],
            Vec::new(),
            mat(7.0),
            FrameFeatureSet::new(false, false, 0, 0),
        );
        assert!(shiny.uses_specular(), "one shiny draw among matte ones counts");

        // An empty frame asks for nothing, so there is nothing to report.
        let empty = FramePacket::new(
            0,
            0,
            FrameViewport::new(1, 1),
            [0.0; 4],
            None,
            Vec::new(),
            Vec::new(),
            mat(0.0),
            FrameFeatureSet::new(false, false, 0, 0),
        );
        assert!(!empty.uses_specular());
    }

    #[test]
    fn with_sdf_attaches_a_scene_and_breaks_equality() {
        let scene = SdfScene::new(
            vec![SdfPrimitive::new(
                SdfPrimitive::SPHERE,
                mat(1.0),
                [0.5, 0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
            )],
            mat(2.0),
            mat(3.0),
            [0.0, 0.0, 5.0],
            [100.0, 0.001, 0.0, 0.0],
        );
        let base = sample_packet();
        let with = base.clone().with_sdf(scene.clone());
        assert_eq!(with.sdf(), Some(&scene));
        assert!(base.sdf().is_none());
        assert_ne!(with, base);
    }

    #[test]
    fn with_ambient_attaches_and_breaks_equality() {
        let base = sample_packet();
        assert!(base.ambient().is_none());
        let amb = crate::frame_ambient::FrameAmbient::new([0.6, 0.7, 0.8], [0.2, 0.15, 0.1]);
        let with = base.clone().with_ambient(amb);
        assert_eq!(with.ambient(), Some(&amb));
        assert_ne!(with, base);
    }

    #[test]
    fn with_depth_fog_attaches_and_breaks_equality() {
        let base = sample_packet();
        assert!(base.depth_fog().is_none());
        let fog = crate::frame_depth_fog::FrameDepthFog::new(
            axiom_kernel::Ratio::finite_or_zero(0.97),
            axiom_kernel::Ratio::finite_or_zero(1.0),
            axiom_kernel::Ratio::finite_or_zero(0.9),
            [0.02, 0.03, 0.07],
        );
        let with = base.clone().with_depth_fog(fog);
        assert_eq!(with.depth_fog(), Some(&fog));
        assert_ne!(with, base);
    }

    #[test]
    fn with_postprocess_attaches_and_breaks_equality() {
        let base = sample_packet();
        assert!(base.postprocess().is_none());
        let pp = crate::frame_postprocess::FramePostProcess::cinematic();
        let with = base.clone().with_postprocess(pp);
        assert_eq!(with.postprocess(), Some(&pp));
        assert_ne!(with, base);
    }

    #[test]
    fn with_retro_32bit_profile_attaches_and_breaks_equality() {
        let base = sample_packet();
        assert!(base.retro_32bit().is_none());
        let retro_32bit = crate::frame_retro_32bit::FrameRetro32BitProfile::retro_32bit();
        let with = base.clone().with_retro_32bit_profile(retro_32bit);
        assert_eq!(with.retro_32bit(), Some(&retro_32bit));
        assert_ne!(with, base);
    }

    #[test]
    fn packet_clone_is_equal_and_field_changes_break_equality() {
        let p = sample_packet();
        assert_eq!(p.clone(), p);
        let mut other = sample_packet();
        other = FramePacket::new(
            5, // changed frame index
            other.tick(),
            other.viewport(),
            other.clear_color(),
            other.camera(),
            other.draws().to_vec(),
            other.lights().to_vec(),
            other.light_view_proj(),
            other.features(),
        );
        assert_ne!(other, p);
    }

    #[test]
    fn packet_with_no_camera_reports_none() {
        let p = FramePacket::new(
            0,
            0,
            FrameViewport::new(1, 1),
            [0.0; 4],
            None,
            Vec::new(),
            Vec::new(),
            mat(0.0),
            FrameFeatureSet::new(false, false, 0, 0),
        );
        assert!(p.camera().is_none());
        assert!(p.draws().is_empty());
        assert!(p.lights().is_empty());
    }
}
