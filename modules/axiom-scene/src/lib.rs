//! # Axiom Scene — Engine Module
//!
//! The deterministic 3D scene graph and spatial-object module.
//! `axiom-scene` adapts the layer spine ([`axiom_math`] transforms +
//! [`axiom_frame`] stepping) into concrete scene state:
//!
//! ```text
//! MathApi transforms + Frame/Runtime stepping
//!     -> deterministic scene graph state and stable scene snapshots
//! ```
//!
//! ## What this module is
//! - An *isolated* engine module that depends only on approved layers
//!   (`axiom-kernel`, `axiom-runtime`, `axiom-math`, `axiom-frame`).
//! - The single place that owns scene topology, transforms, cameras,
//!   lights, and renderable references.
//! - The single place that builds a deterministic [`SceneSnapshot`]
//!   future apps and engine systems consume.
//!
//! ## What this module is not
//! Not a renderer, not a render graph, not a resource/asset module, not
//! a physics system, not an animation system, not an input mapper, not
//! a plugin host, not an editor, not gameplay. It also has no
//! dependency on `axiom-host`, no browser APIs, no GPU APIs, no
//! wall-clock time, no randomness.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`SceneApi`]. Every other
//! type is reachable only through the facade — internal modules each
//! own one primary public thing but are never re-exported here.

mod camera;
mod camera_snapshot;
mod controller_command;
mod light;
mod light_kind;
mod light_snapshot;
mod material_ref;
mod mesh_ref;
mod node_snapshot;
mod player_command;
mod renderable;
mod renderable_snapshot;
mod scene;
mod scene_api;
mod scene_error;
mod scene_error_code;
mod scene_node_id;
mod scene_result;
mod scene_snapshot;
mod scene_storage;
mod spin;

pub use scene_api::SceneApi;
