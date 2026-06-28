//! # Axiom tween — Engine Module (SPEC-09)
//!
//! Generic **display-value animation**: a number `from → to` over a duration
//! under an ease curve, advanced by a presentation interval the app supplies.
//! There is no UI here — this is the shared value-interpolation primitive every
//! presentation surface (2D, HUD, 3D) wants, driven *alongside* the surface by
//! the app (modules never import modules).
//!
//! ## Shape
//! - **[`TweenApi`]** — the one behavioral facade: a tween table keyed by
//!   [`TweenId`]. `start` a [`TweenSpec`], `advance` the whole table by an elapsed
//!   nanosecond interval to get a [`TweenSample`] per live tween, `value` a single
//!   tween, `cancel` to remove one. `TweenApi::ease` evaluates a curve directly.
//! - **[`Ease`]** — the seven curves, dispatched branchlessly by a fn-pointer
//!   table. Endpoints are exact (`ease(_, 0) == 0`, `ease(_, 1) == 1`); `BackOut`
//!   deliberately overshoots between them, which is why a sampled value is a
//!   free [`TweenValue`] float, not a clamped ratio.
//!
//! ## Determinism
//! Presentation-class (§17.5): every output is display-only and must never be
//! read back into a `sim` API. Time is integer nanoseconds and the math is a pure
//! function of the spec plus accumulated elapsed time, so — like the SPEC-00
//! accumulator — sampling at total elapsed `T` is independent of how `T` was
//! chunked across frames. The closures an author attaches (`onUpdate`/
//! `onComplete`) are **not** native data: they live app-side keyed by `TweenId`,
//! keeping this core branchless and closure-free.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`TweenApi`] — plus the
//! pure value vocabulary it traffics in ([`TweenId`], [`TweenSpec`],
//! [`TweenSample`], [`TweenValue`], [`Ease`]).

mod curve;
mod ids;
mod tween_api;

pub use ids::{Ease, TweenId, TweenSample, TweenSpec, TweenValue};
pub use tween_api::TweenApi;
