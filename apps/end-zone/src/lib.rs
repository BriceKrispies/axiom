//! # End Zone — arcade-football engine framework + deterministic showcase
//!
//! A composition-leaf Axiom app (`apps/end-zone`). Everything football-specific
//! lives here; the engine is consumed only through its public facades (the
//! `axiom` umbrella's `RunningApp`, `PhysicsApi`, `FigureApi`, `InputState`,
//! and — on wasm32 — `WindowingApi` + `DebugOverlayApi`).
//!
//! The app is cut into four one-way boundaries (see `ARCHITECTURE.md`):
//!
//! ```text
//! input commands
//!   → fixed-step deterministic simulation      (state, player, football, ai)
//!   → ordered simulation events                (events)
//!   → immutable presentation snapshot          (presentation::snapshot)
//!   → camera director + presentation effects   (camera, presentation)
//!   → Axiom scene/render submission            (app, web)
//! ```
//!
//! Presentation never mutates simulation state; juice and camera shake react
//! only to typed [`events::SimEvent`]s. All variation derives from the explicit
//! seed in [`config::EndZoneConfig`] — no wall clock, no ambient randomness.

pub mod ai;
pub mod app;
pub mod camera;
pub mod config;
pub mod data;
pub mod debug;
pub mod events;
pub mod field;
pub mod football;
pub mod identity;
pub mod physics_rig;
pub mod player;
pub mod presentation;
pub mod scene;
pub mod scene_sync;
pub mod showcase;
pub mod state;

#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::end_zone_start;

pub use app::{build_end_zone, EndZoneApp};
