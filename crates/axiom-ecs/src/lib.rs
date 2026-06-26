//! # Axiom ECS — Layer 05
//!
//! The engine's single **world model**: one deterministic place entities and
//! their component data live, that every feature module and app composes on.
//!
//! Components are stored in **sparse per-type columns** ([`ComponentColumn`]) —
//! an entity appears in a column only if it has that component — assembled into
//! a consumer-defined storage struct. The world itself ([`World`]) owns only
//! entity lifecycle ([`EntityRegistry`]) and the systems that advance it; it
//! knows nothing about which component types exist, so modules define their own
//! and the composition root assembles them. Entities are keyed by the kernel's
//! [`axiom_kernel::EntityId`] in ascending order, so the world is
//! replay-deterministic. Registered [`WorldSystem`]s advance once per engine
//! frame via [`World::advance`], gated on the frame's lifecycle — this layer's
//! adapter over the frame layer.
//!
//! ## What this layer is not
//! Not a renderer, scene graph, or transform hierarchy. Those are component
//! columns + systems a consumer defines on top of this store. Keeping the world
//! generic over its component set is what lets one substrate serve scene,
//! physics, animation, and gameplay without any of them depending on each other.
//!
//! ## Public surface
//! [`EcsApi`] is the documented entry point that constructs the layer's
//! primitives. Those primitives stay public so consumers can name them:
//! [`World`] (the world model) over a [`ColumnSet`] of [`ComponentColumn`]s,
//! [`EntityRegistry`] minting generational [`EntityHandle`]s, [`Query`] over
//! columns (with [`QueryFilterExt`] presence filters), [`CommandBuffer`] /
//! [`ComponentCommandBuffer`] (+ [`CommandReport`]/[`CommandOutcome`]) for
//! barrier-applied structural change, [`EventBuffer`],
//! [`TrackedColumn`] (+ [`ChangeKind`]) for change detection, [`ReplayLog`],
//! [`ComponentTypeId`], [`WorldSystem`]/[`WorldStep`]/[`SchedulePhase`], and
//! [`ErasedColumn`]/[`DynamicComponents`]. Curated set enforced by
//! `tests/architecture.rs::lib_exports_are_curated_set`.

mod change_query;
mod column_set;
mod command_buffer;
mod component_column;
mod component_command_buffer;
mod component_type_id;
mod dynamic_components;
mod ecs_api;
mod entity_handle;
mod entity_registry;
mod erased_column;
mod event_buffer;
mod query;
mod query_filter;
mod replay_log;
mod schedule_phase;
mod tracked_column;
mod world;
mod world_step;
mod world_system;

#[cfg(test)]
mod fixtures;

pub use column_set::ColumnSet;
pub use command_buffer::{CommandBuffer, CommandOutcome, CommandReport};
pub use component_column::ComponentColumn;
pub use component_command_buffer::ComponentCommandBuffer;
pub use component_type_id::ComponentTypeId;
pub use dynamic_components::DynamicComponents;
pub use ecs_api::EcsApi;
pub use entity_handle::EntityHandle;
pub use entity_registry::EntityRegistry;
pub use erased_column::ErasedColumn;
pub use event_buffer::EventBuffer;
pub use query::Query;
pub use query_filter::QueryFilterExt;
pub use replay_log::ReplayLog;
pub use schedule_phase::SchedulePhase;
pub use tracked_column::{ChangeKind, TrackedColumn};
pub use world::World;
pub use world_step::WorldStep;
pub use world_system::WorldSystem;
