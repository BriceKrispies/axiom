//! # Axiom animation — Engine Module
//!
//! A deterministic, **data-first skeletal-animation core**. It owns the runtime
//! animation data model and nothing else: it has no scene graph, no renderer, no
//! physics, no input, and no platform arm. It builds purely on the math layer's
//! dimensionless linear algebra (`Vec3`/`Quat`/`Transform`) and hands back
//! neutral pose data an app — or a higher composition tier — can translate into
//! a renderable rig.
//!
//! ## Shape
//! - **[`AnimationApi`]** — the one behavioral facade. Build the default
//!   humanoid, validate a skeleton, sample a clip at a frame, clamp a pose to
//!   its joint limits, query phases/events, and run forward kinematics.
//! - The **value vocabulary** (re-exported via `pub use ids::{…}`):
//!   - Topology — [`SkeletonDefinition`], [`BoneDefinition`], [`SkeletonError`].
//!   - Poses — [`BindPose`], [`Pose`], [`LocalBoneTransform`].
//!   - Clips — [`AnimationClip`], [`BoneTrack`], [`Keyframe`], [`ClipPhase`],
//!     [`PhaseKind`], and the [`ClipSampler`].
//!   - Events — [`EventTrack`], [`AnimationEvent`], [`EventKind`].
//!   - Limits — [`JointLimit`], [`PoseSolver`].
//!   - [`HumanoidPrefab`] — an editable low-poly humanoid plus an authored
//!     right-foot soccer kick.
//!
//! ## Determinism
//! Time is an integer **frame**, never wall-clock. Sampling a clip at frame `f`
//! is a pure function of the clip and `f`, so scrubbing to a frame — forward or
//! back — always reproduces the same pose. The animated degree of freedom is a
//! per-joint Euler rotation, the representation joint limits clamp and keyframes
//! interpolate; it becomes a composable quaternion only when forward kinematics
//! needs it.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`AnimationApi`] — plus
//! the pure value vocabulary above, re-exported through a single `ids` module.

mod animation_api;
mod clip;
mod events;
mod ids;
mod pose;
mod prefab;
mod sampler;
mod skeleton;
mod solver;

pub use animation_api::AnimationApi;
pub use ids::{
    AnimationClip, AnimationEvent, BindPose, BoneDefinition, BoneTrack, ClipPhase, ClipSampler,
    EventKind, EventTrack, HumanoidPrefab, JointLimit, Keyframe, LocalBoneTransform, PhaseKind,
    Pose, PoseSolver, SkeletonDefinition, SkeletonError,
};
