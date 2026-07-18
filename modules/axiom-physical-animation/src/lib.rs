//! # Axiom Physical Animation — Feature Module (the bridge)
//!
//! The composition tier between the deterministic **animation-authoring**
//! vocabulary and the real **axiom-physics** engine. Authoring emits neutral
//! *physical objectives* from a compiled `MotionPlan`; this module binds a
//! humanoid rig to physics bodies, translates those objectives into physics
//! forces / impulses / kinematic drives, steps the world deterministically, and
//! reads back a [`PhysicalAnimationFrame`].
//!
//! ```text
//! MotionPlan (axiom-animation-authoring)
//!   -> physical objectives (root velocity / foot plant / joint motors / ball impulse / gaze)
//!   -> HumanoidPhysicsBinding (rig joints -> axiom-physics bodies)
//!   -> apply to axiom-physics (apply_force / apply_impulse / set_body_transform)
//!   -> PhysicsApi::step (deterministic fixed RuntimeStep)
//!   -> PhysicsApi::snapshot -> PhysicalAnimationFrame
//! ```
//!
//! ## What this module is / is not
//! - A **feature module** composing exactly two engine modules (the sanctioned
//!   exception): `animation-authoring` and `physics`, over the `kernel`, `math`,
//!   and `runtime` layers. It owns **no** simulation and **no** authoring — both
//!   are reached through their facades ([`axiom_physics::PhysicsApi`] and
//!   `axiom_animation_authoring::AnimationAuthoringApi`).
//! - Not a renderer, app, or scene. It draws nothing; an app reads the frame.
//!
//! ## The two animation paths
//! The authoring module's *pose path* (`sample` / `frame_*`) is unchanged — it
//! yields kinematic `PoseFrame`s. This module adds the *physics-backed path*:
//! [`PhysicalAnimationApi::advance`] steps a real physics world and yields
//! [`PhysicalAnimationFrame`]s. The two are named distinctly and both usable.
//!
//! ## Physics fidelity (honest, given the engine's real capabilities)
//! `axiom-physics` has rigid bodies, forces, impulses, torques, and kinematic
//! bodies — but **no joints or motors**, and building them is out of scope. So
//! the humanoid is a *hybrid*: the **ball** is a real dynamic body driven by a
//! real **impulse** at the strike (it then flies under gravity — never
//! teleported), the **pelvis/root** is a dynamic body driven by an anti-gravity
//! hold plus an approach **force**, and the **limbs** and the **planted foot** are
//! **kinematic** bodies driven from the authored pose. "Joint-motor" and "plant"
//! objectives are realized as kinematic drives — the closest available public
//! mechanism. Everything is same-binary deterministic.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade, [`PhysicalAnimationApi`],
//! plus its identity vocabulary — the [`HumanoidHandle`] the crowd surface hands
//! out and accepts. Every other type (the binding, objectives, frame, errors) is
//! reached only through the facade.

mod humanoid_binding;
mod ids;
mod muscle_group;
mod muscle_profile;
mod physical_animation_api;
mod physical_error;
mod physical_error_code;
mod physical_frame;
mod physical_result;
mod virtual_muscle;

pub use ids::HumanoidHandle;
pub use physical_animation_api::PhysicalAnimationApi;
