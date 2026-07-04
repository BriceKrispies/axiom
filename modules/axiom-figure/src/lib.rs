//! # Axiom Figure — Engine Module
//!
//! A generic, **data-driven articulated box-figure**: a parented set of parts,
//! each carrying a rest transform, a render box size, and an opaque game-defined
//! tag. It is the reusable low-poly character representation any game can pose,
//! and it owns two things and nothing else:
//!
//! - **Portable figure data** — a [`FigureDefinition`] (a validated
//!   [`FigurePart`] list) that round-trips to bytes, so one authored figure is
//!   shared verbatim by a game and by an authoring/inspection tool.
//! - **Box posing** — pairing each part's render box with a world transform the
//!   app has already resolved, yielding [`PosedPart`]s a renderer draws.
//!
//! ## Engine owns mechanism; games own meaning
//! A "kicker", a "goalie", or any other character is authored as *data* (a
//! `FigureDefinition` plus an `axiom-animation` clip) — never wired in here.
//! There are no character names or gameplay assumptions in this module, and
//! `tag` is deliberately opaque.
//!
//! ## Isolated by design
//! This module depends only on [`axiom_kernel`] and [`axiom_math`]
//! (`allowed_modules = []`). The animation *clip* that drives a figure lives in
//! `axiom-animation`; an app composes the two — it builds the animation skeleton
//! from a figure's parts, samples/resolves a clip to world transforms, and hands
//! those to [`FigureApi::posed_parts`]. Figure never names an animation type;
//! it takes a plain `&[Transform]`.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade — [`FigureApi`] — plus its
//! value vocabulary ([`FigureDefinition`], [`FigurePart`], [`PosedPart`],
//! [`FigureError`], [`FigureResult`]).

mod definition;
mod figure_api;
mod figure_error;
mod ids;
mod part;
mod posed_part;

pub use figure_api::FigureApi;
pub use ids::{FigureDefinition, FigureError, FigurePart, FigureResult, PosedPart};
