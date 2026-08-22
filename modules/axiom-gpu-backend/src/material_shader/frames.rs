//! **The projection frames** — the per-axis bases every other layer is written
//! against.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js`, `PARS_FRAGMENT`:
//! `owTangentFrame`, `struct OwFrame`, `owAxisFrame` and `owOrthonormalise`
//! (source lines 171-208). Triplanar sampling projects on these; the
//! tangent-space normal path resolves its `nT` through them; POM marches in the
//! frame's tangent space; de-tiling, the detail layer and the normal blend are
//! all expressed in `T`/`B`/`N`. This is the layer the siblings compose against,
//! so it is transcribed at the level of the individual cross product.
//!
//! ## What is here
//!
//! * [`FRAMES_WGSL`] — the shader text, as the source writes it.
//! * The CPU reference: [`OwFrame`], [`OwTangentBasis`], [`ow_axis_frame`],
//!   [`ow_orthonormalise`], [`ow_tangent_frame`].
//! * A CPU↔GPU parity test on a real adapter (the `parity` submodule, compiled
//!   only under `--features offscreen`).
//!
//! ## Handedness — the thing that is silently wrong if it is wrong
//!
//! A basis built with the cross-product operands swapped is still orthonormal,
//! still compiles, and mirrors every normal map. Both cross products in this
//! layer are asymmetric on purpose:
//!
//! * `owTangentFrame` computes `cross(q1, n)` and `cross(n, q0)` — the operands
//!   are in the *opposite* order in the two lines, which is the whole of the
//!   Mikkelsen construction.
//! * `owOrthonormalise` computes `B = cross(N, T)`, not `cross(T, N)`.
//!
//! The three static axis bases are right-handed in the same sense
//! (`cross(T, B) == N` for every axis and every sign of the normal), and
//! `the_axis_bases_are_right_handed_for_every_axis_and_sign` asserts exactly
//! that — a swapped operand pair in any arm flips one of those six triples.
//!
//! ## The orthonormalisation order
//!
//! `owOrthonormalise` takes `inout OwFrame` and mutates in this order:
//!
//! ```glsl
//! f.N = n;
//! f.T = normalize( f.T - n * dot( n, f.T ) );
//! f.B = cross( n, f.T );
//! ```
//!
//! `B` is built from the **projected, renormalised** `T`, not from the axis
//! frame's original `T`. Reordering the two lines is a different frame (it
//! leaves `B` un-perpendicular to the new `T` whenever `n` is off-axis), so
//! [`ow_orthonormalise`] returns a value whose `b` field is initialised from the
//! new `t`, and `orthonormalising_uses_the_projected_tangent_not_the_original`
//! pins it. Writing `f.N` first is inert — nothing downstream of it in this
//! function reads `f.N` — but it is transcribed anyway, because dead computation
//! in the source is still part of the source.
//!
//! ## `sign` is not what the source uses, and that matters
//!
//! The per-axis sign is `mix( vec3(-1.0), vec3(1.0), step( 0.0, n ) )`, **not**
//! `sign(n)`. They differ at zero: GLSL `sign(0.0)` is `0.0`, which would
//! collapse the whole basis to the zero vector on a face whose normal has an
//! exactly-zero component (every axis-aligned box face has two). `step(0.0, 0.0)`
//! is `1.0`, so a zero component selects the **positive** axis. Rust's
//! `f32::signum` is a third thing again (`+1.0` at `+0.0`, `-1.0` at `-0.0`), so
//! neither builtin is used here: [`gl_step`] is written out.
//!
//! ## `dpdx`/`dpdy` are fragment-only, so they are parameters
//!
//! `owTangentFrame` reads `dFdx(eye)`, `dFdy(eye)`, `dFdx(uv)`, `dFdy(uv)`. A
//! screen-space derivative has no CPU equivalent — there is no neighbouring
//! pixel — so [`ow_tangent_frame`] takes the four derivatives as explicit
//! arguments and the WGSL is split the same way: [`FRAMES_WGSL`] defines
//! `owTangentFrame`, which takes them, and `owTangentFrameScreen`, the
//! fragment-only wrapper with the source's own three-argument signature that
//! supplies them from `dpdx`/`dpdy`. This is the shape `apps/shmup`'s
//! `sky::dome::fwidth` already established: a GLSL *implicit* input becomes an
//! explicit parameter, because a CPU port has no implicit binding to resolve it
//! through.
//!
//! That is not a dead end. The parity test drives `owTangentFrameScreen` over a
//! probe whose `eye` and `uv` are linear in `position.xy` with **dyadic**
//! coefficients, so `f(x+1) - f(x)` is exact in `f32` and the hardware's
//! derivative is a known constant — which the CPU side is then fed. The wrapper
//! is pinned end to end rather than at an invented value, and a swapped
//! `dpdx`/`dpdy` fails it.
//!
//! ## `owTile` is a parameter, not a uniform
//!
//! The source reads the `owTile` uniform (`xy` = scale, `zw` = offset) directly
//! inside `owAxisFrame`. Per the layer calling convention it is an explicit
//! argument here, on both sides. The composer supplies it from the packed
//! parameter block.

use axiom_math::{Vec2, Vec3, Vec4};

/// The projection frames, as WGSL.
///
/// Four items, and the two the source does not have are called out:
///
/// | WGSL | source |
/// |---|---|
/// | `struct OwFrame { uv, T, B, N }` | verbatim |
/// | `fn owAxisFrame(p, n, axis, owTile) -> OwFrame` | `owTile` was a uniform |
/// | `fn owOrthonormalise(f: ptr<function, OwFrame>, n)` | GLSL `inout` |
/// | `fn owTangentFrame(dEyeDx, dEyeDy, dUvDx, dUvDy, n) -> mat3x3<f32>` | derivatives were implicit |
/// | `fn owTangentFrameScreen(eye, n, uv) -> mat3x3<f32>` | the source's own signature |
///
/// `owTangentFrameScreen` calls `dpdx`/`dpdy` and is therefore **fragment-stage
/// only**; everything else in this constant is stage-agnostic.
pub(crate) const FRAMES_WGSL: &str = r#"
// ---------------------------------------------------------------------------
// materials/shader.js, PARS_FRAGMENT: the projection frames.
// ---------------------------------------------------------------------------

struct OwFrame {
    uv: vec2<f32>,
    T: vec3<f32>,
    B: vec3<f32>,
    N: vec3<f32>,
};

// Mikkelsen's screen-space tangent frame. The two cross products take their
// operands in OPPOSITE orders; swapping either pair mirrors every normal map.
// `det == 0.0` (a degenerate uv patch) yields a zero T and B rather than a NaN.
fn owTangentFrame(
    dEyeDx: vec3<f32>,
    dEyeDy: vec3<f32>,
    dUvDx: vec2<f32>,
    dUvDy: vec2<f32>,
    n: vec3<f32>,
) -> mat3x3<f32> {
    let q0 = dEyeDx;
    let q1 = dEyeDy;
    let s0 = dUvDx;
    let s1 = dUvDy;
    let q1p = cross(q1, n);
    let q0p = cross(n, q0);
    let T = q1p * s0.x + q0p * s1.x;
    let B = q1p * s0.y + q0p * s1.y;
    let det = max(dot(T, T), dot(B, B));
    let sc = select(inverseSqrt(det), 0.0, det == 0.0);
    return mat3x3<f32>(T * sc, B * sc, n);
}

// The source's own three-argument signature. FRAGMENT STAGE ONLY: `dpdx`/`dpdy`
// exist nowhere else, which is why the arithmetic above takes the four
// derivatives as parameters and this is a wrapper over it.
fn owTangentFrameScreen(eye: vec3<f32>, n: vec3<f32>, uv: vec2<f32>) -> mat3x3<f32> {
    return owTangentFrame(dpdx(eye), dpdy(eye), dpdx(uv), dpdy(uv), n);
}

// The per-axis world/object projection basis. `s` is `step`, not `sign`: a
// normal component of exactly 0.0 selects the POSITIVE axis, where `sign` would
// zero the basis. `owTile.xy` is the scale, `owTile.zw` the offset.
fn owAxisFrame(p: vec3<f32>, n: vec3<f32>, axis: i32, owTile: vec4<f32>) -> OwFrame {
    let s = mix(vec3<f32>(-1.0), vec3<f32>(1.0), step(vec3<f32>(0.0), n));
    var f: OwFrame;
    if (axis == 0) {
        f.uv = vec2<f32>(-p.z * s.x, p.y);
        f.T = vec3<f32>(0.0, 0.0, -s.x); f.B = vec3<f32>(0.0, 1.0, 0.0); f.N = vec3<f32>(s.x, 0.0, 0.0);
    } else if (axis == 1) {
        f.uv = vec2<f32>(p.x, -p.z * s.y);
        f.T = vec3<f32>(1.0, 0.0, 0.0); f.B = vec3<f32>(0.0, 0.0, -s.y); f.N = vec3<f32>(0.0, s.y, 0.0);
    } else {
        f.uv = vec2<f32>(p.x * s.z, p.y);
        f.T = vec3<f32>(s.z, 0.0, 0.0); f.B = vec3<f32>(0.0, 1.0, 0.0); f.N = vec3<f32>(0.0, 0.0, s.z);
    }
    f.uv = f.uv * owTile.xy + owTile.zw;
    return f;
}

// Re-anchor an axis frame onto the true interpolated normal. GLSL `inout`, so a
// pointer here; the caller holds `var f: OwFrame` and passes `&f`. THE ORDER IS
// THE ALGORITHM: B is built from the projected, renormalised T.
fn owOrthonormalise(f: ptr<function, OwFrame>, n: vec3<f32>) {
    (*f).N = n;
    (*f).T = normalize((*f).T - n * dot(n, (*f).T));
    (*f).B = cross(n, (*f).T);
}
"#;

/// GLSL `step(edge, x)` — `x < edge ? 0.0 : 1.0`.
///
/// Written out rather than reached for, because the two plausible Rust stand-ins
/// are both wrong: `f32::signum` returns `-1.0` at `-0.0` and never `0.0`, and
/// GLSL's own `sign` returns `0.0` at zero. `step(0.0, x)` returns `1.0` at both
/// zeroes, which is what makes an axis-aligned face pick the positive axis.
fn gl_step(edge: f32, x: f32) -> f32 {
    [1.0_f32, 0.0][usize::from(x < edge)]
}

/// GLSL `mix(x, y, a)` — `x * (1 - a) + y * a`, in that grouping.
fn gl_mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// GLSL `normalize(v)` — `v / length(v)`, a component-wise **division**.
///
/// Deliberately not `v * (1 / length(v))`: float arithmetic is not associative
/// and the reciprocal-multiply is a different value. `axiom_math::Vec3::normalize`
/// is not used either — it rejects the zero vector with a `MathError`, where GLSL
/// propagates a NaN, and this is a transcription of GLSL.
fn gl_normalize(v: Vec3) -> Vec3 {
    let len = v.length();
    Vec3::new(v.x / len, v.y / len, v.z / len)
}

/// GLSL `inversesqrt(x)` — `1 / sqrt(x)`.
fn gl_inverse_sqrt(x: f32) -> f32 {
    1.0 / x.sqrt()
}

/// The per-axis sign vector: `mix( vec3(-1.0), vec3(1.0), step( 0.0, n ) )`.
fn axis_signs(n: Vec3) -> Vec3 {
    Vec3::new(
        gl_mix(-1.0, 1.0, gl_step(0.0, n.x)),
        gl_mix(-1.0, 1.0, gl_step(0.0, n.y)),
        gl_mix(-1.0, 1.0, gl_step(0.0, n.z)),
    )
}

/// `struct OwFrame { vec2 uv; vec3 T; vec3 B; vec3 N; }` — a projected uv and the
/// orthonormal basis that uv is measured in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OwFrame {
    /// The projected, tiled surface parameterisation.
    pub(crate) uv: Vec2,
    /// Tangent — the `+u` direction in world/object space.
    pub(crate) t: Vec3,
    /// Bitangent — the `+v` direction.
    pub(crate) b: Vec3,
    /// Normal — the projection axis, signed by the surface normal.
    pub(crate) n: Vec3,
}

/// What GLSL's `mat3( T * sc, B * sc, n )` is: three **columns**, which the
/// source's one call site immediately unpacks as `tbnV[0]`, `tbnV[1]`, `tbnV[2]`
/// into an [`OwFrame`]'s `T`, `B` and `N`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OwTangentBasis {
    /// Column 0 — `T * sc`.
    pub(crate) t: Vec3,
    /// Column 1 — `B * sc`.
    pub(crate) b: Vec3,
    /// Column 2 — `n`, passed through unscaled.
    pub(crate) n: Vec3,
}

/// `owAxisFrame( p, n, axis )` — the projection basis for one world/object axis.
///
/// `axis` is GLSL's `int`: `0` and `1` select the X and Y arms and **everything
/// else** — including a negative value — falls into the Z arm, exactly as the
/// source's `if / else if / else` chain does.
pub(crate) fn ow_axis_frame(p: Vec3, n: Vec3, axis: i32, ow_tile: Vec4) -> OwFrame {
    let s = axis_signs(n);
    // The `else if` chain as an index: 0 -> 0, 1 -> 1, anything else -> 2.
    let arm = usize::from(axis != 0) * (1 + usize::from(axis != 1));
    let uv = [
        Vec2::new(-p.z * s.x, p.y),
        Vec2::new(p.x, -p.z * s.y),
        Vec2::new(p.x * s.z, p.y),
    ][arm];
    OwFrame {
        // `f.uv = f.uv * owTile.xy + owTile.zw`, component-wise.
        uv: Vec2::new(uv.x * ow_tile.x + ow_tile.z, uv.y * ow_tile.y + ow_tile.w),
        t: [
            Vec3::new(0.0, 0.0, -s.x),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(s.z, 0.0, 0.0),
        ][arm],
        b: [
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, -s.y),
            Vec3::new(0.0, 1.0, 0.0),
        ][arm],
        n: [
            Vec3::new(s.x, 0.0, 0.0),
            Vec3::new(0.0, s.y, 0.0),
            Vec3::new(0.0, 0.0, s.z),
        ][arm],
    }
}

/// `owOrthonormalise( inout OwFrame f, vec3 n )` — re-anchor an axis frame onto
/// the true interpolated normal.
///
/// The GLSL mutates in place; this returns the mutated value, which is the same
/// thing said without a `&mut`. **The order is the algorithm**: `b` is
/// `cross(n, t)` over the *projected, renormalised* `t`, so the returned `b`
/// below is initialised from the local `t` and never from `f.t`.
pub(crate) fn ow_orthonormalise(f: OwFrame, n: Vec3) -> OwFrame {
    // `f.N = n;` first in the source. Inert — nothing after it reads `f.N` — but
    // transcribed: it is why the returned `n` is the argument, not `f.n`.
    let t = gl_normalize(f.t.subtract(n.mul_scalar(n.dot(f.t))));
    OwFrame {
        uv: f.uv,
        t,
        b: n.cross(t),
        n,
    }
}

/// `owTangentFrame( eye, n, uv )` — Mikkelsen's screen-space tangent frame, with
/// the four screen-space derivatives the source takes implicitly supplied
/// explicitly.
///
/// `d_eye_dx` is `dFdx(eye)`, `d_eye_dy` is `dFdy(eye)`, `d_uv_dx` is
/// `dFdx(uv)`, `d_uv_dy` is `dFdy(uv)`. See this module's header for why they
/// are parameters and how the WGSL wrapper that does call `dpdx`/`dpdy` is still
/// pinned by the parity test.
pub(crate) fn ow_tangent_frame(
    d_eye_dx: Vec3,
    d_eye_dy: Vec3,
    d_uv_dx: Vec2,
    d_uv_dy: Vec2,
    n: Vec3,
) -> OwTangentBasis {
    let q0 = d_eye_dx;
    let q1 = d_eye_dy;
    let s0 = d_uv_dx;
    let s1 = d_uv_dy;
    // `cross( q1, n )` then `cross( n, q0 )` — the operand order is opposite in
    // the two lines and is not a typo in the source.
    let q1p = q1.cross(n);
    let q0p = n.cross(q0);
    let t = q1p.mul_scalar(s0.x).add(q0p.mul_scalar(s1.x));
    let b = q1p.mul_scalar(s0.y).add(q0p.mul_scalar(s1.y));
    let det = t.dot(t).max(b.dot(b));
    // `( det == 0.0 ) ? 0.0 : inversesqrt( det )`. The unused arm's `1/sqrt(0)`
    // is an infinity that is selected away before it can reach a multiply.
    let sc = [gl_inverse_sqrt(det), 0.0][usize::from(det == 0.0)];
    OwTangentBasis {
        t: t.mul_scalar(sc),
        b: b.mul_scalar(sc),
        n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame's basis is right-handed exactly when `cross(T, B) == N`.
    fn handedness(t: Vec3, b: Vec3, n: Vec3) -> f32 {
        t.cross(b).dot(n)
    }

    #[test]
    fn gl_step_returns_one_at_zero_where_sign_would_return_zero() {
        assert_eq!(gl_step(0.0, 0.0), 1.0);
        assert_eq!(gl_step(0.0, -0.0), 1.0);
        assert_eq!(gl_step(0.0, 1.5), 1.0);
        assert_eq!(gl_step(0.0, -1.5), 0.0);
        assert_eq!(gl_step(2.0, 1.5), 0.0);
    }

    #[test]
    fn gl_mix_is_the_glsl_expansion() {
        assert_eq!(gl_mix(-1.0, 1.0, 0.0), -1.0);
        assert_eq!(gl_mix(-1.0, 1.0, 1.0), 1.0);
        assert_eq!(gl_mix(2.0, 6.0, 0.25), 3.0);
    }

    #[test]
    fn gl_normalize_divides_and_propagates_nan_for_the_zero_vector() {
        let unit = gl_normalize(Vec3::new(0.0, 3.0, 4.0));
        assert_eq!(unit, Vec3::new(0.0, 0.6, 0.8));
        let degenerate = gl_normalize(Vec3::ZERO);
        assert!(degenerate.x.is_nan() & degenerate.y.is_nan() & degenerate.z.is_nan());
    }

    #[test]
    fn gl_inverse_sqrt_is_one_over_the_root() {
        assert_eq!(gl_inverse_sqrt(4.0), 0.5);
        assert_eq!(gl_inverse_sqrt(0.0), f32::INFINITY);
    }

    #[test]
    fn a_zero_normal_component_selects_the_positive_axis() {
        // Every axis-aligned box face has two exactly-zero normal components.
        // `sign` would zero the basis there; `step` must not.
        assert_eq!(axis_signs(Vec3::new(0.0, -0.0, -2.0)), Vec3::new(1.0, 1.0, -1.0));
        assert_eq!(axis_signs(Vec3::new(-1.0, 1.0, 0.5)), Vec3::new(-1.0, 1.0, 1.0));
    }

    #[test]
    fn each_axis_arm_matches_the_source_basis_and_uv() {
        let p = Vec3::new(2.0, -3.0, 5.0);
        let n = Vec3::new(1.0, -1.0, 1.0);
        let tile = Vec4::new(1.0, 1.0, 0.0, 0.0);
        let x = ow_axis_frame(p, n, 0, tile);
        assert_eq!(x.uv, Vec2::new(-5.0, -3.0));
        assert_eq!(x.t, Vec3::new(0.0, 0.0, -1.0));
        assert_eq!(x.b, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(x.n, Vec3::new(1.0, 0.0, 0.0));
        let y = ow_axis_frame(p, n, 1, tile);
        assert_eq!(y.uv, Vec2::new(2.0, 5.0));
        assert_eq!(y.t, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(y.b, Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(y.n, Vec3::new(0.0, -1.0, 0.0));
        let z = ow_axis_frame(p, n, 2, tile);
        assert_eq!(z.uv, Vec2::new(2.0, -3.0));
        assert_eq!(z.t, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(z.b, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(z.n, Vec3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn any_axis_that_is_not_zero_or_one_takes_the_z_arm() {
        let p = Vec3::new(2.0, -3.0, 5.0);
        let n = Vec3::new(1.0, -1.0, -1.0);
        let tile = Vec4::new(1.0, 1.0, 0.0, 0.0);
        let z = ow_axis_frame(p, n, 2, tile);
        // The source's chain is `if 0 / else if 1 / else`, so 7 and -1 are Z too.
        assert_eq!(ow_axis_frame(p, n, 7, tile), z);
        assert_eq!(ow_axis_frame(p, n, -1, tile), z);
        assert_eq!(ow_axis_frame(p, n, i32::MIN, tile), z);
    }

    #[test]
    fn the_tile_scale_and_offset_are_applied_after_projection() {
        let frame = ow_axis_frame(
            Vec3::new(2.0, -3.0, 5.0),
            Vec3::ONE,
            2,
            Vec4::new(4.0, 0.5, -0.25, 10.0),
        );
        // uv = (p.x * s.z, p.y) = (2, -3), then * (4, 0.5) + (-0.25, 10).
        assert_eq!(frame.uv, Vec2::new(7.75, 8.5));
        // The basis is untouched by tiling.
        assert_eq!(frame.t, Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn the_axis_bases_are_right_handed_for_every_axis_and_sign() {
        // Six triples: three axes x two normal signs. A swapped cross-product
        // operand pair in any arm flips one of them, which is the defect that
        // mirrors every normal map while still compiling.
        [Vec3::new(1.0, 1.0, 1.0), Vec3::new(-1.0, -1.0, -1.0)]
            .iter()
            .for_each(|n| {
                (0..3).for_each(|axis| {
                    let f = ow_axis_frame(Vec3::ONE, *n, axis, Vec4::new(1.0, 1.0, 0.0, 0.0));
                    assert_eq!(
                        handedness(f.t, f.b, f.n),
                        1.0,
                        "axis {axis} with normal {n:?} must be right-handed"
                    );
                });
            });
    }

    #[test]
    fn orthonormalising_uses_the_projected_tangent_not_the_original() {
        let frame = ow_axis_frame(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(0.6, 0.0, 0.8),
            2,
            Vec4::new(1.0, 1.0, 0.0, 0.0),
        );
        // The axis frame's T is (1, 0, 0); the true normal is not the axis, so
        // the projection genuinely moves it.
        assert_eq!(frame.t, Vec3::new(1.0, 0.0, 0.0));
        let n = Vec3::new(0.6, 0.0, 0.8);
        let re = ow_orthonormalise(frame, n);
        assert_eq!(re.uv, frame.uv, "orthonormalising leaves uv alone");
        assert_eq!(re.n, n, "f.N = n");
        // T is the original T with the normal projected out, renormalised.
        assert!((re.t.subtract(Vec3::new(0.8, 0.0, -0.6))).length() < 1.0e-6);
        // B = cross(n, NEW t). Built from the original T it would be
        // cross((.6,0,.8),(1,0,0)) = (0, .8, 0) — a different, shorter vector.
        assert!((re.b.subtract(Vec3::new(0.0, 1.0, 0.0))).length() < 1.0e-6);
        assert!(re.b.length() > 0.99, "B is unit only if T was projected first");
        assert!((handedness(re.t, re.b, re.n) - 1.0).abs() < 1.0e-6);
        // And the frame really is orthogonal to the interpolated normal.
        assert!(re.t.dot(n).abs() < 1.0e-6);
        assert!(re.b.dot(n).abs() < 1.0e-6);
    }

    #[test]
    fn orthonormalising_an_already_aligned_frame_is_the_identity_on_the_basis() {
        let n = Vec3::new(0.0, 1.0, 0.0);
        let frame = ow_axis_frame(Vec3::ONE, n, 1, Vec4::new(1.0, 1.0, 0.0, 0.0));
        let re = ow_orthonormalise(frame, n);
        assert_eq!(re.t, frame.t);
        assert_eq!(re.b, frame.b);
        assert_eq!(re.n, n);
    }

    #[test]
    fn the_tangent_frame_recovers_the_uv_directions_of_a_planar_patch() {
        // A flat patch in the XZ plane seen with n = +y: u runs along +x and v
        // along -z, which makes (u, v, n) a RIGHT-handed triple.
        let n = Vec3::new(0.0, 1.0, 0.0);
        let basis = ow_tangent_frame(
            Vec3::new(0.5, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -0.25),
            Vec2::new(0.5, 0.0),
            Vec2::new(0.0, 0.25),
            n,
        );
        // T is the +u direction and B the +v direction, both unit here.
        assert!(basis.t.subtract(Vec3::new(1.0, 0.0, 0.0)).length() < 1.0e-6);
        assert!(basis.b.subtract(Vec3::new(0.0, 0.0, -1.0)).length() < 1.0e-6);
        assert_eq!(basis.n, n);
        assert!(basis.t.dot(basis.b).abs() < 1.0e-6);
        assert!((handedness(basis.t, basis.b, basis.n) - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn the_tangent_frames_handedness_follows_the_uv_parameterisation() {
        // Not a fixed sign, and worth saying out loud: this frame reproduces
        // whatever handedness the mesh's uv winding has. The same patch as
        // above with v running along +z instead is a LEFT-handed triple, and
        // the frame reports it as one. A normal map authored for the opposite
        // winding is the mesh's problem, not the frame's — which is exactly why
        // the operand order of the two cross products may not be "tidied".
        let n = Vec3::new(0.0, 1.0, 0.0);
        let flipped = ow_tangent_frame(
            Vec3::new(0.5, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 0.25),
            Vec2::new(0.5, 0.0),
            Vec2::new(0.0, 0.25),
            n,
        );
        assert!((handedness(flipped.t, flipped.b, flipped.n) + 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn the_tangent_frame_scales_both_columns_by_the_same_reciprocal_root() {
        // Anisotropic uv: T and B have different lengths and only the LONGER of
        // the two is normalised, because `det` is a max over both.
        let basis = ow_tangent_frame(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(0.0, 0.5),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let longer = basis.t.length().max(basis.b.length());
        assert!((longer - 1.0).abs() < 1.0e-6);
        let ratio = basis.t.length() / basis.b.length();
        assert!((ratio - 4.0).abs() < 1.0e-5, "ratio was {ratio}");
    }

    #[test]
    fn a_degenerate_uv_patch_yields_a_zero_scale_and_never_a_nan() {
        // Zero uv derivatives: T and B are both zero, det is zero, and the
        // source's ternary must select 0.0 rather than `inversesqrt(0)`.
        let n = Vec3::new(0.0, 0.0, 1.0);
        let basis = ow_tangent_frame(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 0.0),
            n,
        );
        assert_eq!(basis.t, Vec3::ZERO);
        assert_eq!(basis.b, Vec3::ZERO);
        assert_eq!(basis.n, n);
    }

    #[test]
    fn the_wgsl_names_the_four_entry_points_siblings_compose_against() {
        // The composer splices this text; these are the signatures reported to
        // the other layers, so a rename has to break a test.
        assert!(FRAMES_WGSL.contains("struct OwFrame {"));
        assert!(FRAMES_WGSL.contains("fn owAxisFrame(p: vec3<f32>, n: vec3<f32>, axis: i32, owTile: vec4<f32>) -> OwFrame {"));
        assert!(FRAMES_WGSL.contains("fn owOrthonormalise(f: ptr<function, OwFrame>, n: vec3<f32>) {"));
        assert!(FRAMES_WGSL.contains("fn owTangentFrame(\n"));
        assert!(FRAMES_WGSL
            .contains("fn owTangentFrameScreen(eye: vec3<f32>, n: vec3<f32>, uv: vec2<f32>) -> mat3x3<f32> {"));
        // The two asymmetric cross products, verbatim. Transcription is the risk
        // in this port, so the operand order is asserted on the text itself.
        assert!(FRAMES_WGSL.contains("let q1p = cross(q1, n);"));
        assert!(FRAMES_WGSL.contains("let q0p = cross(n, q0);"));
        assert!(FRAMES_WGSL.contains("(*f).B = cross(n, (*f).T);"));
        // `step`, never `sign`.
        assert!(FRAMES_WGSL.contains("step(vec3<f32>(0.0), n)"));
        assert!(!FRAMES_WGSL.contains("sign("));
    }
}

/// **CPU↔GPU parity for the projection frames**, on a real adapter.
///
/// The pattern is `crate::surface_program::parity`'s: acquire an adapter and
/// **assert** one was found rather than skipping, render one fragment per sample
/// into an `Rgba32Float` target, read the lanes back, compare against the CPU
/// reference above. The harness is local because `surface_program::parity`'s is
/// `pub(super)` to that module; once every material-shader layer has landed, one
/// shared harness under `material_shader/` is the right de-duplication.
///
/// Four entry points, one per thing this layer defines: `owAxisFrame`,
/// `owAxisFrame` + `owOrthonormalise`, `owTangentFrame` with supplied
/// derivatives, and `owTangentFrameScreen` with real `dpdx`/`dpdy`.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::*;

    /// Evaluation contexts per run, and the target's width.
    const SAMPLES: usize = 24;
    /// Target height. Rows 0..=2 carry the three output quadruples; row 3 exists
    /// so every read row sits in a complete 2x2 derivative quad.
    const ROWS: usize = 4;
    /// `copy_texture_to_buffer` row alignment.
    const ROW_ALIGN: u32 = 256;
    /// Uniform slots: four `vec4` per sample.
    const SLOTS: usize = SAMPLES * 4;

    /// **The tolerance, in two measured parts.** One absolute number would be
    /// dishonest here: the lanes range from an exactly-`±1.0` basis component to
    /// a tiled uv in the tens, and a budget wide enough for the second is
    /// hundreds of ULPs of the first.
    ///
    /// `TOLERANCE_ABS` is the floor, and covers the basis lanes: the only
    /// divergence there is `normalize`, which the hardware evaluates as an
    /// `rsqrt`-and-multiply where the CPU reference divides.
    ///
    /// `TOLERANCE_REL` is one `f32` ULP — `2^-23 = 1.19e-7` — because a GPU may
    /// contract `uv * owTile.xy + owTile.zw` into a single-rounding `fma` where
    /// the CPU rounds the multiply and the add separately. That is the hardware's
    /// licence, not a transcription difference.
    ///
    /// **Measured** (Vulkan, and printed by `report` under `--nocapture`, so the
    /// numbers are reproducible rather than remembered): the worst lane in the
    /// whole sweep is `4.77e-7` on a uv of magnitude ~5.5 — *exactly* one ULP at
    /// that exponent — and `1.19e-7` on the tangent frames, again exactly one
    /// ULP. Every entry point uses between `0.63` and `0.68` of its budget, so
    /// the tolerance is roughly 1.5x what this hardware needs: enough headroom
    /// for another adapter's rounding, nowhere near the 10x that would make it
    /// a rubber stamp.
    const TOLERANCE_ABS: f32 = 1.0e-7;
    /// One `f32` ULP. See [`TOLERANCE_ABS`].
    const TOLERANCE_REL: f32 = 1.2e-7;

    /// The harness: a fullscreen triangle, one uniform, and a `SAMPLES x ROWS`
    /// `Rgba32Float` target whose pixel column names the sample and whose row
    /// names which quadruple of the frame to emit.
    const HARNESS_WGSL: &str = r#"
struct FramesProbe { items: array<vec4<f32>, 96> };
@group(0) @binding(0) var<uniform> probe: FramesProbe;

@vertex
fn frames_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

fn frames_pack(f: OwFrame, row: u32) -> vec4<f32> {
    var packed = array<vec4<f32>, 3>(
        vec4<f32>(f.uv.x, f.uv.y, f.T.x, f.T.y),
        vec4<f32>(f.T.z, f.B.x, f.B.y, f.B.z),
        vec4<f32>(f.N.x, f.N.y, f.N.z, 0.0),
    );
    return packed[min(row, 2u)];
}

fn frames_pack_basis(m: mat3x3<f32>, row: u32) -> vec4<f32> {
    var packed = array<vec4<f32>, 3>(
        vec4<f32>(m[0], 0.0),
        vec4<f32>(m[1], 0.0),
        vec4<f32>(m[2], 0.0),
    );
    return packed[min(row, 2u)];
}

@fragment
fn frames_axis_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = probe.items[i * 4u + 0u];
    let b = probe.items[i * 4u + 1u];
    let tile = probe.items[i * 4u + 2u];
    let f = owAxisFrame(a.xyz, b.xyz, i32(a.w), tile);
    return frames_pack(f, u32(position.y));
}

@fragment
fn frames_ortho_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = probe.items[i * 4u + 0u];
    let b = probe.items[i * 4u + 1u];
    let tile = probe.items[i * 4u + 2u];
    var f = owAxisFrame(a.xyz, b.xyz, i32(a.w), tile);
    owOrthonormalise(&f, b.xyz);
    return frames_pack(f, u32(position.y));
}

@fragment
fn frames_tangent_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = probe.items[i * 4u + 0u];
    let b = probe.items[i * 4u + 1u];
    let c = probe.items[i * 4u + 2u];
    let d = probe.items[i * 4u + 3u];
    return frames_pack_basis(owTangentFrame(a.xyz, b.xyz, c.xy, c.zw, d.xyz), u32(position.y));
}

// `eye` and `uv` linear in the pixel centre with DYADIC coefficients, so
// f(x+1) - f(x) is exact in f32 and the hardware's derivative is a known
// constant whatever quad it picks and whether it is coarse or fine.
fn frames_probe_eye(pos: vec2<f32>) -> vec3<f32> {
    return vec3<f32>(
        pos.x * 0.25 + pos.y * -0.5,
        pos.x * 0.125 + pos.y * 0.0625,
        pos.x * -0.5 + pos.y * 0.25,
    );
}

fn frames_probe_uv(pos: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        pos.x * 0.03125 + pos.y * 0.015625,
        pos.x * -0.0078125 + pos.y * 0.0625,
    );
}

@fragment
fn frames_screen_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let d = probe.items[i * 4u + 3u];
    // Derivatives are taken in UNIFORM control flow, before the row select.
    let m = owTangentFrameScreen(frames_probe_eye(position.xy), d.xyz, frames_probe_uv(position.xy));
    return frames_pack_basis(m, u32(position.y));
}
"#;

    /// The exact screen-space derivatives `frames_probe_eye`/`frames_probe_uv`
    /// have, by construction. Every coefficient is a negative power of two and
    /// every product is a small exact multiple of one, so the hardware's
    /// neighbouring-pixel difference **is** the coefficient, bit for bit.
    const PROBE_D_EYE_DX: Vec3 = Vec3::new(0.25, 0.125, -0.5);
    const PROBE_D_EYE_DY: Vec3 = Vec3::new(-0.5, 0.0625, 0.25);
    const PROBE_D_UV_DX: Vec2 = Vec2::new(0.03125, -0.0078125);
    const PROBE_D_UV_DY: Vec2 = Vec2::new(0.015625, 0.0625);

    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl Gpu {
        fn acquire() -> Gpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
            let backend = gpu.backend;
            assert_ne!(
                backend,
                wgpu::Backend::Noop,
                "the parity proof is worthless unless a real backend ran it"
            );
            Gpu {
                device,
                queue,
                backend,
            }
        }

        fn module(&self) -> wgpu::ShaderModule {
            // The error scope is the SHARED device's, so it is entered exclusively;
            // see `crate::test_gpu::validating`.
            let (module, failure) = crate::test_gpu::validating(&self.device, || {
                self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("axiom-material-frames"),
                        source: wgpu::ShaderSource::Wgsl([FRAMES_WGSL, HARNESS_WGSL].concat().into()),
                    })
            });
            assert!(
                failure.is_none(),
                "FRAMES_WGSL must compile on {:?}: {failure:?}",
                self.backend
            );
            module
        }

        /// Render `entry_point` and return every pixel, row-major
        /// (`row * SAMPLES + column`).
        fn render(
            &self,
            module: &wgpu::ShaderModule,
            entry_point: &str,
            uniform: &[u8],
        ) -> Vec<[f32; 4]> {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-material-frames-bgl"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                });
            let buffer = wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-material-frames-uniform"),
                    contents: uniform,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-material-frames-bg"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-material-frames-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-material-frames-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("frames_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry_point),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba32Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
            let size = wgpu::Extent3d {
                width: SAMPLES as u32,
                height: ROWS as u32,
                depth_or_array_layers: 1,
            };
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-material-frames-target"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-material-frames-readback"),
                size: u64::from(row_bytes) * ROWS as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-material-frames-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(ROWS as u32),
                    },
                },
                size,
            );
            self.queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            self.device
                .poll(wgpu::PollType::Wait)
                .expect("the readback must complete");
            let mapped = slice.get_mapped_range();
            (0..ROWS * SAMPLES)
                .map(|pixel| {
                    let base = (pixel / SAMPLES) * row_bytes as usize + (pixel % SAMPLES) * 16;
                    [0_usize, 1, 2, 3].map(|lane| {
                        let at = base + lane * 4;
                        f32::from_le_bytes([
                            mapped[at],
                            mapped[at + 1],
                            mapped[at + 2],
                            mapped[at + 3],
                        ])
                    })
                })
                .collect()
        }
    }

    /// One `owAxisFrame` input set.
    struct AxisProbe {
        p: Vec3,
        n: Vec3,
        axis: i32,
        tile: Vec4,
    }

    /// [`SAMPLES`] axis probes, sweeping every arm of the chain, both normal
    /// signs, **exactly-zero** normal components (where `sign` and `step` part
    /// company), zero and negative positions, and non-trivial tiling.
    fn axis_probes() -> Vec<AxisProbe> {
        (0..SAMPLES)
            .map(|index| {
                let t = index as f32;
                // 0,1,2 then the out-of-range arms 7 and -1, cycling.
                let axis = [0, 1, 2, 7, -1][index % 5];
                AxisProbe {
                    axis,
                    p: Vec3::new(t * 0.37 - 4.0, t * -0.53 + 2.5, t * 0.19 - 1.25),
                    // Every fifth sample zeroes a component on purpose.
                    n: Vec3::new(
                        [t * 0.11 - 1.2, 0.0][usize::from(index % 5 == 0)],
                        [1.6 - t * 0.13, -0.0][usize::from(index % 5 == 2)],
                        [t * -0.09 + 0.7, 0.0][usize::from(index % 5 == 4)],
                    ),
                    tile: Vec4::new(t * 0.13 + 0.5, 1.75 - t * 0.05, t * 0.02 - 0.2, 0.35),
                }
            })
            .collect()
    }

    /// One `owTangentFrame` input set.
    struct TangentProbe {
        d_eye_dx: Vec3,
        d_eye_dy: Vec3,
        d_uv_dx: Vec2,
        d_uv_dy: Vec2,
        n: Vec3,
    }

    /// [`SAMPLES`] tangent probes, including one whose uv derivatives are both
    /// zero — the `det == 0.0` arm the source guards with a ternary.
    fn tangent_probes() -> Vec<TangentProbe> {
        (0..SAMPLES)
            .map(|index| {
                let t = index as f32;
                let degenerate = usize::from(index % 8 == 3);
                TangentProbe {
                    d_eye_dx: Vec3::new(t * 0.031 + 0.4, t * -0.017, 0.12 - t * 0.005),
                    d_eye_dy: Vec3::new(t * -0.009, 0.27 + t * 0.013, t * 0.021 - 0.3),
                    d_uv_dx: Vec2::new(
                        [0.019 + t * 0.002, 0.0][degenerate],
                        [t * -0.0007, 0.0][degenerate],
                    ),
                    d_uv_dy: Vec2::new(
                        [t * 0.0011 - 0.004, 0.0][degenerate],
                        [0.023 - t * 0.0004, 0.0][degenerate],
                    ),
                    n: gl_normalize(Vec3::new(t * 0.07 - 0.8, 0.6, t * -0.05 + 0.4)),
                }
            })
            .collect()
    }

    /// Four `vec4` per sample, `SLOTS` in all, as the uniform's bytes.
    fn uniform_bytes(slots: &[[f32; 4]]) -> Vec<u8> {
        let mut bytes: Vec<u8> = slots
            .iter()
            .flat_map(|slot| slot.iter().copied())
            .flat_map(f32::to_le_bytes)
            .collect();
        bytes.resize(SLOTS * 16, 0);
        bytes
    }

    fn axis_uniform(probes: &[AxisProbe]) -> Vec<u8> {
        uniform_bytes(
            &probes
                .iter()
                .flat_map(|probe| {
                    [
                        [probe.p.x, probe.p.y, probe.p.z, probe.axis as f32],
                        [probe.n.x, probe.n.y, probe.n.z, 0.0],
                        [probe.tile.x, probe.tile.y, probe.tile.z, probe.tile.w],
                        [0.0; 4],
                    ]
                })
                .collect::<Vec<[f32; 4]>>(),
        )
    }

    fn tangent_uniform(probes: &[TangentProbe]) -> Vec<u8> {
        uniform_bytes(
            &probes
                .iter()
                .flat_map(|probe| {
                    [
                        [probe.d_eye_dx.x, probe.d_eye_dx.y, probe.d_eye_dx.z, 0.0],
                        [probe.d_eye_dy.x, probe.d_eye_dy.y, probe.d_eye_dy.z, 0.0],
                        [
                            probe.d_uv_dx.x,
                            probe.d_uv_dx.y,
                            probe.d_uv_dy.x,
                            probe.d_uv_dy.y,
                        ],
                        [probe.n.x, probe.n.y, probe.n.z, 0.0],
                    ]
                })
                .collect::<Vec<[f32; 4]>>(),
        )
    }

    /// An [`OwFrame`] as the three quadruples `frames_pack` emits.
    fn frame_rows(f: OwFrame) -> [[f32; 4]; 3] {
        [
            [f.uv.x, f.uv.y, f.t.x, f.t.y],
            [f.t.z, f.b.x, f.b.y, f.b.z],
            [f.n.x, f.n.y, f.n.z, 0.0],
        ]
    }

    /// An [`OwTangentBasis`] as the three quadruples `frames_pack_basis` emits.
    fn basis_rows(m: OwTangentBasis) -> [[f32; 4]; 3] {
        [
            [m.t.x, m.t.y, m.t.z, 0.0],
            [m.b.x, m.b.y, m.b.z, 0.0],
            [m.n.x, m.n.y, m.n.z, 0.0],
        ]
    }

    /// The budget one lane is held to: a floor plus one `f32` ULP of the value
    /// itself. See [`TOLERANCE_ABS`] / [`TOLERANCE_REL`].
    fn budget(expected: f32) -> f32 {
        TOLERANCE_ABS + TOLERANCE_REL * expected.abs()
    }

    /// Compare the CPU rows against the rendered pixels, failing loudly, and
    /// return `(worst absolute delta, worst delta as a fraction of its own
    /// budget)`. The second number is the measurement the tolerance is set from:
    /// at `1.0` the hardware needs exactly the budget it is given.
    fn compare(what: &str, cpu: &[[[f32; 4]; 3]], gpu: &[[f32; 4]]) -> (f32, f32) {
        cpu.iter()
            .enumerate()
            .flat_map(|(sample, rows)| {
                rows.iter().enumerate().flat_map(move |(row, expected)| {
                    let actual = gpu[row * SAMPLES + sample];
                    (0..4).map(move |lane| {
                        let delta = (expected[lane] - actual[lane]).abs();
                        let allowed = budget(expected[lane]);
                        assert!(
                            delta <= allowed,
                            "{what} disagrees at sample {sample} row {row} lane {lane}: \
                             CPU {} vs GPU {} (delta {delta}, budget {allowed})",
                            expected[lane],
                            actual[lane]
                        );
                        (delta, delta / allowed)
                    })
                })
            })
            .fold((0.0_f32, 0.0_f32), |(worst, ratio), (delta, share)| {
                (worst.max(delta), ratio.max(share))
            })
    }

    /// Assert one entry point's measurement against its budget, naming the entry
    /// point so a failure says which one moved.
    ///
    /// This used to `eprintln!` the numbers for `--nocapture`. It does not any
    /// more: no layer or module in this engine emits console output, tests
    /// included, and the architecture checker enforces that (Module Law #10).
    /// The measurement is better placed here anyway — a printed number is only
    /// read when someone thinks to look, whereas a share above 1.0 now fails the
    /// build, and the recorded figures live in `notes/material-frames.md`.
    fn report(what: &str, measured: (f32, f32)) -> (f32, f32) {
        assert!(
            measured.1 <= 1.0,
            "{what}: worst absolute delta {:e} is {:.3} of its budget — over 1.0              means the measured hardware error now exceeds the tolerance this              layer was pinned at. Re-derive the budget from the measurement; do              not widen it to fit.",
            measured.0,
            measured.1,
        );
        measured
    }

    /// The frame varies across the probe set — otherwise a parity pass against a
    /// constant would prove nothing.
    fn assert_varies(what: &str, cpu: &[[[f32; 4]; 3]]) {
        let spread = cpu.iter().fold((f32::MAX, f32::MIN), |(low, high), rows| {
            (low.min(rows[0][0]), high.max(rows[0][0]))
        });
        assert!(
            spread.1 - spread.0 > 0.5,
            "{what} must vary across the probe set, or the parity is vacuous"
        );
    }

    #[test]
    fn the_axis_frames_agree_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        let module = gpu.module();
        let probes = axis_probes();
        let rendered = gpu.render(&module, "frames_axis_fs", &axis_uniform(&probes));
        let cpu: Vec<[[f32; 4]; 3]> = probes
            .iter()
            .map(|probe| frame_rows(ow_axis_frame(probe.p, probe.n, probe.axis, probe.tile)))
            .collect();
        assert_varies("owAxisFrame", &cpu);
        report("owAxisFrame", compare("owAxisFrame", &cpu, &rendered));
        // Rows 1 and 2 are the basis: a select over literals, with no arithmetic
        // for the hardware to round differently. They are BIT-equal, not
        // within-tolerance, and saying so is what proves the whole of this entry
        // point's divergence is the tiled uv's multiply-add.
        (0..SAMPLES).for_each(|sample| {
            (1..3).for_each(|row| {
                assert_eq!(
                    cpu[sample][row],
                    rendered[row * SAMPLES + sample],
                    "the axis basis must be bit-equal at sample {sample} row {row}"
                );
            });
        });
    }

    #[test]
    fn the_orthonormalised_frames_agree_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        let module = gpu.module();
        let probes = axis_probes();
        let rendered = gpu.render(&module, "frames_ortho_fs", &axis_uniform(&probes));
        let cpu: Vec<[[f32; 4]; 3]> = probes
            .iter()
            .map(|probe| {
                frame_rows(ow_orthonormalise(
                    ow_axis_frame(probe.p, probe.n, probe.axis, probe.tile),
                    probe.n,
                ))
            })
            .collect();
        assert_varies("owOrthonormalise", &cpu);
        report(
            "owOrthonormalise",
            compare("owOrthonormalise", &cpu, &rendered),
        );
    }

    #[test]
    fn the_tangent_frame_agrees_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        let module = gpu.module();
        let probes = tangent_probes();
        let rendered = gpu.render(&module, "frames_tangent_fs", &tangent_uniform(&probes));
        let cpu: Vec<[[f32; 4]; 3]> = probes
            .iter()
            .map(|probe| {
                basis_rows(ow_tangent_frame(
                    probe.d_eye_dx,
                    probe.d_eye_dy,
                    probe.d_uv_dx,
                    probe.d_uv_dy,
                    probe.n,
                ))
            })
            .collect();
        report("owTangentFrame", compare("owTangentFrame", &cpu, &rendered));
        // The degenerate arm really is in the set, and really is zero on both
        // sides — a `det == 0` guard that never fires proves nothing.
        let degenerate: Vec<usize> = (0..SAMPLES).filter(|index| index % 8 == 3).collect();
        assert!(!degenerate.is_empty());
        degenerate.iter().for_each(|sample| {
            assert_eq!(rendered[*sample], [0.0, 0.0, 0.0, 0.0]);
            assert_eq!(rendered[SAMPLES + *sample], [0.0, 0.0, 0.0, 0.0]);
        });
    }

    /// The fragment-only wrapper: real `dpdx`/`dpdy` over a probe whose exact
    /// derivatives are known by construction, fed to the same CPU reference.
    /// This is what stops `owTangentFrameScreen` from silently swapping the two.
    #[test]
    fn the_screen_derivative_wrapper_supplies_dpdx_and_dpdy_in_the_source_order() {
        let gpu = Gpu::acquire();
        let module = gpu.module();
        let probes = tangent_probes();
        let rendered = gpu.render(&module, "frames_screen_fs", &tangent_uniform(&probes));
        let cpu: Vec<[[f32; 4]; 3]> = probes
            .iter()
            .map(|probe| {
                basis_rows(ow_tangent_frame(
                    PROBE_D_EYE_DX,
                    PROBE_D_EYE_DY,
                    PROBE_D_UV_DX,
                    PROBE_D_UV_DY,
                    probe.n,
                ))
            })
            .collect();
        report(
            "owTangentFrameScreen",
            compare("owTangentFrameScreen", &cpu, &rendered),
        );
        // And the swap really is detectable: the frame built from the derivatives
        // exchanged is a different frame, so a wrapper that swapped them would
        // have failed above.
        let straight = ow_tangent_frame(
            PROBE_D_EYE_DX,
            PROBE_D_EYE_DY,
            PROBE_D_UV_DX,
            PROBE_D_UV_DY,
            probes[0].n,
        );
        let swapped = ow_tangent_frame(
            PROBE_D_EYE_DY,
            PROBE_D_EYE_DX,
            PROBE_D_UV_DY,
            PROBE_D_UV_DX,
            probes[0].n,
        );
        assert!(straight.t.subtract(swapped.t).length() > 0.1);
    }
}
