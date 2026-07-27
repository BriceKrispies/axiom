//! The field paint **rules**: one configuration structure holding every value
//! the paint system uses, the paint categories (one material, one batch each),
//! and the camera-relative level-of-detail classifier.
//!
//! # Why the field is painted this way
//!
//! A real field carries ~100 full-width yard lines, ~400 one-yard ticks and a
//! set of block numbers. At the presentation this app actually renders at — a
//! low-resolution, pixel-stylized raster (the Canvas2D software rasterizer runs
//! a 240×135 framebuffer) — almost all of that paint is *thinner than one
//! pixel*. Sub-pixel geometry cannot be drawn stably: it flickers as coverage
//! flips on and off while the camera moves, and it costs a projection and a
//! shade per triangle to produce that flicker. The old field spent ~2 000
//! triangles of thin quads doing exactly that.
//!
//! So the field is painted from three things instead:
//!
//! * **Broad alternating five-yard turf bands** carry the sense of distance.
//!   They are large filled polygons, so they are perfectly stable under camera
//!   rotation and cost 2 triangles each (see [`super::generator`]).
//! * **A small, fixed set of retained markings**, every one of which is a real
//!   world-space rectangle wide enough to survive projection — never a line,
//!   never a stroke.
//! * **Camera-relative level of detail**, so the compact markings (hash blocks,
//!   ten-yard divisions) exist only where they are still several pixels across.
//!
//! Everything numeric lives in [`PAINT`] and [`PALETTE`]; the emission pass is
//! [`super::paint_layout`].

use axiom::prelude::Vec3;

/// Paint floats this far above the turf, yards. Paired with the camera's
/// pulled-out near plane (see [`crate::scene_sync`]) this keeps far paint
/// steady instead of z-fighting the turf it sits on.
pub const PAINT_Y: f32 = 0.03;

/// Every value the field paint system uses. Nothing about band spacing,
/// marking width, or level of detail is written anywhere else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintConfig {
    /// Turf band length along `Z`, yards. The playable field is divided into
    /// bands of this size, alternating between two close turf values.
    pub band_yards: f32,
    /// Spacing of the retained major divisions, yards.
    pub major_yards: f32,
    /// Spacing of hash blocks along `Z`, yards.
    pub hash_spacing_yards: f32,
    /// Full width of a sideline / goal line / end line, yards.
    pub boundary_width: f32,
    /// Full width of a major division, yards.
    pub major_width: f32,
    /// Full width of the line of scrimmage and the line to gain, yards.
    pub gameplay_width: f32,
    /// Hash block extent along `Z` (its thickness), yards.
    pub hash_width: f32,
    /// Hash block extent along `X` (its length), yards. Wider than it is thick
    /// so a hash reads as a squat paint block, not a dash.
    pub hash_length: f32,
    /// Distance from the camera within which a marking is [`Lod::Near`], yards.
    pub near_yards: f32,
    /// Distance within which a marking is [`Lod::Mid`], yards.
    pub mid_yards: f32,
    /// Distance past which a marking is [`Lod::Culled`], yards. Chosen so a
    /// marking is dropped while it is still several pixels across, never once
    /// it has decayed into an unstable fragment.
    pub cull_yards: f32,
    /// Camera-forward depth a marking must clear to be considered at all,
    /// yards. Anything nearer is at or behind the near plane and is culled
    /// before any projection work happens.
    pub min_depth_yards: f32,
    /// How far above [`PAINT_Y`] the two gameplay lines sit, yards. They are
    /// the only paint that can cross other paint, and this keeps the crossing
    /// resolved by depth instead of by z-fighting.
    pub gameplay_lift: f32,
}

/// The one field paint configuration.
pub const PAINT: PaintConfig = PaintConfig {
    band_yards: 5.0,
    major_yards: 10.0,
    hash_spacing_yards: 1.0,
    boundary_width: 0.42,
    major_width: 0.30,
    gameplay_width: 0.36,
    hash_width: 0.30,
    hash_length: 0.90,
    near_yards: 16.0,
    mid_yards: 34.0,
    cull_yards: 62.0,
    min_depth_yards: 0.6,
    gameplay_lift: 0.02,
};

/// Every colour the field surface and its paint use, linear RGB.
///
/// The two turf values are deliberately *close*: the bands must communicate
/// distance without reading as stripes. The paint runs muted off-white for
/// ordinary markings, a step brighter for the boundary that establishes field
/// identity, and two named gameplay hues that must never be confused with
/// either.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintPalette {
    /// The lighter of the two alternating turf bands.
    pub band_light: [f32; 3],
    /// The darker of the two alternating turf bands.
    pub band_dark: [f32; 3],
    /// The surround outside the field proper.
    pub apron: [f32; 3],
    /// Sidelines, goal lines, end lines — the brightest paint.
    pub boundary: [f32; 3],
    /// Ordinary paint at the near fade tier.
    pub paint_near: [f32; 3],
    /// Ordinary paint at the mid fade tier.
    pub paint_mid: [f32; 3],
    /// The line of scrimmage.
    pub scrimmage: [f32; 3],
    /// The line to gain.
    pub line_to_gain: [f32; 3],
}

/// The one field palette.
pub const PALETTE: PaintPalette = PaintPalette {
    band_light: [0.066, 0.330, 0.192],
    band_dark: [0.055, 0.288, 0.166],
    apron: [0.034, 0.166, 0.104],
    boundary: [0.880, 0.900, 0.870],
    paint_near: [0.740, 0.762, 0.730],
    paint_mid: [0.520, 0.548, 0.518],
    scrimmage: [0.400, 0.548, 0.780],
    line_to_gain: [0.860, 0.740, 0.320],
};

/// How many paint categories exist.
pub const PAINT_CATEGORY_COUNT: usize = 6;

/// Which batch a paint quad belongs to.
///
/// A category is the unit of batching: one material, one contiguous run of
/// pool slots, one group of draws. The two `Major*` variants are the same
/// marking at two deterministic fade tiers — the engine's Lambert pipeline has
/// no alpha blending, so a fade tier is a fixed *brightness* step, which is the
/// honest equivalent and is stable frame to frame in a way a per-marking alpha
/// ramp would not be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintCategory {
    /// Sidelines, goal lines, end lines.
    Boundary,
    /// A ten-yard division inside the near tier.
    MajorNear,
    /// A ten-yard division inside the mid tier, one fade step dimmer.
    MajorMid,
    /// A paired hash block. Near tier only.
    Hash,
    /// The active line of scrimmage.
    Scrimmage,
    /// The active line to gain.
    LineToGain,
}

impl PaintCategory {
    /// Every category, in pool order.
    pub const ALL: [PaintCategory; PAINT_CATEGORY_COUNT] = [
        PaintCategory::Boundary,
        PaintCategory::MajorNear,
        PaintCategory::MajorMid,
        PaintCategory::Hash,
        PaintCategory::Scrimmage,
        PaintCategory::LineToGain,
    ];

    /// This category's index into a per-category table.
    pub fn index(self) -> usize {
        match self {
            PaintCategory::Boundary => 0,
            PaintCategory::MajorNear => 1,
            PaintCategory::MajorMid => 2,
            PaintCategory::Hash => 3,
            PaintCategory::Scrimmage => 4,
            PaintCategory::LineToGain => 5,
        }
    }

    /// The hard bound on how many quads of this category may exist in a frame.
    /// The scene pool is built to exactly these sizes, so emission can never
    /// allocate and can never grow the scene.
    pub fn pool_size(self) -> usize {
        match self {
            // Two sidelines, two goal lines, two end lines.
            PaintCategory::Boundary => 6,
            // Nine interior ten-yard divisions, with headroom for both tiers.
            PaintCategory::MajorNear => 12,
            PaintCategory::MajorMid => 12,
            // The near window is `2 * near_yards` yards of paired blocks.
            PaintCategory::Hash => 72,
            PaintCategory::Scrimmage => 1,
            PaintCategory::LineToGain => 1,
        }
    }

    /// This category's linear RGB from `palette`.
    pub fn color(self, palette: &PaintPalette) -> [f32; 3] {
        match self {
            PaintCategory::Boundary => palette.boundary,
            PaintCategory::MajorNear | PaintCategory::Hash => palette.paint_near,
            PaintCategory::MajorMid => palette.paint_mid,
            PaintCategory::Scrimmage => palette.scrimmage,
            PaintCategory::LineToGain => palette.line_to_gain,
        }
    }
}

/// The camera-relative detail tier a point falls in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    /// Close enough for hash blocks and full-brightness paint.
    Near,
    /// Ten-yard divisions only, one fade step dimmer.
    Mid,
    /// Turf bands, sidelines and gameplay lines only — no compact markings.
    Far,
    /// Behind the near plane, beyond the cull distance, or not a finite point.
    Culled,
}

/// The camera reduced to what level of detail needs: an eye and a unit forward.
///
/// Constructing one is the single place a degenerate or non-finite camera is
/// rejected. Everything downstream works from a validated `PaintCamera`, so no
/// NaN or infinity can reach a marking position — and therefore none can reach
/// a projected coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintCamera {
    pub eye: Vec3,
    pub forward: Vec3,
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

impl PaintCamera {
    /// The paint camera for an eye looking at `target`, or `None` when either
    /// point is non-finite or the two coincide (no forward direction exists).
    pub fn looking(eye: Vec3, target: Vec3) -> Option<PaintCamera> {
        let to = target.subtract(eye);
        let length = to.length();
        let usable = finite(eye) && finite(target) && length.is_finite() && length > 1.0e-4;
        usable.then(|| PaintCamera {
            eye,
            forward: to.mul_scalar(1.0 / length),
        })
    }

    /// How far `point` sits along the camera's forward axis, yards. Negative
    /// behind the camera.
    pub fn depth(&self, point: Vec3) -> f32 {
        let to = point.subtract(self.eye);
        to.x * self.forward.x + to.y * self.forward.y + to.z * self.forward.z
    }
}

/// The detail tier `point` falls in for this camera.
///
/// Depth is tested first and is what makes aggressive yaw safe: a marking at or
/// behind the near plane is culled here, before any quad is built and long
/// before the renderer would have to clip it.
pub fn classify(camera: &PaintCamera, point: Vec3, config: &PaintConfig) -> Lod {
    let offset = point.subtract(camera.eye);
    let range = offset.length();
    let depth = camera.depth(point);
    let valid = finite(point) && range.is_finite();
    let visible = valid && depth >= config.min_depth_yards && range <= config.cull_yards;
    let tier = [Lod::Far, Lod::Mid, Lod::Near]
        [usize::from(range <= config.mid_yards) + usize::from(range <= config.near_yards)];
    [Lod::Culled, tier][usize::from(visible)]
}
