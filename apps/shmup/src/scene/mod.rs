//! **The composition tier** — where the ported subsystems stop being ten
//! isolated ports and become one running scene.
//!
//! Nothing under here is a port of a single source file; each module is the
//! composition step the source performs across several. The citations are per
//! function, not per module.
//!
//! | module        | what it composes |
//! |---------------|------------------|
//! | [`level`]     | `world/index.js`'s `WorldSystem.init` — the assembler, the ground, the collision BVH, the spawn table |
//! | [`wiring`]    | every ported `<name>/index.js` facade, constructed and stepped |
//! | [`game`]      | `player/index.js`'s `PlayerSystem` + `core/engine.js`'s frame ordering |
//! | [`app`]       | `main.js` — the browser bootstrap, on Axiom's engine path |
//! | [`furniture`] | **not** a port — a labelled placeholder standing in for the unported `dressing.js`, so the prop library is not dead |
//!
//! The dependency direction is one-way: everything here reads the ported
//! subsystems, and no ported subsystem reads anything here.

pub mod app;
pub mod furniture;
pub mod wiring;
pub mod game;
pub mod level;

