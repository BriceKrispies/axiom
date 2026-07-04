//! # Axiom Proc Player — expand recipes into a live scene (app)
//!
//! The Player half of Axiom Proc v0. It owns no operators and no calc; it is the
//! composition leaf that takes the four gated generation layers
//! (`axiom-recipe`, `axiom-proc-core`, `axiom-proc-texture`, `axiom-proc-mesh`)
//! and turns their **recipes** into **ordinary Axiom runtime resources**:
//!
//! - bake each texture recipe → RGBA8 buffer → `RunningApp::add_texture_data`,
//! - bake each mesh recipe → geometry → `RunningApp::add_mesh_data`,
//! - light the generated textures as plain `Material`s,
//! - place everything in a normal scene with a camera and a light,
//! - and report the sizes ([`RoomReport`]).
//!
//! The one demo — a small generated room (brick wall, stone floor, wooden crate)
//! — proves the load-bearing rule of the whole pipeline: *ship the recipe, not
//! the resources.* A few hundred recipe bytes expand into the scene at load time,
//! and nothing but those bytes is needed to do it.

pub mod recipes;
pub mod room;

pub use recipes::DemoRecipes;
pub use room::{expand, expand_room, RoomReport};
