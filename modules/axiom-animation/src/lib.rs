//! # Axiom Animation — Engine Module
//!
//! Minimal, deterministic **skeletal-animation mechanics**. This module owns the
//! reusable machinery every animated game needs — skeletons, poses, clip
//! sampling, and pose blending — and nothing about any particular game.
//!
//! ```text
//! kernel Tick / Ratio / ids + math Transform / Quat::nlerp
//!     -> deterministic skeleton poses sampled and blended over engine ticks
//! ```
//!
//! ## Engine owns mechanism; games own meaning
//! A skeleton is a parented set of bones, a pose is one local transform per
//! bone, a clip is per-bone keyframe tracks sampled at a [`axiom_kernel::Tick`],
//! and two poses blend by a [`axiom_kernel::Ratio`]. Which clip a "kicker" plays
//! and when a "goalie" dives is **meaning** — it lives in an app or a game
//! cartridge, never here. There are no character names, humanoid assumptions, or
//! gameplay state machines in this module.
//!
//! ## What this module is
//! - An *isolated* engine module depending only on the approved layers
//!   [`axiom_kernel`] and [`axiom_math`] (`allowed_modules = []`).
//! - The single owner of skeleton topology, poses, clip sampling, and blending.
//!
//! ## What this module is not
//! Not a scene, renderer, resource/asset store, physics, input mapper, or audio
//! module — it imports none of them. It has no browser/GPU APIs, no wall-clock
//! time, and no randomness. Turning a resolved model pose into scene node
//! transforms is the job of an app, which reads [`AnimationApi::resolve_model`]
//! and writes the results into its scene.
//!
//! ## Deliberately deferred
//! This is the deterministic *contract*, not a full animation engine. A stateful
//! `Animator` that advances play-time and an `AnimationGraph` state machine are
//! **not** implemented here — they are higher constructs that build on this
//! sampling/blending contract. See `ARCHITECTURE.md`.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade — [`AnimationApi`] — plus
//! its identity vocabulary ([`SkeletonId`], [`ClipId`], [`BoneId`]). Every other
//! type (skeletons, bones, poses, clips, keyframes, errors) is reached only
//! through the facade.

mod animation_api;
mod animation_error;
mod animation_error_code;
mod animation_result;
mod blend;
mod bone;
mod clip;
mod clip_event;
mod clip_phase;
mod ids;
mod interpolate;
mod joint_limit;
mod keyframe;
mod pose;
mod skeleton;
mod track;

pub use animation_api::AnimationApi;
pub use ids::{BoneId, ClipId, SkeletonId};
