//! # Axiom Animation Lab
//!
//! A demo app that proves the animation **mechanism vs. meaning** boundary. The
//! reusable mechanism — skeletons, poses, clip sampling, joint limits, events,
//! forward kinematics — lives in the `axiom-animation` engine module. All the
//! *meaning* lives here: an 18-bone humanoid, an authored right-foot soccer
//! kick, named kick phases, and a `KickContact` event. The app authors that
//! content entirely through the module's `AnimationApi` facade, then samples,
//! joint-limit-clamps, and forward-kinematics-solves the kick per frame and
//! renders it as SVG stick figures.

pub mod rig;
pub mod scene;
pub mod svg;
