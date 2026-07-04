//! # Axiom Recipe — the procedural recipe graph (layer)
//!
//! A **recipe** is a stable-id'd, versioned DAG of operator nodes. Each node
//! carries an opaque operator code, a flat list of raw parameter words, and
//! links to strictly-earlier nodes. This layer owns the **container only**:
//!
//! - [`RecipeGraph`] — the graph, its append-only builder, validation, and
//!   deterministic little-endian serialization (`SchemaVersion`-stamped,
//!   `StableHash`-digested).
//! - [`Node`] / [`NodeId`] / [`RecipeId`] — the graph's identity and structure.
//! - [`Param`] + its typed views [`Scalar`] / [`Color`] — how an operator reads
//!   a raw parameter word without any per-value branch.
//! - [`RecipeError`] / [`RecipeResult`] — stable validation/decode failures.
//!
//! ## What it is, and is not
//! - **Domain-free.** An operator code is an opaque `u16`; a parameter is an
//!   opaque `u32` word. What a code *means* — a noise texture, a cube mesh — and
//!   how a node is *evaluated* belong to a higher generation layer, never here.
//! - **Acyclic by construction.** A node's inputs must reference strictly-earlier
//!   nodes; [`RecipeGraph::validate`] enforces it, which is exactly the cycle
//!   check (a back/forward edge is the only way to form a cycle in an id-ordered
//!   append graph).
//! - **Branchless + deterministic.** Serialization is canonical little-endian
//!   bytes; the bytes are the determinism proof and the digest is their label.

mod ids;
mod node;
mod recipe_error;
mod recipe_graph;
mod value;

pub use ids::{NodeId, RecipeId};
pub use node::Node;
pub use recipe_error::{RecipeError, RecipeResult};
pub use recipe_graph::{RecipeGraph, MAX_NODES};
pub use value::{Color, Param, Scalar};
