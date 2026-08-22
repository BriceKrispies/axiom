//! The player: movement, camera feel, springs, tuning.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/player/` — `springs.js`, `tuning.js`,
//! `movement.js`, `camera.js`, `mantle.js`.
//!
//! | this module   | source            |
//! |----------------|-------------------|
//! | [`springs`]    | `springs.js`      |
//! | [`tuning`]     | `tuning.js`       |
//! | [`mantle`]     | `mantle.js`       |
//! | [`movement`]   | `movement.js`     |
//! | [`camera`]     | `camera.js`       |
//!
//! ## The physics seam
//!
//! The source delegates every collision query to `physics` — a
//! `createCharacter()`-shaped controller plus a duck-typed `raycast`/
//! `capsuleCast`/`checkCapsule` world. Neither exists in this crate yet
//! (`src/physics/` is a concurrent, separate port). Rather than invent a stand-in
//! physics facade, this module names exactly the methods `movement.js` and
//! `mantle.js` call as two narrow traits — [`movement::CharacterController`] and
//! [`mantle::WorldProbe`] — the same seam shape the audio port used for its own
//! `WorldProbe` (`crate::audio::spatial::WorldProbe`). Whatever lands in
//! `src/physics/` binds these by implementing the traits; nothing here assumes
//! a particular collision backend. See each trait's doc comment for the exact
//! method-by-method mapping back to the source's duck-typed calls.
//!
//! A third seam, [`movement::PlayerInput`], stands in for `ctx.input`
//! (`src/core/input.js`), which — like the physics seam — is not part of this
//! port. `Time` and `Config` are **not** re-seamed: `crate::engine::Time` and
//! `crate::config::Config` already carry every field `movement.js`/`camera.js`
//! read from `ctx.time`/`ctx.config`, so the player module takes them directly.

pub mod camera;
pub mod mantle;
pub mod movement;
pub mod springs;
pub mod tuning;
pub mod health;
pub mod lowhealth;
pub mod system;

/// A bare `(x, y, z)` triple — the port's stand-in for `THREE.Vector3` at
/// module boundaries. Not a math type: no operators, no methods. Every
/// subsystem in this crate that needs vector *arithmetic* does it inline as
/// plain `f64` component math, exactly as the source's `Vector3.set`/`.copy`
/// call sites do; this alias exists only so signatures read `Vec3` instead of
/// `[f64; 3]` at every call site.
pub type Vec3 = [f64; 3];
