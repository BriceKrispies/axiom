//! **FX** — GPU particles, impacts, decals, muzzle flash, tracers, shells,
//! explosions, refraction and ambience.
//!
//! Ported from Claude-of-Duty `src/fx/` — **every file**, except
//! `preview.js`/`preview.html`/`shoot.mjs` (the source's own
//! dev harness, outside the game) and `noise.js`'s consumer split with
//! [`crate::materials::noise`] (a different noise implementation entirely —
//! see [`noise`]'s module doc).
//!
//! | this module     | source            |
//! |------------------|--------------------|
//! | [`noise`]        | `fx/noise.js`      |
//! | [`util`]          | `fx/util.js`       |
//! | [`particles`]     | `fx/particles.js`  |
//! | [`ambience`]     | `fx/ambience.js`   |
//! | [`atlas`]          | `fx/atlas.js`      |
//! | [`decals`]         | `fx/decals.js`     |
//! | [`shells`]          | `fx/shells.js`     |
//! | [`tracers`]          | `fx/tracers.js`    |
//! | [`lights`]            | `fx/lights.js`     |
//! | [`haze`]                | `fx/haze.js`       |
//! | [`explosions`]           | `fx/explosions.js`|
//! | [`muzzle`]                | `fx/muzzle.js`     |
//! | [`impacts`]                 | `fx/impacts.js`    |
//! | [`system`]                    | `fx/index.js`      |
//! | [`world`]                       | — (the physics seam every one of the above needs; see its own doc) |
//!
//! ## Determinism
//!
//! `fx` takes `ctx.rng.fork()` at init (`index.js:40`) — [`system::FxSystem::new`]'s
//! `seed` parameter is that fork's output. Every subsequent draw anywhere in
//! this module tree happens off [`system::FxSystem::rng`] (or a value forked
//! from it, in the exact order and place the source forks: once for the
//! particle atlas, once for the decal atlas, once inside `ShellSystem::new`
//! for the brass texture bake — see [`system::FxSystem::new`]'s doc). Draw
//! order is part of the contract everywhere in this port; every function
//! below that consumes `rng` is commented against its source line range so
//! the order stays diffable by eye.
//!
//! ## What is not ported: the render seam
//!
//! Nothing in this module tree touches a GPU. Every shader source string,
//! every `THREE.*` buffer/material/mesh, and every camera/scene-graph read
//! is a documented seam — see [`particles`], [`atlas`], [`decals`],
//! [`haze`], [`muzzle`] and [`system`]'s module docs for exactly where and
//! why. What *is* ported is everything CPU-testable: emission and
//! integration math, the ring-buffer bookkeeping, the procedural texture
//! bakes, the per-surface impact recipes, and the facade's budget/dispatch
//! logic.

pub mod atlas;
pub mod ambience;
pub mod burst;
pub mod decals;
pub mod explosions;
pub mod haze;
pub mod impacts;
pub mod lights;
pub mod muzzle;
pub mod noise;
pub mod particles;
pub mod recipes;
pub mod shells;
pub mod system;
pub mod tracers;
pub mod util;
pub mod world;
