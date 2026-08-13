//! The mesh layer's result alias.

use crate::mesh_error::MeshError;

/// The result of a fallible mesh construction, validation, or geometry
/// operation. Every fallible entry point in `axiom-mesh` and `axiom-mesh-ops`
/// returns this; nothing in either layer panics on data it was handed.
pub type MeshResult<T> = Result<T, MeshError>;
