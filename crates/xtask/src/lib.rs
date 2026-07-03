//! Axiom repo-local tooling library.
//!
//! Exposes the architecture checker so both the `xtask` binary and the
//! integration tests (`tests/checker_tests.rs`) can drive it. This crate is
//! **not** an Axiom layer — it has no `layer.toml` and is excluded from the
//! layer chain.

pub mod app_manifest;
pub mod cargo_metadata;
pub mod check;
pub mod class_check;
pub mod classification;
pub mod coverage_scope;
pub mod game_manifest;
pub mod hygiene;
pub mod manifest;
pub mod module_manifest;
pub mod rust_source;
pub mod violation;
