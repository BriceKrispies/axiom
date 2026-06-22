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
/// model-view-projection matrices (column-major, 16 floats each), and its linear
/// RGBA colour. Objects appear in the packet in deterministic command-list draw
/// order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameDrawItem {
    object_id: u64,
    mesh_id: u64,
    material_id: u64,
    world: [f32; 16],
    mvp: [f32; 16],
    color: [f32; 4],
}

impl FrameDrawItem {
    /// A draw item with its stable `object_id`, `mesh_id`, `material_id`,
    /// column-major `world` and `mvp` matrices, and linear RGBA `color`.
    pub const fn new(
        object_id: u64,
        mesh_id: u64,
        material_id: u64,
        world: [f32; 16],
        mvp: [f32; 16],
        color: [f32; 4],
    ) -> Self {
        FrameDrawItem {
            object_id,
            mesh_id,
            material_id,
            world,
            mvp,
            color,
        }
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

    /// The linear RGBA colour.
    pub const fn color(&self) -> [f32; 4] {
        self.color
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
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(seed: f32) -> [f32; 16] {
        [seed; 16]
    }

    #[test]
    fn viewport_accessors_round_trip() {
        let v = FrameViewport::new(800, 600);
        assert_eq!(v.width(), 800);
        assert_eq!(v.height(), 600);
        // Debug + Clone + Eq are derived and must be exercised.
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
        // Colour in [0..3], intensity in [3].
        assert_eq!(l.color_intensity(), [1.0, 0.0, 0.0, 2.5]);
        assert_ne!(l, FrameLight::new(0, [2.0, 3.0, -4.0], [1.0, 0.0, 0.0, 2.5]));
        assert!(format!("{l:?}").contains("FrameLight"));
    }

    #[test]
    fn draw_item_accessors_round_trip() {
        let d = FrameDrawItem::new(7, 11, 13, mat(9.0), mat(5.0), [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(d.object_id(), 7);
        assert_eq!(d.mesh_id(), 11);
        assert_eq!(d.material_id(), 13);
        assert_eq!(d.world(), mat(9.0));
        assert_eq!(d.mvp(), mat(5.0));
        assert_eq!(d.color(), [0.1, 0.2, 0.3, 1.0]);
        assert_ne!(
            d,
            FrameDrawItem::new(8, 11, 13, mat(9.0), mat(5.0), [0.1, 0.2, 0.3, 1.0])
        );
        assert!(format!("{d:?}").contains("FrameDrawItem"));
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
        assert_eq!(p.camera(), Some(FrameCamera::new(mat(1.0), mat(2.0), mat(3.0))));
        assert_eq!(p.draws().len(), 1);
        assert_eq!(p.draws()[0].object_id(), 7);
        assert_eq!(p.lights().len(), 1);
        assert_eq!(p.lights()[0].kind(), 0);
        assert_eq!(p.light_view_proj(), mat(7.0));
        assert_eq!(p.features(), FrameFeatureSet::new(false, true, 1, 0));
        assert!(format!("{p:?}").contains("FramePacket"));
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
