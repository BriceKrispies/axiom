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
//! | [`sky_look`]  | `sky/index.js`'s per-frame key-light and sky terms, from the CPU atmosphere model |
//! | [`game`]      | `player/index.js`'s `PlayerSystem` + `core/engine.js`'s frame ordering |
//! | [`app`]       | `main.js` — the browser bootstrap, on Axiom's engine path |
//!
//! The dependency direction is one-way: everything here reads the ported
//! subsystems, and no ported subsystem reads anything here.

pub mod app;
pub mod game;
pub mod level;
pub mod sky_look;
