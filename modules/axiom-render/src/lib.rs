//! # Axiom Render — Engine Module
//!
//! Backend-neutral render compilation. `RenderApi` takes a
//! scene-independent [`RenderInput`](crate::render_input::RenderInput)
//! and produces a deterministic
//! [`RenderCommandList`](crate::render_command_list::RenderCommandList).
//!
//! ## What this module is
//! - The translator from a frame's *render-facing* inputs (camera
//!   matrices, light arrays, mesh/material arrays, objects) into a
//!   deterministic command stream.
//! - The owner of `RenderInput`, `RenderCommand`, and
//!   `RenderCommandList`.
//!
//! ## What this module is not
//! Not a renderer with a backend. Not a scene module. Not a resources
//! module. Not a WebGPU module. Not an asset loader. The module
//! imports no host APIs and no other module.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`RenderApi`].

mod render_api;
mod render_camera;
mod render_command;
mod render_command_list;
mod render_input;
mod render_light;
mod render_material;
mod render_mesh;
mod render_object;
mod render_pipeline_kind;

pub use render_api::RenderApi;
