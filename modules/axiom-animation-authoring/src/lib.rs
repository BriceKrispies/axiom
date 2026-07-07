//! # Axiom Animation Authoring — Engine Module
//!
//! A small, deterministic, inspectable **procedural-animation authoring
//! vocabulary** for humanoid motion. Where the sibling `axiom-animation` module
//! owns skeletal *mechanics* (skeletons, poses, clip sampling, blending), this
//! module owns the higher-level *authoring* layer:
//!
//! ```text
//! HumanoidRigSpec + MotionSpec (phases -> pose goals / constraints / events)
//!     -> (MotionCompiler validates) -> MotionPlan
//!     -> (MotionSampler at a Tick) -> PoseFrame
//! ```
//!
//! A [`AnimationAuthoringApi`] authors a rig and a motion as *data*, compiles the
//! motion (rejecting unknown joints/effectors/targets, invalid or overlapping
//! phase ranges, and non-finite values), and samples the compiled plan into a
//! deterministic [pose frame] of root/joint/effector transforms plus the active
//! constraints, contacts, and emitted events at that tick. Sampling the same plan
//! at the same tick is byte-for-byte reproducible: no wall-clock time, no
//! randomness, no console output.
//!
//! ## Engine owns mechanism; games own meaning
//! The vocabulary here (a `leg_backswing` goal, a `ball_contact` event) is
//! game-*agnostic* mechanism. The built-in `soccer_penalty_kick_v0` motion is a
//! worked example authored *with* that vocabulary, not new engine concepts — a
//! future game or editor authors its own motions the same way.
//!
//! ## What this module is / is not
//! - An *isolated* engine module depending only on the [`axiom_kernel`] and
//!   [`axiom_math`] layers (`allowed_modules = []`). Engine modules may not depend
//!   on each other, so it consumes no scene/skeleton module and owns its output as
//!   neutral data.
//! - Not a renderer, physics engine, input mapper, asset store, editor UI, or
//!   game app. Turning a [pose frame] into scene/render data is the job of an app,
//!   which reads the facade's `frame_*` accessors and writes its own scene.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade — [`AnimationAuthoringApi`]
//! — plus its identity vocabulary ([`RigId`], [`MotionId`], [`PhaseId`],
//! [`PlanId`], [`JointId`], [`EffectorId`], [`TargetId`]). Every other type
//! (rig spec, motion spec, phases, pose goals, constraints, events, plan, pose
//! frame, errors) is reached only through the facade.
//!
//! [pose frame]: AnimationAuthoringApi::sample

mod authoring_api;
mod authoring_error;
mod authoring_error_code;
mod authoring_result;
mod constraint;
mod contact;
mod ease;
mod effector;
mod humanoid_rig;
mod ids;
mod joint;
mod motion_compiler;
mod motion_event;
mod motion_phase;
mod motion_plan;
mod motion_sampler;
mod motion_spec;
mod penalty_kick;
mod physical_objective;
mod pose_frame;
mod pose_goal;
mod root_motion;

pub use authoring_api::AnimationAuthoringApi;
pub use ids::{EffectorId, JointId, MotionId, PhaseId, PlanId, RigId, TargetId};
