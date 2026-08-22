//! **Parallax occlusion mapping** — the layer that justifies the whole split.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js`: `owPOM` in
//! `PARS_FRAGMENT`, its call site in `MAIN_FRAGMENT` (`Vp` / `vt` / `pFade`), and
//! the `parallax` / `parallaxFade` / `parallaxLayers` knobs of `DEFAULT_PARAMS`
//! (packed there as `owParallaxP = vec4(parallax, parallaxFade[0],
//! parallaxFade[1], parallaxLayers)`).
//!
//! POM marches the height field stored in the albedo map's alpha and returns the
//! displaced uv. It is **a bounded loop with a linear refine** — precisely the
//! shape the field algebra cannot express, and therefore the reason this shader
//! is hand-written WGSL rather than a `FieldGraph`. The loop stays a loop: the
//! `engine_no_branching` dylint reads Rust HIR, and a `for` inside a `&str` is
//! shader text, which is data. The **Rust** in this file — the CPU reference
//! included — has zero control flow.
//!
//! ## What the source actually does, in order
//!
//! 1. **Disabled is free.** `depth <= 0.0 || fade <= 0.001` returns `uv`
//!    untouched, before any sample. That early return is not an optimisation to
//!    fold away: `mix(uv, uv, w)` is `uv*(1-w) + uv*w`, which is *not* bit-equal
//!    to `uv`. A zero-depth surface must cost nothing and must return the uv it
//!    was given, bit for bit — so the CPU reference selects the untouched `uv`
//!    rather than letting a degenerate march produce it.
//! 2. **Layer count follows the grazing angle**, `mix(layers, 8.0, |vt.z|)`:
//!    face-on is cheap (8), grazing pays the authored `parallaxLayers`. Then
//!    `max(nl * fade, 4.0)` — distance thins the march to a floor of four.
//! 3. **The march.** `dUv = ((vt.xy / max(|vt.z|, 0.30)) * depth * fade) / nl`,
//!    stepping the uv against the view while `cur` climbs one `layer` per step.
//!    Its exit is `cur >= d || float(i) >= nl` inside a hard cap of 48
//!    iterations — the cap, the `>=`, and `i` counting from zero are the
//!    algorithm, not decoration. An off-by-one changes every silhouette.
//! 4. **The linear refine.** `after` is the signed gap at the step we landed on,
//!    `before` the gap one step back; `w = clamp(after / max(after - before,
//!    1e-4), 0, 1)` and the answer is `mix(c, prev, w)`. The two-sample lerp is
//!    left exactly as grouped: float arithmetic is not associative and a march
//!    accumulates, so a tidied expression diverges over the steps.
//!
//! ## The refine is a step, not a lerp — a defect ported verbatim
//!
//! Read that weight carefully. At a genuine crossing the loop exits with
//! `cur >= d`, so `after = d - cur <= 0`; the step before it had `cur' < d'`, so
//! `before = d' - cur' > 0`. Their difference is therefore **negative**, and
//! `max(after - before, 1e-4)` — a guard whose sign is wrong for the quantity it
//! guards — replaces it with `+1e-4`. The weight becomes a large *negative*
//! number and clamps to `0`. Symmetrically, when the march runs out of layers
//! with `after > 0`, the weight clamps to `1`. So `w` is only ever `0` or `1`
//! (barring a `0 < after < 1e-4` sliver), and `mix(c, prev, w)` returns one of
//! its two endpoints: the intersection is never interpolated.
//!
//! The textbook form divides by `after - before` unguarded, or guards it with a
//! *negative* floor (`min(after - before, -1e-4)`), and lands between the two
//! samples. The source does not, and this port does not "fix" it: the whole
//! point of a port is that the pixels match, and a POM silhouette that
//! interpolates where the original snapped is a different image. The finding is
//! recorded in `docs/work-manifests/shmup-port/notes/material-pom.md` for
//! whoever decides whether Axiom's own materials should keep the source's
//! behaviour or the textbook one — that is a product decision, not a
//! transcription one.
//!
//! One consequence is worth stating because it shapes the test: with `w`
//! saturated and every step an exact multiple of the step vector, the returned
//! uv is a *bit-exact* accumulation, so the parity below asserts equality rather
//! than a tolerance for the march. What the comparison really pins is the step
//! count and the exit condition — which is where the algorithm actually lives.
//!
//! ## `textureGrad`, not `textureSample`
//!
//! The source samples through `OW_TEX`, which is `textureGrad( t, uv, dx, dy )`,
//! and its comment says why: *"Explicit-gradient sampling keeps the mip selection
//! correct through the parallax march."* Implicit derivatives in non-uniform
//! control flow are undefined — the loop is exactly that — so `ddx`/`ddy` are
//! computed once at the call site and threaded in. They stay explicit here.
//!
//! ## Calling convention
//!
//! Free functions taking explicit arguments, textures and samplers included
//! (WGSL permits both as function parameters). `owPOM` reads the `owParallaxP.w`
//! uniform for its layer count; here that arrives as the `layers` argument. The
//! orchestrator wires the bindings and supplies the tangent frame from the
//! `frames` layer.

use axiom_math::{Vec2, Vec3};

/// The WGSL for this layer: the fade curve, the tangent-space view direction,
/// and the march itself.
///
/// Transcribed from the GLSL text of `shader.js`, not from the Rust below —
/// they are two readings of one source, which is the only way the pair can
/// catch a misreading.
pub(crate) const POM_WGSL: &str = r#"
// --- parallax occlusion mapping (Claude-of-Duty materials/shader.js owPOM) ---

// MAIN_FRAGMENT: `float pFade = 1.0 - smoothstep( owParallaxP.y, owParallaxP.z, owDist );`
// owDist is `length( vViewPosition )`; y/z are parallaxFade[0]/[1].
fn axiom_pom_fade(view_distance: f32, fade_near: f32, fade_far: f32) -> f32 {
    return 1.0 - smoothstep(fade_near, fade_far, view_distance);
}

// MAIN_FRAGMENT:
//   vec3 Vp = normalize( vViewPosition );   // or normalize( vOwViewDirP )
//   vec3 vt = normalize( vec3( dot( Vp, f.T ), dot( Vp, f.B ), dot( Vp, f.N ) ) );
// `view_dir` points from the fragment towards the eye, so vt.z > 0 face-on.
fn axiom_pom_view_tangent(
    view_dir: vec3<f32>,
    tangent: vec3<f32>,
    bitangent: vec3<f32>,
    normal: vec3<f32>,
) -> vec3<f32> {
    let vp = normalize(view_dir);
    return normalize(vec3<f32>(dot(vp, tangent), dot(vp, bitangent), dot(vp, normal)));
}

// PARS_FRAGMENT: `vec2 owPOM( vec2 uv, vec3 vt, vec2 ddx, vec2 ddy, float depth, float fade )`.
// `layers` is the uniform owParallaxP.w the source reads directly; the height is
// the alpha of the albedo map, and OW_TEX expands to textureGrad.
fn axiom_pom(
    uv: vec2<f32>,
    vt: vec3<f32>,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
    depth: f32,
    fade: f32,
    layers: f32,
    height_map: texture_2d<f32>,
    height_sampler: sampler,
) -> vec2<f32> {
    if (depth <= 0.0 || fade <= 0.001) { return uv; }
    var nl = mix(layers, 8.0, clamp(abs(vt.z), 0.0, 1.0));
    nl = max(nl * fade, 4.0);
    let layer = 1.0 / nl;
    let P = (vt.xy / max(abs(vt.z), 0.30)) * depth * fade;
    let d_uv = P * layer;

    var cur = 0.0;
    var c = uv;
    var d = 1.0 - textureSampleGrad(height_map, height_sampler, c, ddx, ddy).a;
    for (var i = 0; i < 48; i = i + 1) {
        if (cur >= d || f32(i) >= nl) { break; }
        c = c - d_uv;
        d = 1.0 - textureSampleGrad(height_map, height_sampler, c, ddx, ddy).a;
        cur = cur + layer;
    }
    let prev = c + d_uv;
    let after = d - cur;
    let before = (1.0 - textureSampleGrad(height_map, height_sampler, prev, ddx, ddy).a) - cur + layer;
    let w = clamp(after / max(after - before, 1e-4), 0.0, 1.0);
    return mix(c, prev, w);
}
"#;

/// The hard iteration cap the source writes as `for ( int i = 0; i < 48; i ++ )`.
/// A bound, not a layer count: `nl` almost always ends the march first.
pub(crate) const POM_MAX_STEPS: u32 = 48;

/// What one march produced, and how well-conditioned it was.
///
/// [`PomMarch::uv`] is the algorithm's answer. The other three are the march's
/// own conditioning, and they exist because a POM parity comparison is only
/// meaningful if the two sides took the *same* number of steps: the exit test is
/// a float comparison, so a fixture whose `cur` and `d` cross by a hair proves
/// nothing about the maths and everything about the hardware's last mantissa
/// bit. The tests assert on all four, and print the whole report — which is what
/// `Debug` is for here, and why it is the only derive: a `Clone`, `Copy` or
/// `PartialEq` nothing calls is an uncovered function, and the Coverage Law does
/// not make an exception for code a macro wrote.
#[derive(Debug)]
pub(crate) struct PomMarch {
    /// The displaced uv — what `owPOM` returns.
    pub(crate) uv: Vec2,
    /// `nl`: the effective layer count after the grazing-angle mix, the fade
    /// scale and the floor of four. Zero when the effect is disabled.
    pub(crate) layer_count: f32,
    /// How many times the loop body ran before its exit condition held.
    pub(crate) steps: u32,
    /// The smallest `|cur - d|` any iteration's exit test saw — how far the
    /// march was from taking a different number of steps. Infinite when the
    /// effect is disabled, because then no test ran.
    pub(crate) margin: f32,
}

/// GLSL/WGSL `clamp(x, 0.0, 1.0)`, whose spec expansion is `min(max(x, 0.0),
/// 1.0)`. `f32::clamp` agrees with that expansion for every finite `x`, which is
/// every value that reaches it here: the march's inputs are a texture sample in
/// `0..=1`, a layer depth built from it, and a normalised cosine.
fn saturate(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// GLSL/WGSL `mix(a, b, t)`: `a * (1 - t) + b * t`, in that grouping.
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

/// GLSL/WGSL `smoothstep(e0, e1, x)`. Degenerate edges (`e0 == e1`) divide by
/// zero on both sides, exactly as the source does.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = saturate((x - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}

/// GLSL `normalize(v)`: `v / length(v)`, a division per lane — deliberately not
/// a reciprocal-multiply, which is the single most common transcription defect
/// this port has found.
fn normalize3(v: Vec3) -> Vec3 {
    let len = v.dot(v).sqrt();
    Vec3::new(v.x / len, v.y / len, v.z / len)
}

/// The distance fade, `1 - smoothstep(near, far, dist)`: 1 up close, 0 past
/// `far`. The source multiplies both the layer count and the uv sweep by it, so
/// the effect thins out rather than popping.
pub(crate) fn pom_fade(view_distance: f32, fade_near: f32, fade_far: f32) -> f32 {
    1.0 - smoothstep(fade_near, fade_far, view_distance)
}

/// The tangent-space view direction the march steps against: the eye vector
/// normalised, projected onto the frame, then normalised again. Both
/// normalisations are in the source and neither is redundant — the frame is
/// orthonormal only after `owOrthonormalise`, and the second `normalize` is what
/// makes `vt.z` a cosine.
pub(crate) fn pom_view_tangent(
    view_dir: Vec3,
    tangent: Vec3,
    bitangent: Vec3,
    normal: Vec3,
) -> Vec3 {
    let vp = normalize3(view_dir);
    normalize3(Vec3::new(
        vp.dot(tangent),
        vp.dot(bitangent),
        vp.dot(normal),
    ))
}

/// One step of the march, carried through a `fold` because the Rust here may not
/// loop. `running` is the source's exit test inverted; once it is false the
/// state is frozen, so the test stays false for every later iteration and the
/// fold is exactly the `break`.
struct MarchState {
    c: Vec2,
    d: f32,
    cur: f32,
    steps: u32,
    margin: f32,
}

/// **The CPU reference.** `owPOM`, evaluated in `f32` — the same storage width
/// the GPU uses, so the only differences left between the two are the hardware's
/// (an `fma` contraction, a differently-factored `mix`).
///
/// `height` is the alpha of the albedo map at level 0. `ddx`/`ddy` do not appear:
/// on the GPU they select the mip, and the parity fixture pins that choice by
/// marching at level 0. What they must *not* do is disappear from the WGSL, and
/// `the_march_honours_the_gradients_it_is_given` is the test that says they
/// reach the sampler.
pub(crate) fn pom(
    uv: Vec2,
    vt: Vec3,
    depth: f32,
    fade: f32,
    layers: f32,
    height: impl Fn(Vec2) -> f32,
) -> PomMarch {
    let nl = (mix(layers, 8.0, saturate(vt.z.abs())) * fade).max(4.0);
    let layer = 1.0 / nl;
    // `( vt.xy / max( abs( vt.z ), 0.30 ) ) * depth * fade` — left to right.
    let q = vt.z.abs().max(0.30);
    let sweep = Vec2::new(vt.x / q, vt.y / q).mul_scalar(depth).mul_scalar(fade);
    let d_uv = sweep.mul_scalar(layer);

    let start = MarchState {
        c: uv,
        d: 1.0 - height(uv),
        cur: 0.0,
        steps: 0,
        margin: f32::INFINITY,
    };
    let end = (0..POM_MAX_STEPS).fold(start, |state, i| {
        let running = (state.cur < state.d) & ((i as f32) < nl);
        let stepped_c = state.c.subtract(d_uv);
        let taken = [
            (state.c, state.d, state.cur),
            (stepped_c, 1.0 - height(stepped_c), state.cur + layer),
        ][usize::from(running)];
        MarchState {
            c: taken.0,
            d: taken.1,
            cur: taken.2,
            steps: state.steps + u32::from(running),
            margin: state.margin.min((state.cur - state.d).abs()),
        }
    });

    let prev = end.c.add(d_uv);
    let after = end.d - end.cur;
    let before = (1.0 - height(prev)) - end.cur + layer;
    // `max( after - before, 1e-4 )` — the source's guard, sign and all. See the
    // module header: it saturates `w` to 0 or 1, so this "lerp" is a step.
    let w = saturate(after / (after - before).max(1.0e-4));
    let marched = Vec2::new(mix(end.c.x, prev.x, w), mix(end.c.y, prev.y, w));

    // `if ( depth <= 0.0 || fade <= 0.001 ) return uv;` — the untouched uv, bit
    // for bit, and a march the GPU never runs.
    let off = usize::from((depth <= 0.0) | (fade <= 0.001));
    PomMarch {
        uv: [marched, uv][off],
        layer_count: [nl, 0.0][off],
        steps: [end.steps, 0][off],
        margin: [end.margin, f32::INFINITY][off],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parity fixture's height field, as a pure function of the uv: a 16x16
    /// `Rgba8Unorm` texture whose alpha is `255 - 17 * x` — a staircase down the
    /// u axis, flat along v — sampled NEAREST with clamp-to-edge. Documented in
    /// `docs/work-manifests/shmup-port/notes/material-pom.md`.
    pub(super) const HEIGHT_DIM: i32 = 16;

    /// The stored byte of column `x`. Full height (`alpha = 255`, depth `0`) at
    /// `x = 0` and a full-depth well (`alpha = 0`, depth `1`) at `x = 15`, so
    /// marching towards lower u walks *out* of the well and produces a real
    /// crossing, while marching the other way runs into a wall the layer budget
    /// can never reach.
    pub(super) fn height_byte(x: i32) -> u8 {
        (255 - 17 * x.clamp(0, HEIGHT_DIM - 1)) as u8
    }

    /// Nearest-filtered clamp-to-edge lookup of that texture's alpha. `k / 255`
    /// is the exact unorm8 decode the GPU performs.
    pub(super) fn height_at(uv: Vec2) -> f32 {
        f32::from(height_byte((uv.x * HEIGHT_DIM as f32).floor() as i32)) / 255.0
    }

    /// A march that crosses the height field: eight columns in, stepping one
    /// whole texel per layer.
    fn crossing() -> (Vec2, Vec3, f32, f32, f32) {
        // vt.z = 0.5 => nl = mix(24, 8, 0.5) = 16 exactly, so layer = 1/16 and
        // q = 0.5; vt.x = 0.5, depth = 1 => dUv.x = 0.5/0.5*1*(1/16) = 1/16 =
        // exactly one texel. Start at a texel centre, 14.5/16.
        (
            Vec2::new(0.90625, 0.53125),
            Vec3::new(0.5, 0.0, 0.5),
            1.0,
            1.0,
            24.0,
        )
    }

    #[test]
    fn a_zero_depth_surface_returns_the_uv_bit_for_bit_and_marches_not_at_all() {
        let (uv, vt, _, fade, layers) = crossing();
        let march = pom(uv, vt, 0.0, fade, layers, height_at);
        assert_eq!(march.uv, uv);
        assert_eq!(march.uv.x.to_bits(), uv.x.to_bits());
        assert_eq!(march.uv.y.to_bits(), uv.y.to_bits());
        assert_eq!(march.steps, 0);
        assert_eq!(march.layer_count, 0.0);
        assert_eq!(march.margin, f32::INFINITY);
        // Negative depth is disabled too: the source tests `<= 0.0`.
        assert_eq!(pom(uv, vt, -1.0, fade, layers, height_at).uv, uv);
    }

    #[test]
    fn the_fade_floor_is_inclusive_at_one_thousandth() {
        let (uv, vt, depth, _, layers) = crossing();
        // `fade <= 0.001` — 0.001 itself is off.
        assert_eq!(pom(uv, vt, depth, 0.001, layers, height_at).uv, uv);
        // A hair above it is on, and moves the uv.
        let on = pom(uv, vt, depth, 0.0011, layers, height_at);
        assert_ne!(on.uv, uv);
        assert!(on.steps > 0);
    }

    #[test]
    fn the_layer_count_follows_the_grazing_angle_and_floors_at_four() {
        let uv = Vec2::new(0.90625, 0.53125);
        // Face-on (|vt.z| = 1) is the cheap end: mix(24, 8, 1) = 8.
        let face_on = pom(uv, Vec3::new(0.0, 0.0, 1.0), 1.0, 1.0, 24.0, height_at);
        assert_eq!(face_on.layer_count, 8.0);
        // Grazing (|vt.z| = 0) pays the authored count: mix(24, 8, 0) = 24.
        let grazing = pom(uv, Vec3::new(1.0, 0.0, 0.0), 1.0, 1.0, 24.0, height_at);
        assert_eq!(grazing.layer_count, 24.0);
        // The clamp bites: |vt.z| = 3 is still the face-on end, not extrapolated.
        let beyond = pom(uv, Vec3::new(0.0, 0.0, -3.0), 1.0, 1.0, 24.0, height_at);
        assert_eq!(beyond.layer_count, 8.0);
        // And `max( nl * fade, 4.0 )`: a far-away surface thins to four layers.
        let faded = pom(uv, Vec3::new(1.0, 0.0, 0.0), 1.0, 0.01, 24.0, height_at);
        assert_eq!(faded.layer_count, 4.0);
    }

    #[test]
    fn the_march_walks_the_height_field_until_the_layer_depth_crosses_it() {
        let (uv, vt, depth, fade, layers) = crossing();
        let march = pom(uv, vt, depth, fade, layers, height_at);
        // The march is carried into each assertion message below rather than
        // printed: no layer or module in this engine emits console output,
        // tests included (Module Law #10, enforced by the architecture
        // checker), and a value only a human reading `--nocapture` would see
        // is not evidence anyway.
        let ctx = format!("crossing march: {march:?}");
        assert_eq!(march.layer_count, 16.0, "{ctx}");
        // Hand-computed: the start column is 14, depth 1 - 17/255; each texel
        // towards lower u sheds 17/255 of depth while `cur` climbs 1/16, so
        // `cur >= d` first holds after eight steps.
        assert_eq!(march.steps, 8);
        // And the answer is exactly the point it stopped on — `w` saturates to
        // zero, because the source's `max( after - before, 1e-4 )` has the wrong
        // sign for a quantity that is negative at every crossing.
        let step = 1.0 / 16.0;
        assert_eq!(march.uv.x, uv.x - 8.0 * step);
        // Flat along v: with vt.y = 0 the sweep never leaves the row.
        assert_eq!(march.uv.y, uv.y);
        // The exit test was nowhere near a coin flip.
        assert!(march.margin > 0.02, "margin {}", march.margin);
    }

    #[test]
    fn a_march_that_runs_out_of_layers_stops_on_the_layer_clause() {
        // nl floors at four; the well is deeper than four layers of 1/4 reach
        // before `float( i ) >= nl` ends it. With `d <= 1` by construction, the
        // two clauses go true together at i = nl — the layer clause is the
        // source's safety net, not an independent exit.
        let uv = Vec2::new(0.96875, 0.53125);
        let march = pom(uv, Vec3::new(1.0, 0.0, 0.5), 2.0, 0.0625, 24.0, height_at);
        assert_eq!(march.layer_count, 4.0);
        assert_eq!(march.steps, 4);
        assert_eq!(march.uv.x, uv.x - 4.0 * 0.0625);
    }

    #[test]
    fn the_hard_cap_is_forty_eight_iterations_and_it_is_where_the_refine_saturates_high() {
        // 200 authored layers at 45 degrees: nl = 104, so the loop's own bound
        // stops it first. Marching *up* the staircase runs into the full-depth
        // wall, which forty-eight layers of 1/104 cannot reach — so this is the
        // one exit where `after > 0` and the weight clamps to one, returning the
        // step *before* the one it stopped on.
        let uv = Vec2::new(0.53125, 0.53125);
        let march = pom(uv, Vec3::new(-3.25, 0.0, 0.5), 1.0, 1.0, 200.0, height_at);
        assert_eq!(march.layer_count, 104.0);
        assert_eq!(march.steps, POM_MAX_STEPS);
        assert_eq!(march.uv.x, uv.x + 47.0 * 0.0625);
        assert!(march.margin > 0.5, "margin {}", march.margin);
    }

    #[test]
    fn a_surface_already_at_full_height_exits_before_the_loop_body_runs() {
        // Column zero stores alpha 255, so d is exactly zero and `cur >= d`
        // holds on the very first test. No step, no displacement.
        let uv = Vec2::new(0.03125, 0.53125);
        let march = pom(uv, Vec3::new(0.5, 0.0, 0.5), 1.0, 1.0, 24.0, height_at);
        assert_eq!(march.steps, 0);
        assert_eq!(march.margin, 0.0);
        assert_eq!(march.uv, uv);
    }

    #[test]
    fn the_fade_curve_is_one_up_close_and_zero_past_the_far_edge() {
        assert_eq!(pom_fade(0.0, 6.0, 14.0), 1.0);
        assert_eq!(pom_fade(6.0, 6.0, 14.0), 1.0);
        assert_eq!(pom_fade(14.0, 6.0, 14.0), 0.0);
        assert_eq!(pom_fade(100.0, 6.0, 14.0), 0.0);
        // Halfway is exactly a half: smoothstep(0.5) = 0.25 * (3 - 1) = 0.5.
        assert_eq!(pom_fade(10.0, 6.0, 14.0), 0.5);
        // A quarter of the way in is the cubic, not the line.
        assert!((pom_fade(8.0, 6.0, 14.0) - (1.0 - 0.15625)).abs() < 1.0e-7);
    }

    #[test]
    fn the_tangent_view_direction_is_the_eye_vector_in_the_frame() {
        let vt = pom_view_tangent(
            Vec3::new(0.0, 0.0, 4.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        assert_eq!(vt, Vec3::new(0.0, 0.0, 1.0));
        // Off-axis: the lane order is (T, B, N), and the result is unit length.
        let tilted = pom_view_tangent(
            Vec3::new(3.0, 0.0, 4.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        assert!((tilted.x - 0.6).abs() < 1.0e-6);
        assert_eq!(tilted.y, 0.0);
        assert!((tilted.z - 0.8).abs() < 1.0e-6);
        assert!((tilted.dot(tilted) - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn the_wgsl_declares_the_three_entry_points_the_orchestrator_wires() {
        assert!(POM_WGSL.contains("fn axiom_pom_fade(view_distance: f32, fade_near: f32, fade_far: f32) -> f32"));
        assert!(POM_WGSL.contains("fn axiom_pom_view_tangent("));
        assert!(POM_WGSL.contains("fn axiom_pom("));
        // The texture and sampler are parameters, not globals: that is what lets
        // the orchestrator own the binding indices.
        assert!(POM_WGSL.contains("height_map: texture_2d<f32>"));
        assert!(POM_WGSL.contains("height_sampler: sampler"));
        // Explicit gradients, never an implicit derivative inside the loop.
        assert!(POM_WGSL.contains("textureSampleGrad("));
        assert!(!POM_WGSL.contains("textureSample("));
        // The bound, the exit test and the disabled guard, verbatim.
        assert!(POM_WGSL.contains("for (var i = 0; i < 48; i = i + 1)"));
        assert!(POM_WGSL.contains("if (cur >= d || f32(i) >= nl) { break; }"));
        assert!(POM_WGSL.contains("if (depth <= 0.0 || fade <= 0.001) { return uv; }"));
    }
}

/// **CPU↔GPU parity on a real adapter**, in the shape `surface_program::parity`
/// establishes: compiled only under `--features offscreen`, and it asserts an
/// adapter was acquired rather than skipping, because a parity test that passes
/// when nothing ran proves nothing.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::tests::{height_at, height_byte, HEIGHT_DIM};
    use super::*;

    /// One fragment per case; also the target's width.
    const SAMPLES: usize = 16;

    /// Four `vec4` of input per case.
    const LANES: usize = 4;

    /// The march's budget is **zero**, and that is a measurement, not a hope.
    /// Every quantity it accumulates is exact: `nl` is an exact mix of exact
    /// values, `layer = 1/nl`, `dUv = P * layer` is one correctly-rounded
    /// multiply, the march is repeated subtraction, and the refine's weight
    /// saturates to 0 or 1 so `mix` returns an endpoint untouched. There is
    /// nothing left for an `fma` contraction or a reassociation to change — the
    /// only thing that could move the answer is a *different step count*, which
    /// is exactly what this comparison exists to catch, and which would show up
    /// as a whole texel, not a ULP. So the march is asserted bit-equal.
    const MARCH_TOLERANCE: f32 = 0.0;

    /// The call site's helpers are a different matter: `normalize` is commonly a
    /// reciprocal-square-root on hardware and a divide on the CPU, and
    /// `smoothstep` is three roundings either way. Measured worst delta on the
    /// development adapter: **1.19e-7**, which is `2^-23` — one ULP at 1.0, the
    /// smallest disagreement an `f32` can express there. This is ~2x that, and
    /// the test prints the measurement, so a looser adapter is a visible
    /// regression rather than a silent one.
    const FRAME_TOLERANCE: f32 = 2.5e-7;

    /// The scale a "did the uv actually move" check is judged against — the
    /// tolerance the march would have had if it were not exact.
    const TOLERANCE: f32 = 1.0e-7;

    /// `copy_texture_to_buffer` row alignment.
    const ROW_ALIGN: u32 = 256;

    const HARNESS_WGSL: &str = r#"
struct PomIn { items: array<vec4<f32>, 64> };
@group(0) @binding(0) var<uniform> pom_in: PomIn;
@group(0) @binding(1) var pom_height: texture_2d<f32>;
@group(0) @binding(2) var pom_sampler: sampler;

@vertex
fn pom_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

// uv.xy | depth, fade  ..  vt.xyz | layers  ..  ddx.xy, ddy.xy
@fragment
fn pom_uv_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x) * 4u;
    let a = pom_in.items[i + 0u];
    let b = pom_in.items[i + 1u];
    let c = pom_in.items[i + 2u];
    let out = axiom_pom(a.xy, b.xyz, c.xy, c.zw, a.z, a.w, b.w, pom_height, pom_sampler);
    return vec4<f32>(out.x, out.y, 0.0, 0.0);
}

// view_dir.xyz | dist  ..  T.xyz | near  ..  B.xyz | far  ..  N.xyz
@fragment
fn pom_frame_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x) * 4u;
    let a = pom_in.items[i + 0u];
    let b = pom_in.items[i + 1u];
    let c = pom_in.items[i + 2u];
    let d = pom_in.items[i + 3u];
    let vt = axiom_pom_view_tangent(a.xyz, b.xyz, c.xyz, d.xyz);
    return vec4<f32>(vt.x, vt.y, vt.z, axiom_pom_fade(a.w, b.w, c.w));
}
"#;

    /// One march case. The parameters are **dyadic on purpose**: `vt.z` is 0.5 or
    /// 1.0 so `nl` is an exact 16 or 8, `q = max(|vt.z|, 0.30)` is exact, and
    /// `vt.x * depth` is chosen so `dUv.x` is a whole number of texels. Every
    /// sample therefore lands on a texel *centre*, and the nearest-filter lookup
    /// cannot round differently on the two sides. Without that, a 1-ULP
    /// difference in the accumulation would flip a texel and the test would be
    /// measuring the sampler's rounding rather than the algorithm.
    struct Case {
        uv: Vec2,
        vt: Vec3,
        depth: f32,
        fade: f32,
        layers: f32,
    }

    /// The sixteen cases: every exit the algorithm has (a crossing, the layer
    /// clause, the hard cap, an immediate exit at full height), both ends of the
    /// grazing mix, one, two and three texels a layer, a v-axis sweep, two fade
    /// scales, the layer floor, and the two disabled forms — which must come
    /// back bit-identical.
    fn cases() -> Vec<Case> {
        vec![
            // 0-2: nl = 16 (vt.z = 0.5, q = 0.5), dUv.x = vt.x * depth / 8 = one texel.
            Case { uv: Vec2::new(0.90625, 0.53125), vt: Vec3::new(0.5, 0.0, 0.5), depth: 1.0, fade: 1.0, layers: 24.0 },
            Case { uv: Vec2::new(0.96875, 0.53125), vt: Vec3::new(0.5, 0.0, 0.5), depth: 1.0, fade: 1.0, layers: 24.0 },
            Case { uv: Vec2::new(0.84375, 0.09375), vt: Vec3::new(0.5, 0.0, 0.5), depth: 1.0, fade: 1.0, layers: 24.0 },
            // 3: two texels a layer.
            Case { uv: Vec2::new(0.96875, 0.53125), vt: Vec3::new(1.0, 0.0, 0.5), depth: 1.0, fade: 1.0, layers: 24.0 },
            // 4: three texels a layer.
            Case { uv: Vec2::new(0.96875, 0.71875), vt: Vec3::new(0.75, 0.0, 0.5), depth: 2.0, fade: 1.0, layers: 24.0 },
            // 5: sweeping v as well; the row is flat, so only the returned uv.y moves.
            Case { uv: Vec2::new(0.90625, 0.53125), vt: Vec3::new(0.5, 0.25, 0.5), depth: 1.0, fade: 1.0, layers: 24.0 },
            // 6: the hard cap. nl = 104, marching up into the full-depth wall,
            // which forty-eight layers of 1/104 never reach. The one case where
            // `after > 0` and the weight saturates HIGH.
            Case { uv: Vec2::new(0.53125, 0.53125), vt: Vec3::new(-3.25, 0.0, 0.5), depth: 1.0, fade: 1.0, layers: 200.0 },
            // 7-8: nl = 8 (face-on), q = 1.0, dUv.x = vt.x * depth / 8.
            Case { uv: Vec2::new(0.90625, 0.53125), vt: Vec3::new(0.5, 0.0, 1.0), depth: 1.0, fade: 1.0, layers: 24.0 },
            Case { uv: Vec2::new(0.96875, 0.28125), vt: Vec3::new(1.0, 0.0, 1.0), depth: 1.0, fade: 1.0, layers: 24.0 },
            // 9: fade = 0.5 halves nl (32 -> 16) and leaves dUv untouched, because
            // the sweep scales by fade and the layer by 1/fade.
            Case { uv: Vec2::new(0.90625, 0.53125), vt: Vec3::new(1.0, 0.0, 0.5), depth: 1.0, fade: 0.5, layers: 56.0 },
            // 10: fade = 0.25 with nl_pre = 64 -> nl = 16.
            Case { uv: Vec2::new(0.96875, 0.46875), vt: Vec3::new(1.0, 0.0, 0.5), depth: 2.0, fade: 0.25, layers: 120.0 },
            // 11: the floor. nl_pre * fade = 1, clamped up to 4.
            Case { uv: Vec2::new(0.96875, 0.53125), vt: Vec3::new(1.0, 0.0, 0.5), depth: 2.0, fade: 0.0625, layers: 24.0 },
            // 12: starting at full height, where d = 0 and the loop breaks at i = 0.
            Case { uv: Vec2::new(0.03125, 0.53125), vt: Vec3::new(0.5, 0.0, 0.5), depth: 1.0, fade: 1.0, layers: 24.0 },
            // 13: a negative v sweep from mid-staircase.
            Case { uv: Vec2::new(0.71875, 0.53125), vt: Vec3::new(0.5, -0.125, 0.5), depth: 1.0, fade: 1.0, layers: 24.0 },
            // 14-15: disabled — zero depth, and the fade floor. Both return uv untouched.
            Case { uv: Vec2::new(0.90625, 0.53125), vt: Vec3::new(0.5, 0.0, 0.5), depth: 0.0, fade: 1.0, layers: 24.0 },
            Case { uv: Vec2::new(0.90625, 0.53125), vt: Vec3::new(0.5, 0.0, 0.5), depth: 1.0, fade: 0.001, layers: 24.0 },
        ]
    }

    /// The frame helpers' cases: `(view_dir, T, B, N, dist, near, far)`.
    fn frame_cases() -> Vec<(Vec3, Vec3, Vec3, Vec3, f32, f32, f32)> {
        (0..SAMPLES)
            .map(|index| {
                let t = index as f32;
                (
                    Vec3::new(t * 0.31 - 2.0, t * -0.17 + 1.5, t * 0.23 + 0.75),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                    Vec3::new(0.0, 0.0, 1.0),
                    t * 1.4,
                    6.0,
                    14.0,
                )
            })
            .collect()
    }

    /// A real GPU, or a loud failure.
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
            Gpu {
                device,
                queue,
                backend: gpu.backend,
            }
        }

        /// The fixture texture: 16x16 `Rgba8Unorm`, two mip levels. Level 0 is
        /// the staircase `alpha = 255 - 15 * x`, flat along v. Level 1 is solid
        /// `alpha = 255` — a wall of full height, which makes a mip switch
        /// unmistakable in the output.
        fn height_texture(&self) -> wgpu::Texture {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-pom-height"),
                size: wgpu::Extent3d {
                    width: HEIGHT_DIM as u32,
                    height: HEIGHT_DIM as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 2,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            [0_u32, 1].iter().for_each(|level| {
                let dim = HEIGHT_DIM as u32 >> level;
                let pixels: Vec<u8> = (0..dim * dim)
                    .flat_map(|texel| {
                        let x = (texel % dim) as i32;
                        [0, 0, 0, [height_byte(x), 255][*level as usize]]
                    })
                    .collect();
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: *level,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(dim * 4),
                        rows_per_image: Some(dim),
                    },
                    wgpu::Extent3d {
                        width: dim,
                        height: dim,
                        depth_or_array_layers: 1,
                    },
                );
            });
            texture
        }

        /// Render `entry_point` over a `SAMPLES x 1` `Rgba32Float` target — float,
        /// because an 8-bit target quantises to 1/255, a thousand times coarser
        /// than the tolerance — and read every lane back.
        fn render(&self, entry_point: &str, inputs: &[u8]) -> Vec<[f32; 4]> {
            let module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-pom-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl([POM_WGSL, HARNESS_WGSL].concat().into()),
                });
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-pom-parity-bgl"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });
            let uniform = wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-pom-parity-uniform"),
                    contents: inputs,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let texture = self.height_texture();
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            // NEAREST everywhere, so a lookup is one exact texel: linear
            // filtering has implementation-defined sub-texel precision, which
            // would put the hardware's interpolator inside the comparison.
            let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("axiom-pom-parity-sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-pom-parity-bg"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-pom-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-pom-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("pom_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
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
            let target = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-pom-parity-target"),
                size: wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-pom-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-pom-parity-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target_view,
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
                    texture: &target,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            self.device
                .poll(wgpu::PollType::Wait)
                .expect("the readback must complete");
            let mapped = slice.get_mapped_range();
            (0..SAMPLES)
                .map(|sample| {
                    [0_usize, 1, 2, 3].map(|lane| {
                        let at = sample * 16 + lane * 4;
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

    /// Pad a flat float list into the harness's uniform.
    fn uniform_bytes(values: Vec<f32>) -> Vec<u8> {
        let mut bytes: Vec<u8> = values.iter().copied().flat_map(f32::to_le_bytes).collect();
        bytes.resize(SAMPLES * LANES * 16, 0);
        bytes
    }

    /// The march cases' uniform, and the gradients every case is sampled with.
    fn march_bytes(gradient: f32) -> Vec<u8> {
        uniform_bytes(
            cases()
                .iter()
                .flat_map(|case| {
                    [
                        case.uv.x, case.uv.y, case.depth, case.fade, //
                        case.vt.x, case.vt.y, case.vt.z, case.layers, //
                        gradient, 0.0, 0.0, gradient, //
                        0.0, 0.0, 0.0, 0.0,
                    ]
                })
                .collect(),
        )
    }

    /// The worst absolute lane delta — the measurement a tolerance is set from.
    fn worst_delta(cpu: &[[f32; 4]], gpu: &[[f32; 4]]) -> f32 {
        cpu.iter()
            .zip(gpu.iter())
            .flat_map(|(expected, actual)| {
                [0_usize, 1, 2, 3].map(|lane| (expected[lane] - actual[lane]).abs())
            })
            .fold(0.0_f32, f32::max)
    }

    /// **The parity proof.** Every march case, on a real adapter, against the CPU
    /// reference — and the conditioning assertions that make the comparison mean
    /// something: the same step count on both sides is only guaranteed because
    /// each exit test is far from flipping and each sample lands on a texel
    /// centre.
    #[test]
    fn the_march_agrees_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );
        let all = cases();
        assert_eq!(all.len(), SAMPLES, "one fragment per case");
        // A gradient far below one texel: log2 of it is negative, so the LOD
        // clamps to level 0 and the staircase is what the march reads.
        let rendered = gpu.render("pom_uv_fs", &march_bytes(1.0e-6));
        let expected: Vec<[f32; 4]> = all
            .iter()
            .map(|case| {
                let march = pom(case.uv, case.vt, case.depth, case.fade, case.layers, height_at);
                [march.uv.x, march.uv.y, 0.0, 0.0]
            })
            .collect();
        let delta = worst_delta(&expected, &rendered);
        expected
            .iter()
            .zip(rendered.iter())
            .enumerate()
            .for_each(|(index, (cpu, actual))| {
                (0..2).for_each(|lane| {
                    let gap = (cpu[lane] - actual[lane]).abs();
                    assert!(
                        gap <= MARCH_TOLERANCE,
                        "owPOM disagrees at case {index} lane {lane}: \
                         CPU {} vs GPU {} (delta {gap}, budget {MARCH_TOLERANCE})",
                        cpu[lane],
                        actual[lane]
                    );
                });
            });
        // The delta is carried in the assertion message rather than printed: no
        // layer or module in this engine emits console output, tests included
        // (Module Law #10, enforced by the architecture checker). The recorded
        // figure lives in `notes/material-pom.md`.
        assert!(
            delta <= MARCH_TOLERANCE,
            "owPOM worst CPU/GPU delta {delta:e} exceeds the measured budget              {MARCH_TOLERANCE:e}"
        );
    }

    /// The fixture is only a proof if the march is well-conditioned: the same
    /// step count on both sides, a real displacement to compare, and every
    /// sample on a texel centre. Each of those is asserted rather than assumed.
    #[test]
    fn every_case_is_well_conditioned_enough_for_the_comparison_to_mean_something() {
        let all = cases();
        let mut faults: Vec<String> = Vec::new();
        let mut crossed = 0;
        let mut exhausted = 0;
        let mut started = 0;
        for (index, case) in all.iter().enumerate() {
            // Record every uv the march samples, and check each lands mid-texel.
            let visited = std::cell::RefCell::new(Vec::<f32>::new());
            let march = pom(case.uv, case.vt, case.depth, case.fade, case.layers, |uv| {
                visited.borrow_mut().push(uv.x);
                height_at(uv)
            });
            let moved = (march.uv.x - case.uv.x).abs() + (march.uv.y - case.uv.y).abs();
            if case.depth <= 0.0 || case.fade <= 0.001 {
                assert_eq!(march.uv, case.uv, "case {index} must be untouched");
                assert_eq!(march.uv.x.to_bits(), case.uv.x.to_bits());
                assert_eq!(march.uv.y.to_bits(), case.uv.y.to_bits());
                continue;
            }
            let off_centre = visited
                .borrow()
                .iter()
                .map(|u| {
                    let scaled = u * HEIGHT_DIM as f32;
                    (scaled - scaled.floor() - 0.5).abs()
                })
                .fold(0.0_f32, f32::max);
            // The per-case figures fold into the fault message below instead of
            // being printed: no layer or module in this engine emits console
            // output, tests included (Module Law #10). A number only visible
            // under `--nocapture` is narration; a number in a failure message
            // is evidence.
            let shape = format!(
                "nl {:>5} steps {:>2} margin {:.5} moved {moved:.5}",
                march.layer_count, march.steps, march.margin
            );
            if off_centre >= 1.0e-5 {
                faults.push(format!(
                    "case {index} ({shape}) samples {off_centre} texels off \
                     centre; nearest-filter rounding would then be inside the \
                     comparison"
                ));
            }
            // A march that exits before its first step compares 0 against a
            // depth of exactly 0 — a tie, but a tie between two exactly
            // representable values that both sides compute identically, so it
            // cannot be decided differently. Every other case has to be far
            // from flipping, and has to move.
            let immediate = march.steps == 0;
            if !immediate && march.margin <= 0.01 {
                faults.push(format!(
                    "case {index} exit test is only {} from flipping",
                    march.margin
                ));
            }
            if !immediate && moved <= 100.0 * TOLERANCE {
                faults.push(format!("case {index} moved the uv by only {moved}"));
            }
            let capped = march.steps == POM_MAX_STEPS;
            let limited = !capped & (march.steps as f32 >= march.layer_count);
            crossed += usize::from(!immediate & !capped & !limited);
            exhausted += usize::from(capped | limited);
            started += usize::from(immediate);
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
        assert!(crossed >= 9, "only {crossed} cases cross the height field");
        assert!(exhausted >= 2, "only {exhausted} cases run out of layers");
        assert_eq!(started, 1, "exactly one case must exit before its first step");
    }

    /// `ddx`/`ddy` are not decoration. Sampling with implicit derivatives in the
    /// loop's non-uniform control flow is undefined, which is why the source
    /// threads explicit gradients through — so prove they reach the sampler: a
    /// gradient wide enough to select mip 1 (a solid wall of full height) must
    /// change the answer.
    #[test]
    fn the_march_honours_the_gradients_it_is_given() {
        let gpu = Gpu::acquire();
        let sharp = gpu.render("pom_uv_fs", &march_bytes(1.0e-6));
        // One texel of the 16-wide level 0 is 1/16 of uv; 1/8 selects level 1.
        let blurred = gpu.render("pom_uv_fs", &march_bytes(0.125));
        let moved = worst_delta(&sharp, &blurred);
        assert!(
            moved > 0.01,
            "the mip the gradients select does not reach the march (delta {moved})"
        );
        // And at level 1 the height is solid, so `cur >= d` holds at i = 0 and
        // every case comes back at the uv it started from.
        cases()
            .iter()
            .zip(blurred.iter())
            .enumerate()
            .for_each(|(index, (case, lanes))| {
                assert_eq!(lanes[0], case.uv.x, "case {index} u");
                assert_eq!(lanes[1], case.uv.y, "case {index} v");
            });
    }

    /// The call site's two helpers, on the same adapter: the tangent-space view
    /// direction and the distance fade.
    #[test]
    fn the_view_tangent_and_the_fade_agree_with_the_cpu_reference() {
        let gpu = Gpu::acquire();
        let all = frame_cases();
        let rendered = gpu.render(
            "pom_frame_fs",
            &uniform_bytes(
                all.iter()
                    .flat_map(|(v, t, b, n, dist, near, far)| {
                        [
                            v.x, v.y, v.z, *dist, //
                            t.x, t.y, t.z, *near, //
                            b.x, b.y, b.z, *far, //
                            n.x, n.y, n.z, 0.0,
                        ]
                    })
                    .collect(),
            ),
        );
        let expected: Vec<[f32; 4]> = all
            .iter()
            .map(|(v, t, b, n, dist, near, far)| {
                let vt = pom_view_tangent(*v, *t, *b, *n);
                [vt.x, vt.y, vt.z, pom_fade(*dist, *near, *far)]
            })
            .collect();
        let delta = worst_delta(&expected, &rendered);
        // Figure carried in the message, not printed — Module Law #10; the
        // recorded value lives in `notes/material-pom.md`.
        assert!(
            delta <= FRAME_TOLERANCE,
            "the pom frame helpers disagree by {delta:e} (tolerance \
             {FRAME_TOLERANCE:e})"
        );
        // Non-vacuous: the fade sweeps its whole range across the cases, and the
        // tangent lanes are not all the same.
        let fades: Vec<f32> = expected.iter().map(|lanes| lanes[3]).collect();
        assert!(fades.iter().cloned().fold(0.0_f32, f32::max) > 0.99);
        assert!(fades.iter().cloned().fold(1.0_f32, f32::min) < 0.01);
    }
}
