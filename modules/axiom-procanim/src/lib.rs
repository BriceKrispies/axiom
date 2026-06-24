//! # axiom-procanim — deterministic procedural animation
//!
//! The animation half of proc-driven rendering. Where the procedural-generation
//! substrate ([`axiom-proc`](axiom_proc) et al.) computes *what exists*, this
//! module computes *how it moves*: [`ProcAnimApi::animate`] turns
//! `(seed, address, tick)` into an animated transform — a fixed-point position
//! offset, yaw, and scale.
//!
//! Each entity's motion is keyed by its [`axiom_space::Address`]: an
//! [`axiom_entropy`] stream over that address draws the entity's animation
//! parameters once, and a fixed-point sine oscillation + a continuous ramp turn
//! them into per-tick motion. So the same entity animates identically on every
//! run and platform, and distinct entities animate distinctly — the determinism a
//! proc-driven renderer (and lockstep multiplayer) needs.
//!
//! It is integer-only (no naked floats; an app converts the transform to the
//! engine's f32 `Transform` at the GPU edge) and branchless. The returned
//! transform is read through its methods (`offset`/`yaw`/`scale`); like the other
//! substrate modules, the result type is not part of the crate's named surface —
//! [`ProcAnimApi`] is the single facade.

mod animated_transform;
mod procanim_api;

pub use procanim_api::ProcAnimApi;
