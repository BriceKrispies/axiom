//! Metals. The single most important physical rule here: bare metal is
//! metalness 1, and every oxide/paint/dirt layer on top of it is metalness 0.
//! Blending metalness through the rust and chip masks is what makes these read
//! as real steel rather than as grey plastic.
//!
//! WGSL transcription of `Claude-of-Duty/src/materials/glsl/surfaces-metal.js`.

/// `RUST_HELPERS` (`surfaces-metal.js:9-21`).
/// Shared: layered iron oxide. Returns rust amount \[0,1\] and its colour.
pub const RUST_HELPERS: &str = include_str!("rust_helpers.wgsl");

/// `METAL_RUST` (`surfaces-metal.js:23-88`).
/// Steel with layered rust blooms, flaking plates, pitting and scratches.
pub const METAL_RUST: &str = include_str!("metal_rust.wgsl");

/// `METAL_PAINTED` (`surfaces-metal.js:90-178`).
/// Industrial paint over primer over rust over steel, with chipping and bleed.
pub const METAL_PAINTED: &str = include_str!("metal_painted.wgsl");

/// `METAL_BRUSHED` (`surfaces-metal.js:180-237`).
/// Brushed steel: X-aligned fibres, score lines, dents, smudges and grime.
pub const METAL_BRUSHED: &str = include_str!("metal_brushed.wgsl");

/// `CORRUGATED` (`surfaces-metal.js:239-323`).
/// Galvanised corrugated sheet: ridge profile, panel laps, spangle, rust,
/// perforations, hex fixings with washers and weeping rust streaks.
pub const CORRUGATED: &str = include_str!("corrugated.wgsl");
