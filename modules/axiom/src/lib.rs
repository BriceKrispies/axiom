//! # Axiom — the engine umbrella
//!
//! The one crate an app imports. `axiom` is a **feature module** that composes
//! the engine modules (scene, resources, render-pipeline, webgpu) and the layer
//! spine into the high-level surface the north-star targets — `App`,
//! `DefaultPlugins`, `Assets<T>`, `SceneCommands`, and the component bundles —
//! so an app is pure scene description instead of hand-wired boundaries.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`prelude`]. Everything an app needs
//! is re-exported from there, so an app writes a single `use axiom::prelude::*;`.
//! That one `pub mod` is the module's single facade under Module Law #8.

mod angle;
mod assets;
mod bundle;
mod camera;
mod color;
mod directional_light;
mod handle;
mod material;
mod mesh;
mod renderable;
mod scene_commands;
mod spin;

pub mod prelude;
