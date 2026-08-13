//! # Axiom Mesh — the canonical indexed triangle mesh (layer)
//!
//! `mesh` owns the engine's one neutral CPU-side representation of triangle
//! geometry, [`Mesh`], and the operations that are intrinsic to that
//! representation: validation, derived bounds, generated normals and tangents,
//! transformation, combination, welding, a deterministic digest, and versioned
//! binary serialization.
//!
//! ## What it is, and is not
//!
//! - **Neutral.** A [`Mesh`] is positions, indices, and optional attribute
//!   streams. It names no material, texture, shader, GPU buffer, vertex layout,
//!   scene node, entity, resource id, or asset. An imported mesh and a
//!   procedurally generated mesh are the same value here — nothing downstream
//!   can tell them apart, which is the whole reason the type exists.
//! - **Structure of arrays.** Interleaving is a GPU vertex-layout decision and
//!   belongs to a backend, not to the representation.
//! - **Validated by construction.** [`Mesh::from_streams`] is the only
//!   constructor, so an invalid mesh is unrepresentable.
//! - **It owns no generator.** Producing geometry from a description is the
//!   `axiom-mesh-ops` layer's job. This layer would be identical if the engine
//!   had no procedural generation at all.
//!
//! ## Why a layer, depending on kernel + math
//!
//! Seven engine modules need to name triangle geometry (`resources`,
//! `terrain-mesh`, `figure`, `physics`, `draw2d`, `text`, `gpu-backend`), and an
//! engine **module** may never depend on another module — so the shared
//! primitive has to be a layer. It genuinely uses **math** (every attribute is a
//! `Vec2`/`Vec3`/`Vec4`, bounds are `Aabb`/`Sphere`, transforms are `Mat4`) and
//! **kernel** (`StableHash` for the digest, `BinaryWriter`/`BinaryReader` +
//! `SchemaVersion` for serialization, `Meters` to keep naked floats off the
//! boundary).
//!
//! ## Winding convention
//!
//! Right-handed, Y-up. **Counter-clockwise triangles are front-facing.** For
//! triangle `(a, b, c)` the geometric normal is
//! `(p[b] - p[a]).cross(p[c] - p[a])`. UV origin `(0, 0)` is the lower-left;
//! `v` increases upward. Tangent `w` is `+1` when
//! `bitangent == normal.cross(tangent.xyz)`, `-1` otherwise.

// The representation and its contract.
mod mesh;
mod mesh_error;
mod mesh_error_code;
mod mesh_result;
mod mesh_streams;
mod mesh_validation;

// Geometry derived from the representation.
mod mesh_bounds;
mod mesh_normals;
mod mesh_tangents;

// Transformations of the representation.
mod mesh_combine;
mod mesh_transform;
mod mesh_weld;

// Deterministic identity.
mod mesh_binary;
mod mesh_digest;

pub use mesh::Mesh;
pub use mesh_error::MeshError;
pub use mesh_error_code::MeshErrorCode;
pub use mesh_result::MeshResult;
pub use mesh_streams::MeshStreams;
pub use mesh_validation::{validate_streams, SKIN_WEIGHT_TOLERANCE};

pub use mesh_bounds::{aabb, bounding_sphere};
pub use mesh_normals::{generate_flat_normals, generate_normals};
pub use mesh_tangents::generate_tangents;

pub use mesh_combine::combine;
pub use mesh_transform::{reverse_winding, transform};
pub use mesh_weld::{remove_degenerate_triangles, weld};

pub use mesh_binary::{decode_mesh, encode_mesh, MESH_SCHEMA_VERSION};
pub use mesh_digest::digest;
