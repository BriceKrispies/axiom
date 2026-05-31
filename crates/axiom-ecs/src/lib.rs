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
//! [`World`] (the facade), [`EntityRegistry`] (live entities), [`ComponentColumn`]
//! (a sparse per-type store), and [`WorldSystem`] (per-frame behavior). Curated
//! set enforced by `tests/architecture.rs::lib_exports_are_curated_set`.

mod component_column;
mod entity_registry;
mod world;
mod world_system;

#[cfg(test)]
mod fixtures;

pub use component_column::ComponentColumn;
pub use entity_registry::EntityRegistry;
pub use world::World;
pub use world_system::WorldSystem;
