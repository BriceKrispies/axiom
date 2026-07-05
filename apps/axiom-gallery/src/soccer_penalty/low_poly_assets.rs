//! App-local low-poly primitive vocabulary and color palette for the Stage 1
//! diorama.
//!
//! TEMPORARY APP GLUE. Axiom has no soccer/mesh-part asset module (and no asset
//! *loading* at all in this app's scope), so the smallest deterministic data
//! shapes the diorama needs live here, in the app. These are flat, faceted,
//! retro 32-bit-style primitives described as data — there is no real mesh, texture, or
//! GPU resource behind them yet. A future stage translates these into real
//! `axiom-scene` renderables + `axiom-resources` meshes/materials.

use axiom_math::Vec3;

/// The primitive mesh kinds the Stage 1 diorama is assembled from.
///
/// Every diorama object is one of these chunky, flat-shaded shapes. This keeps
/// the whole scene expressible as fixed constants with no mesh data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveShape {
    /// An axis-aligned box: torsos, heads, limbs, goal posts, the crossbar,
    /// the stadium wall, crowd cards, and ad boards.
    Box,
    /// A low-poly / faceted sphere: the ball. Its radius is `size.x`.
    FacetedBall,
    /// A flat quad lying in a plane: the field, painted field lines, the
    /// penalty spot, blob shadows, and net panels.
    Quad,
    /// A thin line segment: the goal-net grid and goal-area line accents.
    Line,
}

/// A deterministic RGBA color with each channel in `0.0..=1.0`.
///
/// App-local on purpose: color is a rendering/authoring concern of this diorama,
/// not an engine-spine primitive, so it stays in the app.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    /// An opaque color.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// A color with explicit alpha (used by translucent blob shadows).
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

/// The fixed Stage 1 palette: saturated, arcade-y flats chosen to read clearly
/// as a penalty-kick scene. Every value is a compile-time constant so the whole
/// diorama is byte-for-byte reproducible.
pub mod palette {
    use super::Rgba;

    // --- field & markings ---
    // Stronger light/dark contrast so the mown bands read as pronounced stripes
    // (they were near-identical and looked flat/minty), and a warmer light band.
    // Re-saturated toward the reference's vivid turf: the app has no board-wide
    // FramePostProcess grade (the GPU champion never applies one — that seam is an
    // architect job), so the dominant surface's colour punch must come from its
    // albedo. The green channel now dominates harder and blue is pulled down, which
    // enriches the grass and warms it off the pale mint the ungraded raster read as.
    pub const GRASS_LIGHT: Rgba = Rgba::rgb(0.31, 0.60, 0.17);
    pub const GRASS_DARK: Rgba = Rgba::rgb(0.16, 0.44, 0.10);
    pub const LINE_WHITE: Rgba = Rgba::rgb(0.95, 0.97, 0.95);

    // --- goal & net ---
    pub const POST_WHITE: Rgba = Rgba::rgb(0.97, 0.98, 0.99);
    pub const NET_REAR: Rgba = Rgba::new(0.82, 0.86, 0.90, 0.55);
    pub const NET_FRONT: Rgba = Rgba::new(0.90, 0.93, 0.96, 0.70);

    // --- kicker (blue jersey, white shorts, dark socks/boots) ---
    pub const KICKER_JERSEY: Rgba = Rgba::rgb(0.16, 0.30, 0.78);
    pub const KICKER_SHORTS: Rgba = Rgba::rgb(0.93, 0.94, 0.96);
    pub const KICKER_SKIN: Rgba = Rgba::rgb(0.86, 0.66, 0.52);
    pub const KICKER_SOCKS: Rgba = Rgba::rgb(0.10, 0.11, 0.14);

    // --- goalie (yellow jersey, black shorts) ---
    pub const GOALIE_JERSEY: Rgba = Rgba::rgb(0.96, 0.82, 0.14);
    pub const GOALIE_SHORTS: Rgba = Rgba::rgb(0.10, 0.11, 0.13);
    pub const GOALIE_SKIN: Rgba = Rgba::rgb(0.82, 0.62, 0.48);
    pub const GOALIE_GLOVES: Rgba = Rgba::rgb(0.18, 0.62, 0.86);

    // --- ball ---
    pub const BALL_WHITE: Rgba = Rgba::rgb(0.97, 0.97, 0.98);

    // --- backdrop ---
    // The reference has no bright grey wall: the stand behind/above the goal is
    // a dark near-black mass the packed crowd sits against. Darkened to match.
    pub const STADIUM_WALL: Rgba = Rgba::rgb(0.13, 0.14, 0.18);
    pub const CROWD_A: Rgba = Rgba::rgb(0.72, 0.28, 0.30);
    pub const CROWD_B: Rgba = Rgba::rgb(0.28, 0.42, 0.70);
    pub const CROWD_C: Rgba = Rgba::rgb(0.86, 0.72, 0.24);
    pub const AD_BOARD: Rgba = Rgba::rgb(0.09, 0.10, 0.13);
    pub const AD_BOARD_AXIOM: Rgba = Rgba::rgb(0.86, 0.20, 0.42);

    // --- shadows ---
    // A soft contact shadow, not a black cut-out: lower opacity (0.38 -> 0.20 ->
    // 0.14) and a slightly grass-tinted, lifted tone so the ellipse reads as a
    // grounded contact darkening rather than an opaque hole in the pitch. The
    // reference's sunlit turf shows only a very faint soft darkening under the
    // actors, so the blob is pushed fainter still without touching ambient.
    pub const BLOB_SHADOW: Rgba = Rgba::new(0.06, 0.11, 0.07, 0.14);
}

/// A convenience direction constant: the up axis the whole diorama uses. It is
/// also the representative face normal the render plan flat-shades top faces
/// with.
pub const WORLD_UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);
