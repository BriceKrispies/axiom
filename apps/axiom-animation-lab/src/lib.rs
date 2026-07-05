//! # Axiom Animation Lab
//!
//! A data-driven authoring/inspection tool for any articulated motion. It proves
//! the **mechanism vs. meaning** boundary and the **portable-data** boundary: the
//! reusable mechanism (skeletons, poses, clip sampling) lives in
//! `axiom-animation`; the reusable articulated box-figure lives in
//! `axiom-figure`; and all the *meaning* — the sample 13-part rig and its
//! sagittal limb-swing motion — lives here, authored as **bytes** ([`authoring`]).
//!
//! Because it is all portable bytes, the built-in sample is only a default: load
//! any other figure and clip bytes and the same pipeline scrubs them. The lab
//! reads the bytes back through the generic facades, poses the figure per frame,
//! and renders a side-view SVG scrubber.

pub mod authoring;
pub mod scene;
pub mod svg;
