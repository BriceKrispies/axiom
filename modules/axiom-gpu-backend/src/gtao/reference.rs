//! **The CPU reference: the semantic definition of what [`super::wgsl`] means.**
//!
//! Written from the GLSL text of `src/render/gtao.js` and the `COMMON` chunk of
//! `src/render/glsl.js`, **not** from the WGSL — the two transcriptions are
//! independent on purpose, and where they disagree the algorithm decides. This
//! port has measured what a shared misreading costs (ten defects in `sky/`
//! alone), so every division below is a division, every sum is left-associated
//! exactly as the source writes it, and nothing is folded into a reciprocal
//! multiply.
//!
//! # The GLSL builtins are written out
//!
//! `mix`, `clamp`, `sign`, `fract`, `dot`, `length`, `normalize` and `cross` all
//! have exact GLSL definitions and WGSL's peers are permitted to factor
//! differently. So each is expanded here in the one order both sides use:
//!
//! | GLSL | here |
//! |---|---|
//! | `mix(x, y, a)` | `x * (1 - a) + y * a` |
//! | `clamp(x, lo, hi)` | `min(max(x, lo), hi)` |
//! | `sign(x)` | `f32(x > 0) - f32(x < 0)` — **`0.0` at zero**, unlike `f32::signum` |
//! | `fract(x)` | `x - floor(x)` — **not** `%`, and screen coords are positive but the hash's are not |
//! | `dot(a, b)` | `a.x*b.x + a.y*b.y + a.z*b.z`, left-associated |
//! | `length(v)` | `sqrt(dot(v, v))` with that same expansion |
//! | `normalize(v)` | `v / length(v)` — three divisions, not one reciprocal |
//!
//! # The two `if`s inside the loops are selections, not branches
//!
//! `if ( projLen < 1e-4 ) continue;` and `if ( len2 > 2e-5 )` both mean *"this
//! iteration contributes nothing"*, and both guard a division that would
//! otherwise produce a NaN. They are ported as **table selection**
//! (`[skipped, taken][usize::from(cond)]`) rather than a multiply by a `0.0/1.0`
//! mask, because `NaN * 0.0` is `NaN` and a selection discards the unused value
//! outright. That is the Branchless Law's answer and it is also the *correct*
//! answer — the mask would have silently poisoned every degenerate slice.

use super::{SLICES, STEPS};

/// `fract( x )` — `x - floor( x )`. Rust's `%` is not this for negative
/// arguments, and [`hash12`]'s intermediates go negative.
pub(crate) fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// GLSL `clamp( x, lo, hi )` = `min( max( x, lo ), hi )`.
pub(crate) fn glsl_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    f32::min(f32::max(x, lo), hi)
}

/// GLSL `mix( x, y, a )` = `x * ( 1 - a ) + y * a`. Not `x + a * (y - x)`, which
/// is algebraically equal and numerically different.
pub(crate) fn glsl_mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// GLSL `sign( x )`, which returns **`0.0` for any zero** — including `-0.0`.
///
/// `f32::signum` returns `1.0` for `+0.0` and `-1.0` for `-0.0`, and `Math.sign`
/// returns the signed zero itself. This is the GLSL one, and it matters: it is
/// applied to `dot( orthoDir, projNn )`, and at exactly grazing incidence the
/// source wants the slice's `n` to be `0`, not `±acos( cosN )`.
pub(crate) fn glsl_sign(x: f32) -> f32 {
    f32::from(x > 0.0) - f32::from(x < 0.0)
}

/// `owIGN` from `glsl.js` — Jimenez's interleaved gradient noise, the dither that
/// rotates each pixel's slice set:
///
/// ```glsl
/// return fract( 52.9829189 * fract( dot( p, vec2( 0.06711056, 0.00583715 ) ) ) );
/// ```
///
/// Both `fract`s, in that order, and the `dot` written out. The inner one is what
/// keeps the outer multiply in a range where an `f32` still has mantissa left;
/// dropping it (a tempting "simplification") turns the noise into visible
/// diagonal banding at the far end of a street.
pub(crate) fn ign(p: [f32; 2]) -> f32 {
    fract(52.982_918_9 * fract(p[0] * 0.067_110_56 + p[1] * 0.005_837_15))
}

/// `owHash12` from `glsl.js` — Dave Hoskins' white hash, the jitter on the step
/// *positions*:
///
/// ```glsl
/// vec3 p3 = fract( vec3( p.xyx ) * 0.1031 );
/// p3 += dot( p3, p3.yzx + 33.33 );
/// return fract( ( p3.x + p3.y ) * p3.z );
/// ```
///
/// Note `p.xyx` — the **x** component appears twice and `y` once, so this is not
/// symmetric in its arguments. Note also that `p3.yzx + 33.33` broadcasts the
/// scalar across all three lanes *before* the dot, and that `33.33` is an `f32`
/// literal in the source and so is not `33.33_f64`.
pub(crate) fn hash12(p: [f32; 2]) -> f32 {
    let p3 = [
        fract(p[0] * 0.1031),
        fract(p[1] * 0.1031),
        fract(p[0] * 0.1031),
    ];
    let d = p3[0] * (p3[1] + 33.33) + p3[1] * (p3[2] + 33.33) + p3[2] * (p3[0] + 33.33);
    let p3 = [p3[0] + d, p3[1] + d, p3[2] + d];
    fract((p3[0] + p3[1]) * p3[2])
}

/// `owViewPos` from `glsl.js` — view-space position from a uv and a **positive
/// linear view depth in metres**:
///
/// ```glsl
/// vec4 h = projInv * vec4( uv * 2.0 - 1.0, 1.0, 1.0 );
/// vec3 dir = h.xyz / h.w;
/// dir /= max( 1e-6, -dir.z );
/// return dir * depth;
/// ```
///
/// `proj_inv` is **column-major**, the convention every matrix crossing this
/// backend uses, and GLSL's `m * v` is `c0*v.x + c1*v.y + c2*v.z + c3*v.w` — so
/// row `r` reads `m[r], m[4+r], m[8+r], m[12+r]`.
///
/// The `uv` this takes is the **source's**: `v` running up, so that `uv * 2 - 1`
/// is NDC. A WebGPU caller flips first; see
/// [`super::NDC_UV_V_SIGN`]. The transcription is left alone precisely so the
/// correction stays visible at the call site rather than buried in the maths.
pub(crate) fn view_pos(uv: [f32; 2], depth: f32, proj_inv: &[f32; 16]) -> [f32; 3] {
    let c = [uv[0] * 2.0 - 1.0, uv[1] * 2.0 - 1.0, 1.0, 1.0];
    let h = [0_usize, 1, 2, 3].map(|row| {
        proj_inv[row] * c[0]
            + proj_inv[4 + row] * c[1]
            + proj_inv[8 + row] * c[2]
            + proj_inv[12 + row] * c[3]
    });
    let dir = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
    let scale = f32::max(1e-6, -dir[2]);
    let unit = [dir[0] / scale, dir[1] / scale, dir[2] / scale];
    [unit[0] * depth, unit[1] * depth, unit[2] * depth]
}

/// `owArc` — the closed-form cosine-weighted visible arc, and the whole reason
/// this is GTAO rather than SSAO:
///
/// ```glsl
/// return 0.25 * ( -cos( 2.0 * h - n ) + cosN + 2.0 * h * sinN );
/// ```
///
/// `h` is a horizon angle and `n` the projected normal's angle, both measured
/// about the slice's `orthoDir`. The `0.25` in front is not a fudge: it is the
/// `1/4` that falls out of integrating `cos(θ - n)` over the arc, and it is what
/// makes two arcs sum to the correct hemisphere visibility rather than to
/// something that then needs a magic scale.
pub(crate) fn arc(h: f32, n: f32, cos_n: f32, sin_n: f32) -> f32 {
    0.25 * (-(2.0 * h - n).cos() + cos_n + 2.0 * h * sin_n)
}

/// The world radius projected to pixels, then clamped:
///
/// ```glsl
/// float radiusPx = radius * uP11 * 0.5 * uResolution.y / max( 0.2, depth );
/// radiusPx = clamp( radiusPx, 6.0, 128.0 );
/// ```
///
/// `p11` is `camera.projectionMatrix.elements[5]` — the `[1][1]` entry, i.e.
/// `1 / tan( fovY / 2 )`. The chain is left-associated and the depth is a
/// **division**, not a multiply by a precomputed reciprocal.
///
/// Both clamp ends are load-bearing. The `128` ceiling is why the step
/// distribution below had to become quadratic; the `6` floor is what keeps a
/// distant surface from collapsing its whole search inside one texel.
pub(crate) fn radius_px(radius: f32, p11: f32, resolution_y: f32, depth: f32) -> f32 {
    let r = radius * p11 * 0.5 * resolution_y / f32::max(0.2, depth);
    glsl_clamp(r, 6.0, 128.0)
}

/// The **quadratic** step distribution, in pixels along the slice direction:
///
/// ```glsl
/// float ft = ( float( t ) + noise2 ) / float( OW_STEPS );
/// float off = radiusPx * ft * ft + 1.0;
/// ```
///
/// The source's comment is the specification for *why*, and is worth keeping
/// whole: a 1.35 m radius on a wall three metres away projects to 316 px, clamps
/// to 128, and with eight **linear** steps put the first sample sixteen pixels
/// out — so the wall/soffit junction, the foot of a column and the gap under a
/// crate, i.e. *every contact in the frame*, were never sampled at all and the
/// buffer came back at 0.92 visibility almost everywhere. Weighting toward the
/// origin puts the first three taps inside six pixels while still reaching the
/// full radius, at the same eight taps.
///
/// The `+ 1.0` is the other half of that fix and is not slack: a sample that
/// lands back on the centre texel yields a zero-length `ds`, a garbage horizon
/// direction, and a visibility arc that closes completely.
///
/// Grouping: `radiusPx * ft * ft` is `(radiusPx * ft) * ft`, **not**
/// `radiusPx * (ft * ft)`.
pub(crate) fn step_offset(step: usize, noise2: f32, radius_px: f32) -> f32 {
    let ft = (step as f32 + noise2) / STEPS as f32;
    radius_px * ft * ft + 1.0
}

/// One tap of the horizon search, after the pass has already decided whether it
/// counts.
pub(crate) struct Tap {
    /// The tap's **view-space** position, reconstructed by [`view_pos`] from its
    /// own uv and its own depth.
    pub(crate) view_pos: [f32; 3],
    /// Whether this tap is used at all. The pass folds the source's two guards
    /// into this one flag: the uv-bounds test
    /// (`uv1.x > 0.0 && uv1.x < 1.0 && …`, note **strict** on both ends) and the
    /// coverage test (`texture2D( tNormal, uv1 ).z > 0.5`). A rejected tap leaves
    /// the horizon exactly as it was.
    pub(crate) accepted: bool,
}

/// One tap folded into a running horizon cosine:
///
/// ```glsl
/// vec3 ds = owViewPos( uv1, d1, uProjInv ) - P;
/// float len2 = dot( ds, ds );
/// if ( len2 > 2e-5 ) {
///   float inv = inversesqrt( len2 );
///   float c = dot( ds, V ) * inv;
///   float fall = clamp( len2 * invR2, 0.0, 1.0 );
///   fall *= fall;
///   cosHPos = max( cosHPos, mix( c, cosHPos, fall ) );
/// }
/// ```
///
/// **This is the thickness heuristic.** There is no `thickness` term anywhere
/// else in the pass (`uParams.w` is never read); the falloff is what plays its
/// part. `fall` is `clamp(len²/r², 0, 1)` *squared* — a quartic in distance — and
/// it is the second argument of the `mix`, so a tap at the full radius returns
/// the incumbent horizon unchanged while a tap at the origin returns its raw
/// cosine. That is what stops a distant silhouette occluding as if it were an
/// infinitely deep wall.
///
/// `inversesqrt` is the source's; the reference computes `1.0 / sqrt(len2)`,
/// which is the value `inversesqrt` approximates. The `2e-5` guard is a table
/// selection, not a mask — see the module docs.
pub(crate) fn horizon_update(cos_h: f32, ds: [f32; 3], v: [f32; 3], inv_r2: f32) -> f32 {
    let len2 = ds[0] * ds[0] + ds[1] * ds[1] + ds[2] * ds[2];
    let inv = 1.0 / len2.sqrt();
    let c = (ds[0] * v[0] + ds[1] * v[1] + ds[2] * v[2]) * inv;
    let fall = glsl_clamp(len2 * inv_r2, 0.0, 1.0);
    let fall = fall * fall;
    let updated = f32::max(cos_h, glsl_mix(c, cos_h, fall));
    [cos_h, updated][usize::from(len2 > 2e-5)]
}

/// The horizon on one side of one slice: fold [`STEPS`] taps, starting from the
/// source's `-1.0` seed (i.e. "nothing seen yet", a horizon at 180°).
pub(crate) fn horizon(taps: &[Tap], p: [f32; 3], v: [f32; 3], inv_r2: f32) -> f32 {
    taps.iter().fold(-1.0_f32, |cos_h, tap| {
        let ds = [
            tap.view_pos[0] - p[0],
            tap.view_pos[1] - p[1],
            tap.view_pos[2] - p[2],
        ];
        [cos_h, horizon_update(cos_h, ds, v, inv_r2)][usize::from(tap.accepted)]
    })
}

/// A slice's geometric frame: everything derived from the normal, the view vector
/// and the slice azimuth, before any tap is taken.
pub(crate) struct SliceFrame {
    /// `length( projN )` — how much of the normal survives projection into this
    /// slice's plane. Also the slice's **weight** in the sum, which is what makes
    /// a slice whose normal barely lies in it count for barely anything.
    pub(crate) proj_len: f32,
    /// `clamp( dot( projNn, V ), -1.0, 1.0 )`.
    pub(crate) cos_n: f32,
    /// `sign( dot( orthoDir, projNn ) ) * acos( cosN )` — the projected normal's
    /// **signed** angle about `orthoDir`.
    pub(crate) n: f32,
    /// `sin( n )`, hoisted out of the two [`arc`] calls exactly as the source
    /// hoists it.
    pub(crate) sin_n: f32,
}

/// The slice frame:
///
/// ```glsl
/// vec3 axis = normalize( cross( sliceDir, V ) );
/// vec3 projN = N - axis * dot( N, axis );
/// float projLen = length( projN );
/// if ( projLen < 1e-4 ) continue;
/// vec3 projNn = projN / projLen;
/// vec3 orthoDir = normalize( sliceDir - V * dot( sliceDir, V ) );
/// float cosN = clamp( dot( projNn, V ), -1.0, 1.0 );
/// float n = sign( dot( orthoDir, projNn ) ) * acos( cosN );
/// float sinN = sin( n );
/// ```
///
/// `sliceDir` is `vec3( dir2, 0.0 )` — a **view-space** vector, which is the fact
/// [`super::SCREEN_STEP_V_SIGN`] exists for. `dir2` is `(cos φ, sin φ)` with
/// `φ = ( s + noise ) * ( π / SLICES )`; note the source parenthesises
/// `( OW_PI / float( OW_SLICES ) )` as a group, so it is one division evaluated
/// before the multiply.
///
/// The `projLen < 1e-4` skip is *not* applied here — the frame is still computed,
/// and [`slice_visibility`] discards its contribution — because that is where the
/// source's `continue` actually takes effect (nothing else survives an
/// iteration).
pub(crate) fn slice_frame(normal: [f32; 3], v: [f32; 3], dir2: [f32; 2]) -> SliceFrame {
    let slice_dir = [dir2[0], dir2[1], 0.0_f32];
    // cross( sliceDir, V )
    let cr = [
        slice_dir[1] * v[2] - slice_dir[2] * v[1],
        slice_dir[2] * v[0] - slice_dir[0] * v[2],
        slice_dir[0] * v[1] - slice_dir[1] * v[0],
    ];
    let cr_len = (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt();
    let axis = [cr[0] / cr_len, cr[1] / cr_len, cr[2] / cr_len];
    let n_dot_axis = normal[0] * axis[0] + normal[1] * axis[1] + normal[2] * axis[2];
    let proj_n = [
        normal[0] - axis[0] * n_dot_axis,
        normal[1] - axis[1] * n_dot_axis,
        normal[2] - axis[2] * n_dot_axis,
    ];
    let proj_len = (proj_n[0] * proj_n[0] + proj_n[1] * proj_n[1] + proj_n[2] * proj_n[2]).sqrt();
    let proj_nn = [
        proj_n[0] / proj_len,
        proj_n[1] / proj_len,
        proj_n[2] / proj_len,
    ];
    let sd_dot_v = slice_dir[0] * v[0] + slice_dir[1] * v[1] + slice_dir[2] * v[2];
    let ortho = [
        slice_dir[0] - v[0] * sd_dot_v,
        slice_dir[1] - v[1] * sd_dot_v,
        slice_dir[2] - v[2] * sd_dot_v,
    ];
    let ortho_len = (ortho[0] * ortho[0] + ortho[1] * ortho[1] + ortho[2] * ortho[2]).sqrt();
    let ortho_dir = [
        ortho[0] / ortho_len,
        ortho[1] / ortho_len,
        ortho[2] / ortho_len,
    ];
    let cos_n = glsl_clamp(
        proj_nn[0] * v[0] + proj_nn[1] * v[1] + proj_nn[2] * v[2],
        -1.0,
        1.0,
    );
    let n = glsl_sign(ortho_dir[0] * proj_nn[0] + ortho_dir[1] * proj_nn[1] + ortho_dir[2] * proj_nn[2])
        * cos_n.acos();
    SliceFrame {
        proj_len,
        cos_n,
        n,
        sin_n: n.sin(),
    }
}

/// The slice azimuth: `phi = ( float( s ) + noise ) * ( OW_PI / float( OW_SLICES ) )`,
/// returned as the source's `dir2 = vec2( cos( phi ), sin( phi ) )`.
///
/// `π` is the source's `OW_PI = 3.141592653589793`, an `f64` literal narrowed to
/// `f32` by GLSL's `const float` — which is `std::f32::consts::PI` exactly.
pub(crate) fn slice_direction(slice: usize, noise: f32) -> [f32; 2] {
    let phi = (slice as f32 + noise) * (core::f32::consts::PI / SLICES as f32);
    [phi.cos(), phi.sin()]
}

/// One slice's contribution to the visibility sum:
///
/// ```glsl
/// float h1 = -acos( clamp( cosHNeg, -1.0, 1.0 ) );
/// float h2 =  acos( clamp( cosHPos, -1.0, 1.0 ) );
/// h1 = n + max( h1 - n, -OW_HALF_PI );
/// h2 = n + min( h2 - n,  OW_HALF_PI );
/// visibility += projLen * ( owArc( h1, n, cosN, sinN ) + owArc( h2, n, cosN, sinN ) );
/// ```
///
/// Two things a reader is likely to "tidy" and must not.
///
/// **The `±π/2` clamp is about `n`, not about zero.** `n + max( h1 - n, -π/2 )`
/// is not `max( h1, -π/2 )`: the horizon is being confined to the hemisphere
/// *around the surface normal*, which is a different interval on every pixel.
///
/// **There is deliberately no per-slice clamp on the contribution.** The source's
/// comment: *"A single slice legitimately integrates to more than 1 on tilted
/// surfaces; the excess is what compensates the slices whose projected normal is
/// short. Clamping per slice (or per frame) biases the whole buffer dark, which
/// is the classic 'my SSAO looks like dirt' bug."* The only clamp is
/// [`resolve_visibility`]'s, after the division by [`SLICES`], and it is `0..=4`
/// rather than `0..=1` for exactly that reason.
pub(crate) fn slice_visibility(cos_h_neg: f32, cos_h_pos: f32, frame: &SliceFrame) -> f32 {
    let h1 = -glsl_clamp(cos_h_neg, -1.0, 1.0).acos();
    let h2 = glsl_clamp(cos_h_pos, -1.0, 1.0).acos();
    let h1 = frame.n + f32::max(h1 - frame.n, -core::f32::consts::FRAC_PI_2);
    let h2 = frame.n + f32::min(h2 - frame.n, core::f32::consts::FRAC_PI_2);
    let contribution = frame.proj_len
        * (arc(h1, frame.n, frame.cos_n, frame.sin_n) + arc(h2, frame.n, frame.cos_n, frame.sin_n));
    // `if ( projLen < 1e-4 ) continue;` — the slice contributes nothing.
    [contribution, 0.0][usize::from(frame.proj_len < 1e-4)]
}

/// `clamp( visibility / float( OW_SLICES ), 0.0, 4.0 )` — the core pass's `r`
/// channel. A **division** by the slice count, and a ceiling of four, not one.
pub(crate) fn resolve_visibility(sum: f32) -> f32 {
    glsl_clamp(sum / SLICES as f32, 0.0, 4.0)
}

/// The temporal accumulator's history weight, before the neighbourhood clamp:
///
/// ```glsl
/// float w = uFeedback;
/// if ( huv.x < 0.0 || huv.x > 1.0 || huv.y < 0.0 || huv.y > 1.0 ) w = 0.0;
/// float rel = abs( hist.y - cur.y ) / max( 0.05, cur.y );
/// w *= exp( -rel * 30.0 );
/// ```
///
/// Two rejections, multiplied: off-screen history is discarded outright, and
/// history whose *depth* disagrees is faded out on a `30x` relative exponential
/// — a 3.3% depth discrepancy already halves it. That relative form is why the
/// same constant works at 2 m and at 200 m, and the `max( 0.05, cur.y )` floor is
/// what keeps it finite as depth goes to zero.
///
/// `huv` is `vUv - vel` (the *current* uv minus the current-minus-previous
/// delta). The `glsl.js` header claims velocity "can be **added** directly to a
/// uv"; both this pass and `taa.js` subtract, and `taa.js`'s own fallback
/// (`vel = vUv - prevUv`) settles it — the header comment is wrong, the shaders
/// are right.
pub(crate) fn temporal_weight(
    feedback: f32,
    history_uv: [f32; 2],
    history_depth: f32,
    current_depth: f32,
) -> f32 {
    let inside = (history_uv[0] >= 0.0)
        & (history_uv[0] <= 1.0)
        & (history_uv[1] >= 0.0)
        & (history_uv[1] <= 1.0);
    let w = [0.0, feedback][usize::from(inside)];
    let rel = (history_depth - current_depth).abs() / f32::max(0.05, current_depth);
    w * (-rel * 30.0).exp()
}

/// The **wide** neighbourhood clamp:
///
/// ```glsl
/// float mn = cur.x, mx = cur.x;
/// for ( int i = 0; i < 4; i ++ ) { … float s = texture2D( tCurrent, vUv + o * uTexel * 2.0 ).r; … }
/// float h = clamp( hist.x, mn - 0.45, mx + 0.45 );
/// ```
///
/// The four neighbours are at **two** texels, in the source's order
/// `(+1, 0), (-1, 0), (0, +1), (0, -1)` scaled by `uTexel * 2.0`, and the window
/// is widened by `±0.45` on each side. The source's comment explains the width:
/// *"the per-frame signal is 3 slices of a stochastic integral, so a tight clamp
/// would just re-inject its variance."* A conventional AABB clamp here does not
/// stabilise this buffer, it destroys the accumulation that is the whole point.
pub(crate) fn temporal_clamp(history_ao: f32, current_ao: f32, neighbours: [f32; 4]) -> f32 {
    let (mn, mx) = neighbours
        .iter()
        .fold((current_ao, current_ao), |(mn, mx), s| {
            (f32::min(mn, *s), f32::max(mx, *s))
        });
    glsl_clamp(history_ao, mn - 0.45, mx + 0.45)
}

/// `mix( cur.x, h, w )` — the accumulator's output `r`. Its `g` is `cur.y`, the
/// **current** depth, never the history's: the history channel has to describe
/// the frame it is stored for or the next frame's rejection test compares the
/// wrong pair.
pub(crate) fn temporal_blend(current_ao: f32, clamped_history: f32, weight: f32) -> f32 {
    glsl_mix(current_ao, clamped_history, weight)
}

/// The neighbour offsets the temporal clamp samples, in **texels**, in the
/// source's `i = 0..4` order. `o * uTexel * 2.0`, so each is two texels out.
///
/// The source writes them as a nested ternary
/// (`vec2( i == 0 ? 1.0 : i == 1 ? -1.0 : 0.0, … )`); a table is the same four
/// vectors and says so.
pub(crate) const TEMPORAL_NEIGHBOUR_TEXELS: [[f32; 2]; 4] = [
    [2.0, 0.0],
    [-2.0, 0.0],
    [0.0, 2.0],
    [0.0, -2.0],
];

/// Taps per side of the separable bilateral blur: `for ( int i = 1; i <= 3; i ++ )`.
/// Seven taps in total per pass, two passes.
pub(crate) const BLUR_TAPS: usize = 3;

/// The centre tap's weight, which is also the sum's seed: `sum = c.r * 0.4`,
/// `wsum = 0.4`.
pub(crate) const BLUR_CENTRE_WEIGHT: f32 = 0.4;

/// One blur tap's depth-aware weight:
///
/// ```glsl
/// float wa = w0 * exp( -abs( a.g - c.g ) * 22.0 / max( 0.1, c.g ) );
/// ```
///
/// Grouping: the exponent is `( ( -|Δd| ) * 22.0 ) / max( 0.1, c.g )`, left to
/// right — the division is by the *centre's* depth, applied last. This is the
/// edge-stopping term: it is what keeps the blur from dragging a wall's occlusion
/// across a silhouette onto the street behind it, which is the difference between
/// "grounded" and "smudged".
///
/// `w0` is the distance falloff, `0.4 / float( i + 1 )` for `i` in `1..=3`, i.e.
/// `0.2`, `0.4/3`, `0.1` — see [`blur_distance_weight`].
pub(crate) fn blur_tap_weight(w0: f32, tap_depth: f32, centre_depth: f32) -> f32 {
    w0 * (-(tap_depth - centre_depth).abs() * 22.0 / f32::max(0.1, centre_depth)).exp()
}

/// `w0 = 0.4 / float( i + 1 )` — a **reciprocal** falloff, not a Gaussian. One
/// division per tap, exactly as written.
pub(crate) fn blur_distance_weight(tap: usize) -> f32 {
    BLUR_CENTRE_WEIGHT / (tap + 1) as f32
}

/// The blur's weighted sum and weight total, before the divide:
///
/// ```glsl
/// float sum = c.r * 0.4;
/// float wsum = 0.4;
/// for ( int i = 1; i <= 3; i ++ ) { … sum += a.r * wa + b.r * wb; wsum += wa + wb; }
/// ```
///
/// `taps[i]` is the pair `[a, b]` at `+i` and `-i` along `uDirection`, each a
/// `(visibility, depth)` — the `.rg` the source reads. The accumulation is
/// `sum + ( a.r * wa + b.r * wb )`: the pair is summed *first* and then added,
/// which is not the same rounding as two separate `+=`.
pub(crate) fn blur_accumulate(centre: [f32; 2], taps: &[[[f32; 2]; 2]; BLUR_TAPS]) -> (f32, f32) {
    (1..=BLUR_TAPS).fold(
        (centre[0] * BLUR_CENTRE_WEIGHT, BLUR_CENTRE_WEIGHT),
        |(sum, wsum), i| {
            let w0 = blur_distance_weight(i);
            let a = taps[i - 1][0];
            let b = taps[i - 1][1];
            let wa = blur_tap_weight(w0, a[1], centre[1]);
            let wb = blur_tap_weight(w0, b[1], centre[1]);
            (sum + (a[0] * wa + b[0] * wb), wsum + (wa + wb))
        },
    )
}

/// The blur's output `r`:
///
/// ```glsl
/// float ao = sum / wsum;
/// if ( uParams.x > 0.5 ) ao = pow( clamp( ao, 0.0, 1.0 ), uParams.y );
/// ```
///
/// **The clamp and the intensity curve run on the last stage only.** `render()`
/// sets `uParams.x = 0` for the horizontal pass and `1` for the vertical, and the
/// comment says why: *"clamp + intensity curve on the last stage only"*. Applying
/// `pow` twice would square the exponent and applying the `0..=1` clamp early
/// would throw away the above-one visibility the horizontal pass is still
/// carrying — the same excess [`slice_visibility`] deliberately does not clamp.
pub(crate) fn blur_output(sum: f32, wsum: f32, apply_curve: bool, intensity: f32) -> f32 {
    let ao = sum / wsum;
    let curved = glsl_clamp(ao, 0.0, 1.0).powf(intensity);
    [ao, curved][usize::from(apply_curve)]
}

/// One store-and-load round trip through the pass chain's **`RG16Float`** target.
///
/// `setSize` allocates every one of `rtRaw`, `rtBlur`, `rtFinal` and both history
/// buffers as `hdrTarget( w, h, { type: THREE.HalfFloatType, format:
/// THREE.RGFormat } )`. So the chain quantises **three times** on the temporal
/// path (core → temporal → blur-H → blur-V) and the history read is a fourth. A
/// CPU reference that carried `f32` throughout would disagree with the GPU by
/// roughly `5e-4` relative for a reason that is the *storage*, not the port, and
/// any tolerance derived from that measurement would be measuring the wrong
/// thing.
///
/// Delegates to [`crate::bloom_pyramid::half_storage::quantize`], which is
/// round-to-nearest-even in both directions and is driven across all 65,536 half
/// bit patterns by its own tests. That module's header says *"the moment a second
/// pass needs it, lift it whole"* — this is that second pass; see the notes file
/// for the lift the orchestrator should make.
pub(crate) fn store_rg16f(value: [f32; 2]) -> [f32; 2] {
    value.map(crate::bloom_pyramid::half_storage::quantize)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plausible perspective inverse: 60° vertical fov, 16:9, near 0.1.
    /// Column-major, so `[0]`, `[5]`, `[11]`, `[14]` carry the interesting parts.
    fn proj_inv() -> [f32; 16] {
        let p11 = 1.0 / (30.0_f32.to_radians().tan());
        let p00 = p11 / (16.0 / 9.0);
        // 0.5 / 50, not 0.1 / 500: the inverse's `w` row is `a + b` with
        // `a = (n-f)/2fn` and `b = (f+n)/2fn`, which CANCELS catastrophically as
        // the ratio grows (0.1/500 loses 3.4 digits before the reconstruction
        // even starts). 0.5/50 keeps the cancellation at 50x.
        let (near, far) = (0.5_f32, 50.0_f32);
        // The exact inverse of the standard column-major GL perspective
        //   [ p00 0 0 0 | 0 p11 0 0 | 0 0 (f+n)/(n-f) -1 | 0 0 2fn/(n-f) 0 ]
        let a = (near - far) / (2.0 * far * near);
        let b = (far + near) / (2.0 * far * near);
        [
            1.0 / p00, 0.0, 0.0, 0.0, //
            0.0, 1.0 / p11, 0.0, 0.0, //
            0.0, 0.0, 0.0, a, //
            0.0, 0.0, -1.0, b,
        ]
    }

    #[test]
    fn fract_is_floor_subtraction_and_not_a_remainder() {
        assert_eq!(fract(2.25), 0.25);
        // The case Rust's `%` gets wrong: `-2.25 % 1.0` is `-0.25`.
        assert_eq!(fract(-2.25), 0.75);
        assert_eq!(fract(-2.25) - (-2.25_f32 % 1.0), 1.0);
        assert_eq!(fract(0.0), 0.0);
    }

    #[test]
    fn glsl_sign_returns_zero_at_both_zeroes_where_signum_does_not() {
        assert_eq!(glsl_sign(0.0), 0.0);
        assert_eq!(glsl_sign(-0.0), 0.0);
        assert_eq!(glsl_sign(3.0), 1.0);
        assert_eq!(glsl_sign(-3.0), -1.0);
        assert_ne!(glsl_sign(0.0), 0.0_f32.signum());
        assert_ne!(glsl_sign(-0.0), (-0.0_f32).signum());
    }

    #[test]
    fn clamp_and_mix_are_the_glsl_forms() {
        assert_eq!(glsl_clamp(5.0, -1.0, 1.0), 1.0);
        assert_eq!(glsl_clamp(-5.0, -1.0, 1.0), -1.0);
        assert_eq!(glsl_clamp(0.25, -1.0, 1.0), 0.25);
        assert_eq!(glsl_mix(2.0, 6.0, 0.0), 2.0);
        assert_eq!(glsl_mix(2.0, 6.0, 1.0), 6.0);
        assert_eq!(glsl_mix(2.0, 6.0, 0.25), 3.0);
    }

    #[test]
    fn the_two_noises_are_bounded_and_decorrelated() {
        // Both are `fract` of something, so both live in [0, 1).
        let out_of_range = (0..64)
            .map(|i| {
                let p = [i as f32 * 7.0 + 0.5, i as f32 * 3.0 + 0.5];
                (ign(p), hash12(p))
            })
            .filter(|(a, b)| !((*a >= 0.0) & (*a < 1.0) & (*b >= 0.0) & (*b < 1.0)))
            .count();
        assert_eq!(out_of_range, 0, "fract-based noise must stay in [0, 1)");

        // And they must not be the same function: a slice rotation shared with
        // the step jitter is what bands.
        let same = (0..64)
            .map(|i| {
                let p = [i as f32 + 0.5, i as f32 * 2.0 + 0.5];
                (ign(p) - hash12(p)).abs()
            })
            .filter(|d| *d < 1e-6)
            .count();
        assert!(same < 4, "ign and hash12 agree on {same} of 64 points");
    }

    #[test]
    fn ign_is_the_sources_two_nested_fracts() {
        // Recomputed here from the GLSL text rather than by calling `ign`.
        let p = [123.5_f32, 77.5];
        let inner = {
            let d = p[0] * 0.067_110_56 + p[1] * 0.005_837_15;
            d - d.floor()
        };
        let outer = {
            let m = 52.982_918_9 * inner;
            m - m.floor()
        };
        assert_eq!(ign(p), outer);
        // The frame stride is a broadcast add on BOTH components.
        let stride = crate::gtao::FRAME_NOISE_STRIDE;
        let phased = ign([p[0] + stride, p[1] + stride]);
        assert_ne!(phased, outer, "successive frames must rotate the slice set");
    }

    #[test]
    fn hash12_uses_x_twice_and_is_not_symmetric() {
        assert_ne!(hash12([3.5, 9.25]), hash12([9.25, 3.5]));
        // `vec3(p.xyx)`: lanes 0 and 2 are both `x`.
        let p = [3.5_f32, 9.25];
        let p3 = [fract(p[0] * 0.1031), fract(p[1] * 0.1031), fract(p[0] * 0.1031)];
        assert_eq!(p3[0], p3[2]);
    }

    #[test]
    fn view_pos_reconstructs_the_depth_it_was_given() {
        let m = proj_inv();
        [
            ([0.5_f32, 0.5], 3.0_f32),
            ([0.1, 0.9], 12.5),
            ([0.97, 0.02], 0.75),
        ]
        .iter()
        .for_each(|(uv, depth)| {
            let p = view_pos(*uv, *depth, &m);
            // Positive linear view depth in metres: `-z` is the depth exactly,
            // because `owViewPos` normalises the ray to the `z = -1` plane first.
            assert!(
                (-p[2] - depth).abs() < 1e-5,
                "uv {uv:?} depth {depth}: reconstructed -z is {}",
                -p[2]
            );
            // The centre of the screen is on the axis.
            let centred = ((uv[0] - 0.5).abs() < 1e-6) & ((uv[1] - 0.5).abs() < 1e-6);
            assert_eq!(
                centred,
                (p[0].abs() < 1e-6) & (p[1].abs() < 1e-6),
                "only the screen centre reconstructs on the view axis; uv {uv:?}"
            );
        });
    }

    #[test]
    fn view_pos_floors_the_ray_scale_rather_than_dividing_by_zero() {
        // A projection inverse whose reconstructed `-dir.z` is zero exercises the
        // `max( 1e-6, -dir.z )` floor; the result is finite, which is the point.
        let mut degenerate = [0.0_f32; 16];
        degenerate[15] = 1.0;
        let p = view_pos([0.5, 0.5], 3.0, &degenerate);
        assert!(
            p.iter().all(|c| c.is_finite()),
            "the 1e-6 floor must keep the reconstruction finite; got {p:?}"
        );
    }

    #[test]
    fn the_arc_integral_is_zero_on_a_closed_horizon_and_peaks_when_open() {
        // A surface facing the camera: n = 0, cosN = 1, sinN = 0.
        // Fully closed (h1 = h2 = 0) integrates to zero.
        let closed = arc(0.0, 0.0, 1.0, 0.0) + arc(0.0, 0.0, 1.0, 0.0);
        assert!(closed.abs() < 1e-6, "a closed arc must vanish; got {closed}");
        // Fully open (h = ±pi/2) integrates to 1 — a full unoccluded hemisphere.
        let half = core::f32::consts::FRAC_PI_2;
        let open = arc(-half, 0.0, 1.0, 0.0) + arc(half, 0.0, 1.0, 0.0);
        assert!(
            (open - 1.0).abs() < 1e-6,
            "an open arc on a camera-facing surface must integrate to 1; got {open}"
        );
        // Monotone between.
        let mid = arc(-half * 0.5, 0.0, 1.0, 0.0) + arc(half * 0.5, 0.0, 1.0, 0.0);
        assert!(
            (mid > closed) & (mid < open),
            "the arc must be monotone in the horizon angle; {closed} {mid} {open}"
        );
    }

    #[test]
    fn the_pixel_radius_clamps_at_both_ends_and_divides_by_depth() {
        let p11 = 1.732_050_8_f32; // 1 / tan(30 deg)
        // 1.35 m at 3 m on a 1080-line target: the source's own worked example,
        // 316 px, clamped to 128.
        let unclamped = 1.35 * p11 * 0.5 * 1080.0 / 3.0_f32;
        assert!(
            (unclamped - 420.7).abs() < 1.0,
            "the projected radius before clamping is {unclamped} px"
        );
        assert_eq!(radius_px(1.35, p11, 1080.0, 3.0), 128.0);
        // Far away: the 6 px floor.
        assert_eq!(radius_px(1.35, p11, 1080.0, 400.0), 6.0);
        // In between, it is a genuine 1/depth.
        let near = radius_px(1.35, p11, 1080.0, 20.0);
        let far = radius_px(1.35, p11, 1080.0, 40.0);
        assert!(
            (near / far - 2.0).abs() < 1e-4,
            "halving the depth must double the radius; {near} vs {far}"
        );
        // The 0.2 m floor keeps a point on the near plane finite.
        assert_eq!(radius_px(1.35, p11, 1080.0, 0.0), 128.0);
    }

    #[test]
    fn the_step_distribution_is_quadratic_with_a_one_pixel_floor() {
        // The source's claim: with 128 px and eight LINEAR steps the first sample
        // would be sixteen pixels out; quadratic puts the first three inside six.
        let linear_first: f32 = 128.0 * (1.0 / 8.0);
        assert!(
            (linear_first - 16.0).abs() < 1e-4,
            "the linear first step would be {linear_first} px"
        );
        let quad: Vec<f32> = (0..STEPS).map(|t| step_offset(t, 0.0, 128.0)).collect();
        assert_eq!(quad[0], 1.0, "the +1 px floor is the first tap");
        // The source's comment claims "the first three inside six pixels". At
        // zero jitter it is the first TWO (1 px, 3 px); the third lands at 9 px.
        // The prose overstates by one tap; the code is what is ported, and the
        // point it is making -- that the taps are packed at the origin where a
        // linear distribution had NONE inside sixteen pixels -- holds.
        assert_eq!(quad[1], 3.0);
        assert_eq!(quad[2], 9.0, "the third tap is at 9 px, not inside six");
        assert_eq!(
            quad[STEPS - 1],
            99.0,
            "the last tap must still reach most of the 128 px radius"
        );
        // Grouping: (radiusPx * ft) * ft, not radiusPx * (ft * ft).
        let ft = (3.0 + 0.375) / 8.0_f32;
        assert_eq!(step_offset(3, 0.375, 128.0), 128.0 * ft * ft + 1.0);
        // Monotone increasing.
        let ascending = quad.windows(2).all(|w| w[1] > w[0]);
        assert!(ascending, "the steps must march outward; got {quad:?}");
    }

    #[test]
    fn the_falloff_is_a_quartic_that_ignores_a_tap_at_the_radius() {
        let v = [0.0_f32, 0.0, 1.0];
        let inv_r2 = 1.0 / (1.35_f32 * 1.35);
        // A tap right on top of the shading point, directly toward the camera:
        // the horizon opens fully.
        let near = horizon_update(-1.0, [0.0, 0.0, 0.1], v, inv_r2);
        assert!(near > 0.99, "a near frontal tap must raise the horizon; got {near}");
        // The same direction at exactly the radius: `fall == 1`, `mix` returns the
        // incumbent, so the horizon is untouched. That is the thickness model.
        let at_radius = horizon_update(-1.0, [0.0, 0.0, 1.35], v, inv_r2);
        assert!(
            (at_radius + 1.0).abs() < 1e-5,
            "a tap at the full radius must contribute nothing; got {at_radius}"
        );
        // And beyond it, still nothing -- there the clamp makes it exact.
        assert_eq!(horizon_update(-1.0, [0.0, 0.0, 5.0], v, inv_r2), -1.0);
        // The `2e-5` guard: a coincident tap leaves the horizon alone rather than
        // producing a NaN from `inversesqrt( 0 )`.
        let coincident = horizon_update(-0.25, [0.0, 0.0, 0.0], v, inv_r2);
        assert_eq!(coincident, -0.25, "a zero-length ds must be skipped, not NaN");
        assert!(coincident.is_finite());
        // Quartic, not quadratic: at half the radius the blend weight is 1/16.
        let half = horizon_update(-1.0, [0.0, 0.0, 1.35 / 2.0], v, inv_r2);
        let expected = f32::max(-1.0, glsl_mix(1.0, -1.0, 0.25 * 0.25));
        assert!(
            (half - expected).abs() < 1e-5,
            "the falloff must be `clamp(len2/r2,0,1)` SQUARED; got {half} want {expected}"
        );
    }

    #[test]
    fn the_horizon_fold_seeds_at_minus_one_and_skips_rejected_taps() {
        let v = [0.0_f32, 0.0, 1.0];
        let inv_r2 = 1.0 / (1.35_f32 * 1.35);
        let all_rejected: Vec<Tap> = (0..STEPS)
            .map(|_| Tap {
                view_pos: [0.0, 0.0, 0.2],
                accepted: false,
            })
            .collect();
        assert_eq!(
            horizon(&all_rejected, [0.0, 0.0, 0.0], v, inv_r2),
            -1.0,
            "with every tap rejected the horizon must stay at its seed"
        );
        let one_accepted: Vec<Tap> = (0..STEPS)
            .map(|t| Tap {
                view_pos: [0.0, 0.0, 0.2],
                accepted: t == 3,
            })
            .collect();
        assert!(
            horizon(&one_accepted, [0.0, 0.0, 0.0], v, inv_r2) > -1.0,
            "one accepted tap must move the horizon"
        );
    }

    #[test]
    fn a_slice_frame_on_a_camera_facing_surface_has_a_full_projected_normal() {
        let normal = [0.0_f32, 0.0, 1.0];
        let v = [0.0_f32, 0.0, 1.0];
        let frame = slice_frame(normal, v, [1.0, 0.0]);
        assert!(
            (frame.proj_len - 1.0).abs() < 1e-6,
            "a normal lying in the slice plane projects whole; got {}",
            frame.proj_len
        );
        assert!(
            (frame.cos_n - 1.0).abs() < 1e-6,
            "cosN must be 1 when N == V; got {}",
            frame.cos_n
        );
        assert!(frame.n.abs() < 1e-3, "n must be ~0; got {}", frame.n);
        assert!(frame.sin_n.abs() < 1e-3);
    }

    #[test]
    fn a_slice_frame_weights_a_normal_that_leaves_its_plane_less() {
        let v = [0.0_f32, 0.0, 1.0];
        // A normal tilted out of the x-z slice plane: less of it survives.
        let tilted = {
            let l = (1.0_f32 + 1.0).sqrt();
            [0.0, 1.0 / l, 1.0 / l]
        };
        let in_plane = slice_frame([0.0, 0.0, 1.0], v, [1.0, 0.0]).proj_len;
        let out_of_plane = slice_frame(tilted, v, [1.0, 0.0]).proj_len;
        assert!(
            out_of_plane < in_plane,
            "the slice weight must fall as the normal leaves the plane; \
             {out_of_plane} vs {in_plane}"
        );
        // And a slice containing the tilt keeps it.
        let containing = slice_frame(tilted, v, [0.0, 1.0]).proj_len;
        assert!(
            (containing - 1.0).abs() < 1e-5,
            "a slice containing the normal keeps all of it; got {containing}"
        );
    }

    #[test]
    fn the_slice_azimuths_span_a_half_turn_in_slices_steps() {
        let dirs: Vec<[f32; 2]> = (0..SLICES).map(|s| slice_direction(s, 0.0)).collect();
        assert_eq!(dirs.len(), 3);
        assert!((dirs[0][0] - 1.0).abs() < 1e-6, "slice 0 is +x; got {:?}", dirs[0]);
        // Slice s is at s * pi / 3.
        (0..SLICES).for_each(|s| {
            let phi = s as f32 * (core::f32::consts::PI / 3.0);
            assert!((dirs[s][0] - phi.cos()).abs() < 1e-6);
            assert!((dirs[s][1] - phi.sin()).abs() < 1e-6);
        });
        // The noise rotates the whole set.
        let rotated = slice_direction(0, 0.5);
        assert!((rotated[0] - (core::f32::consts::PI / 6.0).cos()).abs() < 1e-6);
    }

    #[test]
    fn a_fully_open_slice_integrates_to_one_and_a_closed_one_to_zero() {
        let frame = slice_frame([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [1.0, 0.0]);
        // cosH = -1 on both sides means nothing was seen: h1 = -pi/2, h2 = +pi/2
        // after the clamp about n (which is ~0 here).
        let open = slice_visibility(-1.0, -1.0, &frame);
        assert!(
            (open - 1.0).abs() < 1e-4,
            "an unoccluded slice on a camera-facing surface integrates to 1; got {open}"
        );
        // cosH = 1 on both sides is a horizon straight at the camera: fully closed.
        let closed = slice_visibility(1.0, 1.0, &frame);
        assert!(
            closed.abs() < 1e-4,
            "a fully occluded slice integrates to 0; got {closed}"
        );
        // 0.5, not 0.0: acos(0) is exactly pi/2, which the clamp about `n` has
        // already pinned h1/h2 to, so cosH = 0 is indistinguishable from `open`.
        let partial = slice_visibility(0.5, 0.5, &frame);
        assert!(
            (partial > closed) & (partial < open),
            "{closed} {partial} {open}"
        );
    }

    #[test]
    fn a_degenerate_slice_contributes_nothing_rather_than_a_nan() {
        // A normal parallel to the slice axis projects to nothing: projLen ~ 0,
        // projNn is a division by ~0, and the source's `continue` is what saves it.
        let v = [0.0_f32, 0.0, 1.0];
        let frame = slice_frame([0.0, 1.0, 0.0], v, [1.0, 0.0]);
        assert!(
            frame.proj_len < 1e-4,
            "this configuration must be the degenerate one; projLen {}",
            frame.proj_len
        );
        let contribution = slice_visibility(-1.0, -1.0, &frame);
        assert_eq!(
            contribution, 0.0,
            "a skipped slice contributes exactly zero, not NaN; got {contribution}"
        );
        assert!(contribution.is_finite());
    }

    #[test]
    fn a_single_tilted_slice_is_allowed_past_one_and_only_the_sum_is_clamped() {
        // The source is emphatic that per-slice clamping biases the buffer dark.
        // A tilted surface's single slice does exceed 1.
        let v = [0.0_f32, 0.0, 1.0];
        let tilt = {
            let l = (0.6_f32 * 0.6 + 0.8 * 0.8).sqrt();
            [0.6 / l, 0.0, 0.8 / l]
        };
        let frame = slice_frame(tilt, v, [1.0, 0.0]);
        let one = slice_visibility(-1.0, -1.0, &frame);
        assert!(
            one > 1.0,
            "a tilted slice legitimately integrates past 1; got {one}"
        );
        // And the only clamp is after the divide, at 4.
        assert_eq!(resolve_visibility(3.0), 1.0);
        assert_eq!(resolve_visibility(30.0), 4.0);
        assert_eq!(resolve_visibility(-3.0), 0.0);
        assert!((resolve_visibility(1.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_temporal_weight_rejects_offscreen_history_outright() {
        [[-0.01_f32, 0.5], [1.01, 0.5], [0.5, -0.01], [0.5, 1.01]]
            .iter()
            .for_each(|huv| {
                let w = temporal_weight(0.92, *huv, 10.0, 10.0);
                assert_eq!(w, 0.0, "history at {huv:?} is off screen and must weigh 0");
            });
        // Exactly on the edge is INSIDE: the source rejects on `< 0.0` and
        // `> 1.0`, strictly.
        assert!(temporal_weight(0.92, [0.0, 1.0], 10.0, 10.0) > 0.0);
    }

    #[test]
    fn the_temporal_weight_fades_on_a_relative_depth_discrepancy() {
        let matched = temporal_weight(0.92, [0.5, 0.5], 10.0, 10.0);
        assert!(
            (matched - 0.92).abs() < 1e-6,
            "matched depths keep the full feedback; got {matched}"
        );
        // 3.33% relative is exp(-1) of the feedback.
        let one_e = temporal_weight(0.92, [0.5, 0.5], 10.0 + 1.0 / 3.0, 10.0);
        assert!(
            (one_e - 0.92 * (-1.0_f32).exp()).abs() < 1e-5,
            "a 1/30 relative depth step must cost one e-fold; got {one_e}"
        );
        // The same *absolute* step far away costs far less — that is the point of
        // the relative form.
        let far = temporal_weight(0.92, [0.5, 0.5], 100.0 + 1.0 / 3.0, 100.0);
        assert!(far > one_e * 2.0, "{far} vs {one_e}");
        // The 0.05 floor keeps it finite at the near plane.
        let at_zero = temporal_weight(0.92, [0.5, 0.5], 0.0, 0.0);
        assert!(at_zero.is_finite() & (at_zero > 0.0), "got {at_zero}");
    }

    #[test]
    fn the_neighbourhood_window_is_wide_and_seeded_from_the_centre() {
        // Nothing outside the window: the history passes through.
        assert_eq!(temporal_clamp(0.5, 0.5, [0.5; 4]), 0.5);
        // A history well below a stable neighbourhood is pulled up to mn - 0.45.
        let clamped = temporal_clamp(0.0, 0.9, [0.9; 4]);
        assert!(
            (clamped - 0.45).abs() < 1e-6,
            "the window is +/-0.45 around the neighbourhood; got {clamped}"
        );
        let high = temporal_clamp(3.0, 0.9, [0.9; 4]);
        assert!((high - 1.35).abs() < 1e-6, "got {high}");
        // The centre seeds min and max, so a lone centre extreme widens it.
        let widened = temporal_clamp(2.0, 1.4, [0.2; 4]);
        assert!((widened - 1.85).abs() < 1e-6, "got {widened}");
        assert_eq!(TEMPORAL_NEIGHBOUR_TEXELS.len(), 4);
        assert_eq!(TEMPORAL_NEIGHBOUR_TEXELS[0], [2.0, 0.0]);
        assert_eq!(TEMPORAL_NEIGHBOUR_TEXELS[3], [0.0, -2.0]);
    }

    #[test]
    fn the_temporal_blend_is_a_mix_toward_the_clamped_history() {
        assert_eq!(temporal_blend(0.2, 0.8, 0.0), 0.2);
        assert_eq!(temporal_blend(0.2, 0.8, 1.0), 0.8);
        assert!((temporal_blend(0.2, 0.8, 0.92) - (0.2 * 0.08 + 0.8 * 0.92)).abs() < 1e-7);
    }

    #[test]
    fn the_blur_distance_weights_are_a_reciprocal_ramp_not_a_gaussian() {
        assert!((blur_distance_weight(1) - 0.2).abs() < 1e-7);
        assert!((blur_distance_weight(2) - 0.4 / 3.0).abs() < 1e-7);
        assert!((blur_distance_weight(3) - 0.1).abs() < 1e-7);
        assert_eq!(BLUR_CENTRE_WEIGHT, 0.4);
        assert_eq!(BLUR_TAPS, 3);
    }

    #[test]
    fn the_blur_stops_at_a_depth_edge_and_passes_a_flat_surface() {
        // Flat: every tap keeps its full distance weight.
        let flat = blur_tap_weight(0.2, 10.0, 10.0);
        assert!((flat - 0.2).abs() < 1e-7, "got {flat}");
        // A 10 cm step at 10 m: 22 * 0.1 / 10 = 0.22 e-folds.
        let stepped = blur_tap_weight(0.2, 10.1, 10.0);
        assert!(
            (stepped - 0.2 * (-0.22_f32).exp()).abs() < 1e-6,
            "got {stepped}"
        );
        // A silhouette against the sky sentinel: exactly zero.
        assert_eq!(blur_tap_weight(0.2, 1.0e4, 10.0), 0.0);
        // The 0.1 m floor keeps a near-plane centre finite.
        assert!(blur_tap_weight(0.2, 0.0, 0.0).is_finite());
    }

    #[test]
    fn the_blur_resolves_a_flat_neighbourhood_to_its_own_value() {
        let centre = [0.6_f32, 10.0];
        let taps = [[[0.6_f32, 10.0]; 2]; BLUR_TAPS];
        let (sum, wsum) = blur_accumulate(centre, &taps);
        let ao = blur_output(sum, wsum, false, 1.1);
        assert!(
            (ao - 0.6).abs() < 1e-6,
            "a flat neighbourhood must resolve to its own value; got {ao}"
        );
        // The weight total is the seed plus two of each distance weight.
        let want = 0.4 + 2.0 * (0.2 + 0.4 / 3.0 + 0.1);
        assert!((wsum - want).abs() < 1e-6, "got {wsum} want {want}");
    }

    #[test]
    fn the_blur_ignores_a_neighbour_across_a_depth_edge() {
        let centre = [0.3_f32, 10.0];
        // Every neighbour is bright AND far behind: the bilateral must reject them.
        let taps = [[[1.0_f32, 1.0e4]; 2]; BLUR_TAPS];
        let (sum, wsum) = blur_accumulate(centre, &taps);
        let ao = blur_output(sum, wsum, false, 1.1);
        assert!(
            (ao - 0.3).abs() < 1e-6,
            "an edge must stop the blur completely; got {ao}"
        );
        assert_eq!(wsum, 0.4, "only the centre may carry weight; got {wsum}");
    }

    #[test]
    fn the_intensity_curve_runs_on_the_last_stage_only() {
        let sum = 0.25_f32;
        let wsum = 0.5_f32;
        // Horizontal pass: no clamp, no curve.
        assert_eq!(blur_output(sum, wsum, false, 1.1), 0.5);
        // Vertical pass: clamp then pow.
        assert!((blur_output(sum, wsum, true, 1.1) - 0.5_f32.powf(1.1)).abs() < 1e-7);
        // Above-one visibility survives the horizontal pass and is clamped by the
        // vertical one, which is why the order matters.
        assert_eq!(blur_output(2.0, 1.0, false, 1.1), 2.0);
        assert_eq!(blur_output(2.0, 1.0, true, 1.1), 1.0);
        // An intensity of 1 is the identity on the curve.
        assert!((blur_output(0.25, 0.5, true, 1.0) - 0.5).abs() < 1e-7);
    }

    #[test]
    fn the_chain_quantises_to_half_because_every_target_is_rg16f() {
        // 0.1 is not representable in f16.
        let stored = store_rg16f([0.1, 30.0]);
        assert_ne!(stored[0], 0.1_f32, "an f16 store must round 0.1");
        assert!((stored[0] - 0.1).abs() < 1e-4);
        // At street depths the depth channel's step is coarse enough to matter to
        // the blur's `22/d` weight.
        let a = store_rg16f([0.5, 30.0])[1];
        let b = store_rg16f([0.5, 30.0 + 0.004])[1];
        assert_eq!(a, b, "a 4 mm step at 30 m is below the f16 grid (1.6 cm)");
        let c = store_rg16f([0.5, 30.0 + 0.05])[1];
        assert_ne!(a, c, "a 5 cm step at 30 m is above it");
        // Exactly representable values are untouched.
        assert_eq!(store_rg16f([0.5, 0.25]), [0.5, 0.25]);
    }

    /// The whole core pass on a synthetic scene, composed from the parts above —
    /// the check that the *composition* is right, not just each function.
    ///
    /// A shading point on a flat plane facing the camera, with every tap of every
    /// slice placed on a wall right beside it, must come back heavily occluded;
    /// the same point with no accepted taps must come back fully open.
    #[test]
    fn the_composed_core_darkens_in_a_corner_and_opens_in_the_clear() {
        let p = [0.0_f32, 0.0, -3.0];
        let v = {
            let l = 3.0_f32;
            [0.0, 0.0, 3.0 / l]
        };
        let normal = [0.0_f32, 0.0, 1.0];
        let inv_r2 = 1.0 / (1.35_f32 * 1.35);

        let visibility = |accepted: bool| {
            let sum = (0..SLICES).fold(0.0_f32, |acc, s| {
                let frame = slice_frame(normal, v, slice_direction(s, 0.25));
                let taps: Vec<Tap> = (0..STEPS)
                    .map(|_| Tap {
                        // A wall 15 cm in front of the plane, well inside the radius.
                        view_pos: [0.0, 0.0, -3.0 + 0.15],
                        accepted,
                    })
                    .collect();
                let pos = horizon(&taps, p, v, inv_r2);
                let neg = horizon(&taps, p, v, inv_r2);
                acc + slice_visibility(neg, pos, &frame)
            });
            resolve_visibility(sum)
        };

        let clear = visibility(false);
        assert!(
            (clear - 1.0).abs() < 1e-3,
            "an unoccluded point must read ~1.0 visibility; got {clear}"
        );
        let corner = visibility(true);
        assert!(
            corner < 0.35,
            "a point 15 cm from a wall must read heavily occluded; got {corner}"
        );
    }
}
