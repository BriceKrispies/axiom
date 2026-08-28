//! **The wiring tier** — where each ported subsystem facade is constructed,
//! stepped, and given the state it needs.
//!
//! Every module here exists because the port produced a complete `<name>/index.js`
//! facade that nothing constructed. The ports were finished; the game was not
//! assembled. These are the seams that assemble it.
//!
//! | module             | facades it drives |
//! |--------------------|-------------------|
//! | [`physics_player`] | `PhysicsCore` + `PlayerCore` |
//! | [`weapons`]        | `WeaponCore` (which owns the viewmodel) |
//! | [`ai`]             | `AiCore` — nav, squads, soldiers |
//! | [`soldier_draw`]   | `ai::soldier` + `ai::animator` — the bodies, skinned |
//! | [`fx_audio`]       | `FxSystem` + `AudioCore` |
//! | [`hud`]            | `UiCore` — the eleven widgets and their DOM views |
//! | [`look`]           | `SkySystem` + `MaterialSystem` |
//!
//! # Construction order is load-bearing
//!
//! These subsystems draw from the level's RNG stream, and the source's
//! `core/registry.js` topo-sorts `static deps` depth-first in insertion order,
//! giving exactly one init sequence:
//!
//! ```text
//! render, materials, sky, physics, world, player, weapons, fx, ai, ui, audio
//! ```
//!
//! `materials` and `sky` never touch `ctx.rng`; every other slot forks once. So
//! the order the constructors run in `Game::new` is not a style choice — build
//! one in the wrong place and every later value in the level silently moves.
//!
//! **The port's stream already diverges from the source's in absolute terms**:
//! `scene::level::build_level` takes two forks where `world/index.js:91` takes
//! one (a documented borrow-checker workaround). What these modules preserve is
//! the *relative* order, which is what keeps the subsystems consistent with each
//! other.

pub mod ai;
pub mod fx_audio;
pub mod fx_draw;
pub mod hud;
pub mod look;
pub mod physics_player;
pub mod sky_draw;
pub mod soldier_draw;
pub mod weapon_look;
pub mod weapons;
