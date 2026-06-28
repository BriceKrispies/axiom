//! # Axiom Resources — Engine Module
//!
//! CPU-side resource descriptions: a deterministic [`ResourcesApi`] that
//! builds the built-in cube mesh, basic-lit material, and solid-colour
//! textures, plus a [`ResolvedResources`](crate::resolved_resources::ResolvedResources)
//! snapshot the app hands to the renderer.
//!
//! ## What this module is
//! - The owner of CPU-side mesh/material/texture data for the engine's
//!   built-in resources.
//! - The producer of `ResolvedResources` — the deterministic snapshot
//!   contract the demo app translates into a `RenderInput`.
//!
//! ## What this module is not
//! Not a GPU resource manager. Not an asset loader (no file IO, no
//! image decoding, no GLTF). Not a render module. Not a scene module.
//! Not a host module.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`ResourcesApi`].

mod basic_lit_material;
mod biome_atlas_texture;
mod checker_texture;
mod cube_mesh;
mod cylinder_mesh;
mod material_data;
mod mesh_data;
mod plane_mesh;
mod resolved_resources;
mod resource_id;
mod resource_table;
mod resources_api;
mod solid_color_texture;
mod sphere_mesh;
mod texture_data;
mod uv_grid_texture;
mod vertex;

pub use resources_api::ResourcesApi;
