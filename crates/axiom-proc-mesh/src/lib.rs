//! # Axiom Proc-Mesh — mesh operators (layer)
//!
//! A tiny set of mesh operators that a recipe composes into a neutral
//! [`MeshBuffer`] (parallel position / normal / uv streams + a triangle-list
//! index buffer). Four **sources** (Cube, Cylinder, Grid, Sphere) and seven
//! **transforms** (Transform, Extrude, Bevel, Bend, Displace, UVProject,
//! Triangulate), dispatched branchlessly by a `const` table over the operator
//! code and baked through the shared [`axiom_proc_core::ProcCore`] executor.
//!
//! ## What it is, and is not
//! - **Neutral output.** A [`MeshBuffer`] is plain geometry — the shape an app
//!   translates into `axiom::MeshData`. It names no engine type.
//! - **Deterministic.** The same recipe and seed produce identical geometry; the
//!   Displace operator draws its noise seed from the node's `axiom-entropy`
//!   stream.
//! - **Branchless + bounded.** Dispatch is a table index; subdivision clamps and
//!   [`MAX_VERTS`] caps output, so a recipe can never ask for an unbounded mesh.
//!   The transform/deform operators are deliberately simple v0 forms.

mod dispatch;
mod mesh_buffer;
mod mesh_op;
mod primitives;
mod proc_mesh_api;
mod transforms;

pub use mesh_buffer::{MeshBuffer, MAX_VERTS};
pub use mesh_op::MeshOp;
pub use proc_mesh_api::ProcMeshApi;
