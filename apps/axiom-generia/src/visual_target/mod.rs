//! The deterministic diorama vocabulary generia's streaming forest is built on
//! (carried over from the gallery growth demo's Visual Target 001 pipeline).
//!
//! A fixed, versioned scene manifest ([`scene::Manifest`] — camera, sun, fog, a
//! terrain patch, ground materials, vegetation ranges) is turned into neutral
//! render data by [`build`]: unit meshes + materials, and the per-tree
//! trunk/foliage/branch/terrain instance builders (`build::*_instances`,
//! `build::terrain_window_mesh`) the wasm arm re-runs per streamed chunk. The
//! [`scatter`] placer seeds deterministic tree/tuft sites from the manifest's
//! ranges. Everything here is a pure function of the manifest TOML — no DOM, no
//! GPU handles — so it compiles and unit-tests on native.

pub mod build;
pub mod scatter;
pub mod scene;
