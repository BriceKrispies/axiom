//! The module's value vocabulary — the pure data types [`AnimationApi`] traffics
//! in, gathered here so `lib.rs` can re-export them alongside the one behavioral
//! facade via a single `pub use ids::{…}` (Module Law #8). These types carry the
//! rig, its poses, its clips, its limits, and the humanoid prefab; the behavior
//! that operates on them lives behind [`AnimationApi`].
//!
//! [`AnimationApi`]: crate::AnimationApi

pub use crate::clip::{AnimationClip, BoneTrack, ClipPhase, Keyframe, PhaseKind};
pub use crate::events::{AnimationEvent, EventKind, EventTrack};
pub use crate::pose::{BindPose, LocalBoneTransform, Pose};
pub use crate::prefab::HumanoidPrefab;
pub use crate::sampler::ClipSampler;
pub use crate::skeleton::{BoneDefinition, SkeletonDefinition, SkeletonError};
pub use crate::solver::{JointLimit, PoseSolver};
