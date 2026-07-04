//! # Axiom Animation Lab
//!
//! A data-driven authoring/inspection tool for the soccer kicker. It proves the
//! **mechanism vs. meaning** boundary and the **portable-data** boundary: the
//! reusable mechanism (skeletons, poses, clip sampling) lives in
//! `axiom-animation`; the reusable articulated box-figure lives in
//! `axiom-figure`; and all the *meaning* — the refined 13-part kicker rig and
//! its sagittal right-foot kick — lives here, authored as **bytes** ([`authoring`]).
//!
//! Those exact bytes are what the game embeds, so tuning the kick here and
//! re-emitting the assets keeps the lab and the game 1-1. The lab loads the
//! bytes back through the generic facades, poses the figure per frame, and
//! renders a side-view SVG scrubber.

pub mod authoring;
pub mod scene;
pub mod svg;
