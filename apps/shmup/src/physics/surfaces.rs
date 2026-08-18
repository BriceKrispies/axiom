//! Ported from Claude-of-Duty `src/physics/surfaces.js:1-143` — the whole file.
//!
//! The twelve-entry surface taxonomy itself — `SURFACE_NAMES` / `SURFACE` —
//! is **not** re-declared here. `apps/shmup/src/world/palette.rs`
//! already carries a `Surface` enum whose declaration order matches the
//! source's `SURFACE_NAMES` exactly (`Surface::index`/`Surface::from_index`
//! prove the round trip), so this module reuses it via `use
//! crate::world::palette::Surface` rather than defining a second tag type.
//! Everything this file *adds* on top — the per-surface physical response
//! table, name-based inference, and the collision layer/mask constants — is
//! ported below.

use crate::world::palette::Surface;

/// Per-surface physical response (`surfaces.js:44-69`, `SURFACE_PROPS`).
///
/// - `pen_depth` — metres of material a reference round (power 1.0) fully
///   defeats.
/// - `energy_loss` — fraction of remaining damage lost per `pen_depth`
///   traversed.
/// - `deflect` — radians of random yaw/pitch scatter per `pen_depth`
///   traversed.
/// - `friction` — dry kinetic coefficient (rigid bodies, ragdolls, footing).
/// - `restitution` — bounce factor for debris.
/// - `density` — kg/m^3, used for the impulse a body of unknown mass
///   receives.
/// - `hardness` — 0..1, spark/chip likelihood, drives fx choice.
/// - `shatters` — the surface breaks rather than absorbs (glass).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceProps {
    pub pen_depth: f64,
    pub energy_loss: f64,
    pub deflect: f64,
    pub friction: f64,
    pub restitution: f64,
    pub density: f64,
    pub hardness: f64,
    pub shatters: bool,
}

/// `SURFACE_PROPS`, indexed by [`Surface::index`] (`surfaces.js:44-69`).
pub const SURFACE_PROPS: [SurfaceProps; 12] = [
    // concrete
    SurfaceProps {
        pen_depth: 0.055,
        energy_loss: 0.62,
        deflect: 0.055,
        friction: 0.92,
        restitution: 0.26,
        density: 2400.0,
        hardness: 0.95,
        shatters: false,
    },
    // metal (structural steel / vehicle panel)
    SurfaceProps {
        pen_depth: 0.022,
        energy_loss: 0.7,
        deflect: 0.075,
        friction: 0.52,
        restitution: 0.44,
        density: 7800.0,
        hardness: 1.0,
        shatters: false,
    },
    // wood
    SurfaceProps {
        pen_depth: 0.32,
        energy_loss: 0.3,
        deflect: 0.03,
        friction: 0.72,
        restitution: 0.3,
        density: 620.0,
        hardness: 0.4,
        shatters: false,
    },
    // dirt
    SurfaceProps {
        pen_depth: 0.26,
        energy_loss: 0.45,
        deflect: 0.05,
        friction: 0.96,
        restitution: 0.09,
        density: 1500.0,
        hardness: 0.2,
        shatters: false,
    },
    // sand
    SurfaceProps {
        pen_depth: 0.19,
        energy_loss: 0.55,
        deflect: 0.06,
        friction: 1.05,
        restitution: 0.04,
        density: 1600.0,
        hardness: 0.12,
        shatters: false,
    },
    // glass
    SurfaceProps {
        pen_depth: 0.45,
        energy_loss: 0.12,
        deflect: 0.012,
        friction: 0.32,
        restitution: 0.2,
        density: 2500.0,
        hardness: 0.85,
        shatters: true,
    },
    // water
    SurfaceProps {
        pen_depth: 1.1,
        energy_loss: 0.5,
        deflect: 0.09,
        friction: 0.3,
        restitution: 0.0,
        density: 1000.0,
        hardness: 0.0,
        shatters: false,
    },
    // foliage
    SurfaceProps {
        pen_depth: 3.0,
        energy_loss: 0.05,
        deflect: 0.008,
        friction: 0.62,
        restitution: 0.06,
        density: 300.0,
        hardness: 0.05,
        shatters: false,
    },
    // fabric
    SurfaceProps {
        pen_depth: 2.2,
        energy_loss: 0.06,
        deflect: 0.01,
        friction: 0.8,
        restitution: 0.05,
        density: 400.0,
        hardness: 0.02,
        shatters: false,
    },
    // flesh
    SurfaceProps {
        pen_depth: 0.55,
        energy_loss: 0.35,
        deflect: 0.02,
        friction: 0.9,
        restitution: 0.05,
        density: 1050.0,
        hardness: 0.05,
        shatters: false,
    },
    // rubber
    SurfaceProps {
        pen_depth: 0.28,
        energy_loss: 0.4,
        deflect: 0.04,
        friction: 1.25,
        restitution: 0.72,
        density: 1200.0,
        hardness: 0.1,
        shatters: false,
    },
    // plaster / drywall
    SurfaceProps {
        pen_depth: 0.7,
        energy_loss: 0.12,
        deflect: 0.02,
        friction: 0.86,
        restitution: 0.14,
        density: 800.0,
        hardness: 0.25,
        shatters: false,
    },
];

impl Surface {
    /// `surfaces.js:44-69` looked up by this surface's index.
    pub const fn props(self) -> SurfaceProps {
        SURFACE_PROPS[self.index() as usize]
    }
}

/// The source's `surfaceIndex(s, fallback = SURFACE.concrete)`
/// (`surfaces.js:72-80`), specialised to the string-name overload — the
/// numeric-index overload is the identity and Rust already has `Surface` as a
/// typed value, so callers holding an index reach for [`Surface::from_index`]
/// directly rather than round-tripping through this function.
///
/// Falls back to [`guess_surface`] when `s` does not name a `Surface`
/// directly, exactly as the source does.
pub fn surface_index(s: &str, fallback: Surface) -> Surface {
    surface_by_name(s).unwrap_or_else(|| guess_surface(s, fallback))
}

fn surface_by_name(s: &str) -> Option<Surface> {
    Surface::ALL.into_iter().find(|c| c.name() == s)
}

/// `(pattern keywords, surface)` pairs. `surfaces.js:82-95`'s `GUESS` table is
/// a list of case-insensitive regexes, each a plain `word|word|word`
/// alternation with no other regex metacharacters (no anchors, wildcards, or
/// character classes) — so a case-insensitive substring test against each
/// keyword is exactly equivalent to the source's regex test, without pulling
/// in a regex engine for this one table.
const GUESS: &[(&[&str], Surface)] = &[
    (
        &[
            "concrete", "cement", "stone", "brick", "rock", "asphalt", "tarmac", "road", "kerb",
            "curb", "marble", "tile",
        ],
        Surface::Concrete,
    ),
    (
        &[
            "metal", "steel", "iron", "alu", "aluminium", "aluminum", "tin", "pipe", "rail",
            "grate", "vent", "car", "vehicle", "chassis", "barrel", "drum", "sign",
        ],
        Surface::Metal,
    ),
    (
        &[
            "wood", "timber", "plank", "crate", "pallet", "door", "plywood", "fence", "log",
            "furnit",
        ],
        Surface::Wood,
    ),
    (
        &["dirt", "mud", "soil", "earth", "ground", "terrain", "gravel", "rubble"],
        Surface::Dirt,
    ),
    (&["sand", "dune", "beach"], Surface::Sand),
    (&["glass", "window", "mirror", "screen", "pane"], Surface::Glass),
    (&["water", "pool", "puddle", "liquid"], Surface::Water),
    (
        &["foliage", "leaf", "leaves", "bush", "tree", "grass", "plant", "hedge", "shrub"],
        Surface::Foliage,
    ),
    (
        &[
            "fabric", "cloth", "canvas", "tarp", "curtain", "carpet", "rug", "sofa", "awning",
        ],
        Surface::Fabric,
    ),
    (
        &[
            "flesh", "body", "skin", "head", "torso", "limb", "enemy", "actor", "char",
        ],
        Surface::Flesh,
    ),
    (&["rubber", "tyre", "tire", "hose", "mat"], Surface::Rubber),
    (
        &[
            "plaster", "drywall", "gypsum", "stucco", "wall", "ceiling", "partition",
        ],
        Surface::Plaster,
    ),
];

/// Best-effort surface inference from a mesh/material name.
/// `surfaces.js:97-102`.
pub fn guess_surface(name: &str, fallback: Surface) -> Surface {
    if name.is_empty() {
        return fallback;
    }
    let lower = name.to_lowercase();
    GUESS
        .iter()
        .find(|(keywords, _)| keywords.iter().any(|k| lower.contains(k)))
        .map_or(fallback, |&(_, surface)| surface)
}

/* ------------------------------------------------------------------ */
/* Collision layers                                                    */
/* ------------------------------------------------------------------ */

/// Collision layer bits. `surfaces.js:112-125`, `LAYER`. `u16` because the
/// BVH packs a per-triangle mask into a `Uint16Array`
/// (`bvh.js:52`/`this.mask`).
pub mod layer {
    /// Immovable level geometry.
    pub const STATIC: u16 = 1 << 0;
    /// Static props — crates, cars. Same BVH, separate bit so AI can ignore.
    pub const PROP: u16 = 1 << 1;
    /// Simulated debris & dropped weapons.
    pub const DEBRIS: u16 = 1 << 2;
    /// Player capsule.
    pub const PLAYER: u16 = 1 << 3;
    /// AI character capsules / hitboxes.
    pub const ACTOR: u16 = 1 << 4;
    /// Ragdoll bones.
    pub const RAGDOLL: u16 = 1 << 5;
    /// Breakable glass — blocks bullets briefly, never blocks sight.
    pub const GLASS: u16 = 1 << 6;
    /// Water volumes.
    pub const WATER: u16 = 1 << 7;
    /// Invisible clip: stops characters, ignored by bullets and cameras.
    pub const CLIP: u16 = 1 << 8;
    /// Blocks bullets but not movement (grates, railings modelled thin).
    pub const SHOOT_ONLY: u16 = 1 << 9;
    /// Non-colliding trigger volume.
    pub const TRIGGER: u16 = 1 << 10;
    /// Foliage — no collision, deflects bullets barely, blocks nothing.
    pub const FOLIAGE: u16 = 1 << 11;
}

/// Precomposed query masks. `surfaces.js:127-143`, `MASK`.
pub mod mask {
    use super::layer;

    pub const ALL: u16 = 0xffff & !layer::TRIGGER;
    /// Everything a character capsule collides with.
    pub const CHARACTER: u16 = layer::STATIC | layer::PROP | layer::CLIP;
    /// Everything a bullet can strike.
    pub const BULLET: u16 = layer::STATIC
        | layer::PROP
        | layer::DEBRIS
        | layer::ACTOR
        | layer::RAGDOLL
        | layer::GLASS
        | layer::SHOOT_ONLY
        | layer::FOLIAGE;
    /// Static-only: camera collision, cover queries, decal projection.
    pub const WORLD: u16 = layer::STATIC | layer::PROP;
    /// Line of sight — glass and foliage do not block vision.
    pub const SIGHT: u16 = layer::STATIC | layer::PROP | layer::DEBRIS;
    /// What rigid debris bounces off.
    pub const DEBRIS: u16 = layer::STATIC | layer::PROP | layer::CLIP;
    /// Explosion occlusion.
    pub const EXPLOSION: u16 = layer::STATIC | layer::PROP;
}
