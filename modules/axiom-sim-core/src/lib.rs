//! # Axiom sim-core — Engine Module (Phase 2 simulation substrate)
//!
//! A generic, deterministic *simulation language* — the vocabulary later phases
//! use to express Dwarf-Fortress-like interactions, with none of the domain
//! content. sim-core owns six things and nothing else:
//!
//! - **Facts** — typed assertions about subject entities.
//! - **Relations** — typed links between ordered subjects.
//! - **Definitions** — a registry of data-defined concepts (tags + properties).
//! - **Processes** — tick-scheduled activities, woken from a deterministic queue.
//! - **Effects** — proposed mutations applied only at an explicit boundary.
//! - **Causal journal** — structured "why did this happen" cause tracking.
//! - **Materials/substances** — quantities, residues, interactions, transfer
//!   rules, and material-effect rules over the definition registry.
//! - **Body/anatomy** — body plans, tissue definitions, instantiated bodies with
//!   parts and surfaces, body routes, and wound records.
//! - **Process scheduler** — logical ticks, a process lifecycle + wake queue,
//!   dirty-fact/relation/subject invalidation, dependency subscriptions, a process
//!   handler seam, and an explicit effect boundary that drives long-lived
//!   simulation without scanning the whole world each tick.
//!
//! All of these are *generic substrate*: sim-core names no concrete material,
//! creature, organ, or behavior — later phases supply meaning as data + codes.
//!
//! ## What this module is not
//! It is **not** a layer, and it owns no domain meaning: no concrete materials,
//! creatures, bodies-of-a-species, fluids simulation, toxicology, combat, jobs,
//! needs, thoughts, history, AI, gameplay, scene graph, rendering, physics, input,
//! audio, or browser/GPU APIs. It references ECS entity handles
//! (the ecs layer) but does not own the ECS; entity liveness is checked against a
//! borrowed `axiom_ecs::EntityRegistry`. It uses only logical ticks — never
//! wall-clock time — and no randomness.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`SimCoreApi`] — plus the
//! module's **identity vocabulary**: the pure `u64`-backed id newtypes the facade
//! traffics in ([`FactId`], [`ProcessId`], [`ResidueId`], [`BodySurfaceId`], …).
//! Those ids carry no behavior; they are simply the *nouns* the facade returns,
//! and a composition root cannot retain a handle across ticks unless it can name
//! the handle's type (the same reason the `ecs` layer exports `EntityHandle`).
//! Every other type (facts, relations, effects, the causal journal, …) stays
//! behind the facade and is reachable only through it.

mod body;
mod body_plan;
mod body_route;
mod body_surface;
mod causal;
mod cause;
mod definition;
mod dirty_set;
mod effect;
mod facade;
mod fact;
mod ids;
mod interaction;
mod material;
mod material_effect;
mod process;
mod process_dependency;
mod process_handler;
mod process_lifecycle;
mod quantity;
mod relation;
mod residue;
mod scheduler;
mod sim_tick;
mod sim_world;
mod tissue;
mod transfer;
mod wake_reason;
mod wound;

pub use facade::SimCoreApi;
pub use ids::{
    BodyId, BodyPartId, BodyPlanId, BodySurfaceId, CausalEventId, DefinitionId, FactId,
    InteractionId, MaterialEffectRuleId, ProcessId, RelationId, ResidueId, RuleId, TissueId,
    TransferRuleId, WoundId,
};
