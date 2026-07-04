//! Scalar and small-array interpolation curves shared across the growth gallery
//! app: plain linear interpolation and the two smoothstep forms used by the
//! vista painter (`vista.rs`), the visual-target diorama builder
//! (`visual_target/build.rs`), the scatter placer (`visual_target/scatter.rs`),
//! and the `growth_render_maps` example.
//!
//! These are infallible free functions over `f32` / `[f32; 3]`. `axiom-math`
//! only offers a fallible, handle-based `MathApi::lerp` (it validates its inputs
//! and returns a `MathResult`), which is a poor fit for the tight, always-finite
//! inner loops here and would change behavior (a panic/`Result` path on the hot
//! path). Consolidating the six copy-pasted definitions into this single
//! app-local home is the smaller, behavior-identical fix; promoting these plain
//! scalar helpers into `axiom-math` as *free* functions (so the whole engine can
//! share them) is a candidate follow-up to finding L2, out of scope for this
//! stream.

/// Linear interpolation `a + (b - a) * t`. `t` outside `[0, 1]` extrapolates,
/// by design.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Component-wise linear interpolation of two RGB / vector triples.
pub fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)]
}

/// Smooth Hermite interpolation between `edge0` and `edge1` (GLSL `smoothstep`),
/// clamped to `[0, 1]`. The edge-band width is floored at `1e-3` so a degenerate
/// zero-width band cannot divide by zero.
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1.0e-3)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Clamped smoothstep on the unit interval: `smoothstep(0, 1, t)`.
pub fn smoothstep01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
