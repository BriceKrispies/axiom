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
mod app;
mod assets;
mod bundle;
mod camera;
mod color;
mod controller;
mod default_plugins;
mod directional_light;
mod frame_outcome;
mod handle;
mod material;
mod mesh;
mod mesh_geometry;
mod player;
mod point_light;
mod renderable;
mod scene_commands;
mod spin;
mod texture;
mod window;

pub mod prelude;
