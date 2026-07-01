//! # Axiom Physics — Engine Module
//!
//! A deterministic 3D rigid-body physics subsystem, modeled architecturally
//! after real engine physics systems (Unreal Chaos, Unity Physics, Godot
//! Physics) but built in Axiom's strict, branchless, fully-covered style.
//!
//! ```text
//! kernel ids / dimensioned scalars / errors
//!   + runtime fixed step
//!   + math Vec3 / Transform
//!     -> deterministic rigid-body world, linear integration, snapshots
//! ```
//!
//! ## What this module is
//! - An *isolated* engine module depending only on the approved layers
//!   [`axiom_kernel`], [`axiom_runtime`], and [`axiom_math`].
//! - The single owner of [`crate::PhysicsApi`]'s physics world: bodies,
//!   colliders, mass properties, forces, commands, events, and snapshots.
//!
//! ## What this module is not
//! It does not mutate or even know about a scene, renderer, mesh, asset, input,
//! animation, audio, ECS world, plugin host, or editor. It has no browser/GPU
//! APIs, no wall-clock time, and no randomness. Composition — translating a
//! [`PhysicsSnapshot`](crate::PhysicsApi) into scene/render state — is the job of
//! an app or a future feature module, never of physics itself.
//!
//! ## Current scope
//! A deterministic rigid-body world with a live collision pipeline:
//! semi-implicit integration of both **linear and angular** motion (orientation is
//! integrated from accumulated torque and a per-shape diagonal inverse inertia),
//! per-world linear/angular **damping**, an `O(n²)` AABB broad phase, a narrow
//! phase generating contacts for sphere/sphere, sphere/plane, sphere/box, and
//! box/plane pairs, and a sequential-impulse solver with restitution, a
//! deterministic **tangential friction** pass (Coulomb-clamped), and Baumgarte
//! position correction, all run under deterministic substepping with atomic
//! non-finite rollback. Spatial queries answer exact sphere/box/plane tests, and
//! per-step diagnostic counts and lifecycle events are reported.
//!
//! Genuinely deferred (do not assume these exist yet — see `ARCHITECTURE.md` and
//! `ROADMAP.md`): capsule and box/box contacts, oriented-box (rotated) contacts,
//! collision/trigger lifecycle events, and cross-instance (cross-build) f32
//! determinism — replay is proven **same-binary** only.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade — [`PhysicsApi`] — plus its
//! identity vocabulary: the [`PhysicsBodyHandle`] and [`PhysicsColliderHandle`]
//! handles the facade returns and accepts. Every other type (configs, bodies,
//! colliders, shapes, materials, snapshots, records, events) stays reachable
//! only through the facade.

mod broad_phase_pair;
mod collider_bounds;
mod contact_manifold;
mod contact_pair;
mod contact_report;
mod contact_solver;
mod force_accumulator;
mod ids;
mod integrator;
mod mass_properties;
mod physics_api;
mod physics_body;
mod physics_body_desc;
mod physics_body_handle;
mod physics_body_kind;
mod physics_collider;
mod physics_collider_handle;
mod physics_collider_shape;
mod physics_command;
mod physics_config;
mod physics_error;
mod physics_error_code;
mod physics_event;
mod physics_material;
mod physics_query;
mod physics_result;
mod physics_shape_kind;
mod physics_snapshot;
mod physics_step_record;
mod physics_step_result;
mod physics_world;

pub use ids::{PhysicsBodyHandle, PhysicsColliderHandle};
pub use physics_api::PhysicsApi;
