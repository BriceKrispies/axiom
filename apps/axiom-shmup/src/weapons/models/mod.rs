//! Ported from Claude-of-Duty `src/weapons/models/*.js` — the three weapon
//! assemblies (`rifle.js`, `smg.js`, `pistol.js`). Each `build_*` function
//! lays the 27 part builders from `weapons::parts` out against a documented
//! dimension sheet into one [`Assembly`][crate::weapons::geometry::Assembly],
//! exactly as its source file does.
//!
//! See `docs/work-manifests/shmup-port/03-weapon-geometry-api.md`
//! for the fixed Rust primitive/`Assembly` API these are written against, and
//! `parts.rs`'s sibling modules for the part builders themselves.
//!
//! This is app code (`apps/`), outside the Branchless Law and the Coverage
//! Law — plain `if`/`for` throughout, matching the source's own control
//! flow, per the port recipe.
//!
//! ## Shared node types
//!
//! Every source model returns a `nodes` object of named attachment points
//! the (not-yet-ported) animation rig reads. The three shapes below are the
//! recurring ones across all three weapons; each `build_*` function's own
//! `*Nodes` struct (in its own file) composes them plus whatever is specific
//! to that weapon (the rifle/smg's `chargeRest`/`boltRest`/`selectorPivot`
//! vs. the pistol's `slideRest`/`slideGeom`).
//!
//! Every field is `f32`, matching [`Assembly::node`][crate::weapons::geometry::assembly]'s
//! own `Node { pos: [f32; 3], rot: [f32; 3] }` shape and the rest of a
//! model's authoring math (`bore`, `railTop`, … are all `f32` throughout
//! `weapons::parts`) — unlike `weapons::clips::AttachNodes`'s `f64` fields,
//! which serve a different, not-yet-wired-up consumer (see that module's own
//! doc: it is deliberately "the subset... rather than a placeholder for the
//! whole rig").

/// A weapon-space position + Euler rotation attachment point — `{ pos:
/// [x,y,z], rot: [rx,ry,rz] }` in the source (`nodes.magSeat`,
/// `nodes.chargeRest`, `nodes.boltRest`, `nodes.triggerPivot`,
/// `nodes.selectorPivot`, `nodes.slideRest`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PosRot {
    pub pos: [f32; 3],
    pub rot: [f32; 3],
}

/// A hand attachment target: the WRIST position (see each model's long
/// comment on why — the glove is modelled from the wrist forward, so the
/// target is `knuckle - 0.098 * fingerDir`, never the palm directly) plus the
/// finger and dorsal-back directions the arm-fitting rig solves against
/// (`nodes.gripR`/`nodes.gripL`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GripTarget {
    pub pos: [f32; 3],
    pub finger: [f32; 3],
    pub back: [f32; 3],
}

/// The handguard's collision cylinder, for the build-time fingertip contact
/// solve (`Arm.fitToCylinder`) — `nodes.handguard` (rifle only; the smg and
/// pistol have no free-float handguard for the support hand to wrap).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandguardProfile {
    pub axis: [f32; 3],
    pub dir: [f32; 3],
    pub r: f32,
    pub z0: f32,
    pub z1: f32,
}

/// `shell: { caseLen, rimR }` — the cartridge dimensions the ejected-brass
/// system reads, common to all three models' return values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellDims {
    pub case_len: f32,
    pub rim_r: f32,
}

pub mod pistol;
pub mod rifle;
pub mod smg;
