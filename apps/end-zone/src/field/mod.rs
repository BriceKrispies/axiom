//! The procedural field: one documented coordinate system, the generator that
//! builds every visible field piece once, and the marking/number geometry.

pub mod coordinates;
pub mod generator;
pub mod markings;

pub use coordinates::{
    normalized_to_world, world_to_yard_line, yard_line_to_z, DriveDirection, OffenseFrame,
    OffensePoint, FIELD_HALF_LENGTH, FIELD_HALF_WIDTH, GOAL_LINE_Z,
};
pub use generator::{generate_field, FieldGeometry, FieldMaterial, FieldMesh, FieldPiece};
