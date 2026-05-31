//! # Axiom ECS — Layer 05
//!
//! The engine's single **world model**: one deterministic place entities and
//! their component data live, that every feature module and app composes on.
//!
//! It is deliberately **generic** — a consumer supplies the component row type
//! `R` (typically a struct of `Option` components) — and knows nothing about
//! transforms, scenes, rendering, or any concrete component. Entities are keyed
//! by the kernel's [`axiom_kernel::EntityId`] and held in id order, so the world
//! is replay-deterministic. Registered [`WorldSystem`]s advance the world once
//! per engine frame via [`World::advance`], gated on the frame's lifecycle —
//! the same pattern `axiom-scene::advance` uses, and this layer's adapter over
//! the frame layer.
//!
//! ## What this layer is not
//! Not a renderer, not a scene graph, not a transform hierarchy. Those are
//! *systems and component rows* a consumer defines on top of this store. Keeping
//! the world generic is what lets one substrate serve scene, physics, animation,
//! and gameplay without any of them depending on each other.
//!
//! ## Public surface
//! [`World`] (the facade), [`EntityStore`] (its entity container), and
//! [`WorldSystem`] (the per-frame behavior contract). Curated set enforced by
//! `tests/architecture.rs::lib_exports_are_curated_set`.

mod entity_store;
mod world;
mod world_system;

#[cfg(test)]
mod fixtures;

pub use entity_store::EntityStore;
pub use world::World;
pub use world_system::WorldSystem;
