//! The curated `axiom` prelude: the single barrel an app imports.
//!
//! `use axiom::prelude::*;` brings in the whole high-level surface. The skeleton
//! re-exports the math primitives and the ecs schedule phases an app names; the
//! ergonomic value types (`Mesh`, `Material`, `Assets`, `SceneCommands`, the
//! component bundles) and `App`/`DefaultPlugins` are added as features land.

pub use axiom_ecs::SchedulePhase;
pub use axiom_math::{Mat4, Transform, Vec2, Vec3, Vec4};
