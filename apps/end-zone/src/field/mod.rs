//! The procedural field: one documented coordinate system, the generator that
//! builds every static surface piece once, and the camera-driven paint system
//! that decides which markings are worth drawing this frame.

pub mod coordinates;
pub mod generator;
pub mod inspect;
pub mod paint;
pub mod paint_layout;

pub use coordinates::{
    normalized_to_world, world_to_yard_line, yard_line_to_z, z_to_yards_from_own_goal,
    DriveDirection, OffenseFrame, OffensePoint, FIELD_HALF_LENGTH, FIELD_HALF_WIDTH, FIELD_WIDTH,
    GOAL_LINE_Z,
};
pub use generator::{generate_field, FieldGeometry, FieldMaterial, FieldMesh, FieldPiece};
pub use paint::{
    classify, Lod, PaintCamera, PaintCategory, PaintConfig, PaintPalette, PAINT,
    PAINT_CATEGORY_COUNT, PAINT_Y, PALETTE,
};
pub use paint_layout::{
    field_paint, is_major_division, paint_pool_capacity, GameplayLines, PaintQuad,
};
