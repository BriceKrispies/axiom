//! Float / distribution draws over a deterministic [`EntropyStream`].
//!
//! The engine's `axiom-entropy` layer produces integers plus a single
//! unit-interval draw ([`EntropyStream::unit`], a kernel `Ratio` in `[0, 1)`).
//! Worldgen additionally needs uniform ranges, a Gaussian, and uniform points on
//! the sphere. These are app-side conveniences — the growth app is a composition
//! leaf, exempt from the naked-`f32` and branchless spine laws — each composed
//! purely over `unit()`, so the same seed still reproduces the same world.
//!
//! The one genuinely reusable, *unitful* primitive — the area-preserving uniform
//! sphere point — lives in the engine as [`axiom_math::unit_vec3`]; this module
//! only adapts the two unit draws it consumes. The uniform-range and Box–Muller
//! conveniences stay app-side deliberately: they traffic in naked `f32`, so they
//! do not belong in a layer.

use axiom_entropy::EntropyStream;
use axiom_math::Vec3;

/// Uniform `f32` in `[min, max)`, one `unit()` draw.
pub fn range(stream: &mut EntropyStream, min: f32, max: f32) -> f32 {
    min + (max - min) * stream.unit().get()
}

/// A roughly standard-normal `f32` via Box–Muller over two `unit()` draws
/// (deterministic). The first draw is floored away from zero so `ln` stays finite.
pub fn normal(stream: &mut EntropyStream) -> f32 {
    let u1 = stream.unit().get().max(1.0e-7);
    let u2 = stream.unit().get();
    (-2.0 * u1.ln()).sqrt() * (core::f32::consts::TAU * u2).cos()
}

/// A uniformly-distributed unit vector on the sphere: draws the two uniforms the
/// engine's area-preserving [`axiom_math::unit_vec3`] sampler consumes.
pub fn unit_vec3(stream: &mut EntropyStream) -> Vec3 {
    let u = stream.unit();
    let v = stream.unit();
    axiom_math::unit_vec3(u, v)
}
