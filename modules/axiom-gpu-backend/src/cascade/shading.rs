//! The fragment stage of [`crate::cascade`]: how a receiver *reads* the atlas.
//!
//! Transcribed from `csmShaderChunk`'s GLSL in
//! `C:/dev/Claude-of-Duty/src/render/csm.js` — `owIGNoise`, `owVogel`,
//! `owCsmCascade` and `owSunShadow`. The fit next door decides where each
//! cascade's map looks from; this decides which cascade a fragment reads, where
//! in it, with what bias, and how the two cascades either side of a boundary are
//! blended.
//!
//! Everything is a pure function of a `tap(layer, u, v)` closure, so the same
//! definition drives a native test over an analytic map and the adapter proof
//! over a real rendered atlas.
//!
//! `mix`, `smoothstep` and `step` are **written out** rather than called through
//! a builtin whose factoring is unspecified — the precedent
//! `crate::surface_program::emit` sets, and the reason the GPU parity lands where
//! it does. The source's early returns become one table select at the end,
//! because the Rust here is branchless; the WGSL keeps them, because shader text
//! is data.

use axiom_math::{Vec3, Vec4};

use crate::cascade::{CascadeParams, CascadeQuality, CascadeSet, MAX_CASCADES};

/// GLSL `mix(a, b, t)` — written out, never a builtin whose factoring is
/// unspecified.
///
/// The spec's factoring is `x * (1 - a) + y * a`, and it is written that way
/// here because float arithmetic is not associative: `a + (b - a) * t` is
/// algebraically the same and numerically is not. This file and
/// `adapter_proof.rs` both carried the `a + (b - a) * t` form, which made the
/// pass's "bit-exact, worst delta 0.0" real-adapter proof compare one misreading
/// to itself. A proof that cannot fail is not a proof. Every other `mix` in this
/// crate (`gtao`, `indirect_lighting`) already used the spec form.
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

/// GLSL `smoothstep(e0, e1, x)` — written out. `csm.js` calls it with `e0 > e1`
/// for the far fade-out, which the spec leaves to the implementation and every
/// implementation evaluates as this formula: a descending ramp.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).max(0.0).min(1.0);
    t * t * (3.0 - 2.0 * t)
}

/// GLSL `step(edge, x)` — `1.0` when `x >= edge`.
fn step(edge: f32, x: f32) -> f32 {
    f32::from(x >= edge)
}

/// `owIGNoise` — the interleaved-gradient hash that gives each pixel its own
/// Vogel phase. `fract(x)` is `x - floor(x)`, which for a negative argument is
/// not Rust's `%`.
pub(crate) fn ig_noise(x: f32, y: f32) -> f32 {
    let d = x * 0.067_110_56 + y * 0.005_837_15;
    let f0 = d - d.floor();
    let m = 52.982_918_9 * f0;
    m - m.floor()
}

/// `owVogel` — tap `i` of `n` on a Vogel disc rotated by `phi`.
pub(crate) fn vogel(index: u32, taps: u32, phi: f32) -> (f32, f32) {
    let r = ((index as f32 + 0.5) / taps as f32).sqrt();
    let theta = index as f32 * 2.399_963_2 + phi;
    (theta.cos() * r, theta.sin() * r)
}

/// The cascade the shader picks for a view depth: the first whose far split it
/// is under, else the last live one.
///
/// The source scans `for i in 0..N { if (vd < split[i]) { c = i; break; } }`.
/// Because the splits **ascend** — `crate::cascade::splits` builds them that way
/// and the fit copies them in order — "the index of the first split the depth is
/// under" is just "how many splits the depth is at or past", which is a `count`
/// rather than a search: total, no `Option` to default back out of, and the
/// source's fall-through to the last cascade becomes the clamp that was always
/// implied.
pub(crate) fn select_cascade(view_depth: f32, split: &[f32; MAX_CASCADES], count: usize) -> usize {
    let count = count.min(MAX_CASCADES).max(1);
    split
        .iter()
        .take(count)
        .filter(|s| view_depth >= **s)
        .count()
        .min(count - 1)
}

/// Project a world point into cascade `index`, applying the normal offset the
/// source applies before projecting: `wPos + wN * texel * (0.55 + 1.1 * (1 - NdL))`.
///
/// Returns `(u, v, depth)` in wgpu convention: `depth` is already `[0, 1]`, and
/// `v` counts down the framebuffer — the same two conventions the engine's
/// existing `shadow_factor` applies.
pub(crate) fn project(
    set: &CascadeSet,
    index: usize,
    world_pos: Vec3,
    world_normal: Vec3,
    n_dot_l: f32,
) -> (f32, f32, f32) {
    let fit = set.fits[index.min(MAX_CASCADES - 1)];
    let offset = fit.texel * (0.55 + 1.1 * (1.0 - n_dot_l));
    let p = world_pos.add(world_normal.mul_scalar(offset));
    let sc = fit.view_proj.transform_vec4(Vec4::new(p.x, p.y, p.z, 1.0));
    let ndc = Vec3::new(sc.x / sc.w, sc.y / sc.w, sc.z / sc.w);
    (ndc.x * 0.5 + 0.5, ndc.y * -0.5 + 0.5, ndc.z)
}

/// `owCsmCascade` — the filtered shadow term from one cascade.
///
/// `tap(layer, u, v)` reads the stored light-space depth; the real pass reads
/// the atlas layer, a test reads whatever it rendered.
///
/// The source's three early returns (outside the depth range, outside the map,
/// PCSS found no blocker) become one table select at the end. The PCSS blocker
/// mean divides by `count.max(1)` rather than `count`, so the discarded arm is a
/// finite number rather than a `NaN` — value-identical, because a table index
/// selects rather than arithmetically combines.
pub(crate) fn cascade_shadow<F: Fn(usize, f32, f32) -> f32>(
    set: &CascadeSet,
    params: CascadeParams,
    quality: CascadeQuality,
    index: usize,
    world_pos: Vec3,
    world_normal: Vec3,
    n_dot_l: f32,
    rot: f32,
    tap: &F,
) -> f32 {
    let index = index.min(MAX_CASCADES - 1);
    let fit = set.fits[index];
    let (u, v, depth) = project(set, index, world_pos, world_normal, n_dot_l);
    let inside_depth = (depth < 1.0) & (depth > 0.0);
    let edge = u.min(1.0 - u).min(v.min(1.0 - v));
    let slope = ((1.0 - n_dot_l * n_dot_l).max(0.0).sqrt() / n_dot_l.max(0.12))
        .max(0.0)
        .min(5.0);
    let bias = (fit.texel * (0.7 + 1.15 * slope)) / fit.range;
    let recv = depth - bias;

    let inv_tex = 1.0 / set.map_size as f32;
    let extent = fit.texel * set.map_size as f32;
    let max_r = params.max_filter_texels * inv_tex;
    let search_r = max_r.min(10.0 * inv_tex);
    let (blocker_sum, blockers) = (0..quality.blocker_taps).fold((0.0_f32, 0.0_f32), |acc, i| {
        let (vx, vy) = vogel(i, quality.blocker_taps, rot);
        let d = tap(index, u + vx * search_r, v + vy * search_r);
        let hit = f32::from(d < recv);
        (acc.0 + d * hit, acc.1 + hit)
    });
    let blocker = blocker_sum / blockers.max(1.0);
    let gap = ((recv - blocker) * fit.range).max(0.0);
    let penumbra = gap * params.softness;
    let pcss_r = (penumbra / extent).max(inv_tex).min(max_r);
    let filter_r = [1.4 * inv_tex, pcss_r][usize::from(quality.pcss)];

    let sum = (0..quality.pcf_taps).fold(0.0_f32, |acc, i| {
        let (vx, vy) = vogel(i, quality.pcf_taps, rot);
        acc + step(recv, tap(index, u + vx * filter_r, v + vy * filter_r))
    });
    let filtered = sum / quality.pcf_taps as f32;
    let lit = !inside_depth | (edge <= 0.0) | (quality.pcss & (blockers < 0.5));
    [filtered, 1.0][usize::from(lit)]
}

/// `owSunShadow` — the whole term: cascade selection, the cross-fade over the
/// last 12% of the selected cascade, and the fade-out over the last 12% of the
/// range.
///
/// `frag_coord` is the fragment's window coordinate, which only the Vogel phase
/// reads. `world_normal` must already be normalised, as the source's
/// `normalize(nrmView * mat3(viewMatrix))` is.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sun_shadow<F: Fn(usize, f32, f32) -> f32>(
    set: &CascadeSet,
    params: CascadeParams,
    quality: CascadeQuality,
    view_depth: f32,
    world_pos: Vec3,
    world_normal: Vec3,
    sun_dir_world: Vec3,
    frag_coord: (f32, f32),
    tap: &F,
) -> f32 {
    let split = set.split();
    let split_near = set.split_near();
    let last = set.count - 1;
    let n_dot_l = world_normal.dot(sun_dir_world);
    let rot = ig_noise(frag_coord.0 + params.rotation, frag_coord.1 + params.rotation)
        * core::f32::consts::TAU;
    let c = select_cascade(view_depth, &split, set.count);
    let here = cascade_shadow(
        set,
        params,
        quality,
        c,
        world_pos,
        world_normal,
        n_dot_l,
        rot,
        tap,
    );
    let next = cascade_shadow(
        set,
        params,
        quality,
        (c + 1).min(last),
        world_pos,
        world_normal,
        n_dot_l,
        rot,
        tap,
    );
    // Cross-fade the last 12% of a cascade into the next one. The source skips
    // the mix entirely below t = 0.001 rather than mixing by a hair, so the gate
    // is part of the value, not an optimisation.
    let t_raw = smoothstep(
        mix(split_near[c], split[c], 0.88),
        split[c],
        view_depth,
    );
    let t = [0.0, t_raw][usize::from((c < last) & (t_raw > 0.001))];
    let blended = mix(here, next, t);
    // Fade the whole thing out at the far edge so there is no hard terminator.
    let faded = mix(
        1.0,
        blended,
        smoothstep(split[last], split[last] * 0.88, view_depth),
    );
    let shaded = mix(1.0, faded, params.strength);
    let lit = (params.strength <= 0.0) | (view_depth >= split[last]) | (n_dot_l <= 0.0);
    [shaded, 1.0][usize::from(lit)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cascade::{fit, quality_tier, CascadeCamera, MAP_SIZE};
    use axiom_math::Mat4;

    /// The source's shipped configuration: `4 x 2048`, 140 m, lambda 0.86.
    fn street_camera() -> CascadeCamera {
        CascadeCamera {
            world: Mat4::translation(Vec3::new(0.0, 3.0, 10.0)),
            fovy_radians: 60_f32.to_radians(),
            aspect: 16.0 / 9.0,
            near: 0.5,
            far: 300.0,
        }
    }

    /// Pointing FROM the scene TOWARD the sun, the source's convention.
    fn sun() -> Vec3 {
        Vec3::new(0.35, 0.85, 0.4).normalize().unwrap()
    }

    fn street_set() -> CascadeSet {
        fit(4, street_camera(), sun(), MAP_SIZE).unwrap()
    }

    #[test]
    fn cascade_selection_walks_the_splits_in_order() {
        let split = [4.0_f32, 18.0, 55.0, 140.0];
        assert_eq!(select_cascade(1.0, &split, 4), 0);
        assert_eq!(select_cascade(4.0, &split, 4), 1, "the boundary is exclusive");
        assert_eq!(select_cascade(17.9, &split, 4), 1);
        assert_eq!(select_cascade(54.0, &split, 4), 2);
        assert_eq!(select_cascade(139.0, &split, 4), 3);
        // Past the last split there is nothing to pick, so the last one stands
        // (the caller's fade-out has already taken the term to 1.0 by then).
        assert_eq!(select_cascade(500.0, &split, 4), 3);
        // A shorter set never looks at the lanes past its count.
        assert_eq!(select_cascade(100.0, &split, 2), 1);
        assert_eq!(select_cascade(0.0, &split, 0), 0);
        assert_eq!(select_cascade(0.0, &split, 9), 0);
    }

    #[test]
    fn the_vogel_disc_and_the_noise_phase_match_the_source() {
        // r = sqrt((i + 0.5) / n), theta = i * 2.39996323 + phi.
        let (x, y) = vogel(0, 16, 0.0);
        let r0 = (0.5_f32 / 16.0).sqrt();
        assert!(((x - r0).abs() < 1.0e-6) & (y.abs() < 1.0e-6), "tap 0 at {x},{y}");
        // Every tap is inside the unit disc, and the outermost is on it.
        (0..20).for_each(|i| {
            let (x, y) = vogel(i, 20, 1.0);
            assert!((x * x + y * y).sqrt() <= 1.0 + 1.0e-6, "tap {i} escaped the disc");
        });
        let (lx, ly) = vogel(19, 20, 0.0);
        assert!((lx * lx + ly * ly).sqrt() > 0.98, "the last tap is on the rim");
        // fract(x) is x - floor(x): the hash stays in [0, 1) for negative
        // coordinates too, which is where a `%` transcription would break.
        [(-512.0_f32, -3.5_f32), (0.5, 0.5), (1919.0, 1079.0)]
            .into_iter()
            .for_each(|(x, y)| {
                let n = ig_noise(x, y);
                assert!((0.0..1.0).contains(&n), "noise at {x},{y} escaped: {n}");
            });
        // It is a hash, not a constant: neighbouring pixels differ.
        assert_ne!(ig_noise(10.5, 4.5), ig_noise(11.5, 4.5));
    }

    /// An analytic shadow map: a single flat blocker at a known stored depth
    /// over a disc of the map, everything else empty (1.0).
    fn disc_map(layer_depth: [f32; MAX_CASCADES], centre: (f32, f32), radius: f32) -> impl Fn(usize, f32, f32) -> f32 {
        move |layer, u, v| {
            let d = ((u - centre.0).powi(2) + (v - centre.1).powi(2)).sqrt();
            [1.0, layer_depth[layer]][usize::from(d <= radius)]
        }
    }

    #[test]
    fn a_receiver_under_a_blocker_is_shadowed_and_one_beside_it_is_not() {
        let set = street_set();
        let quality = quality_tier(3);
        let params = CascadeParams::default();
        let n = Vec3::new(0.0, 1.0, 0.0);
        let ground = Vec3::new(0.0, 0.0, 5.0);
        let view_depth = 5.0_f32;
        let c = select_cascade(view_depth, &set.split(), set.count());
        let (u, v, depth) = project(&set, c, ground, n, n.dot(sun()));
        // A blocker sitting well in front of the receiver, right where the
        // receiver projects.
        let mut depths = [1.0_f32; MAX_CASCADES];
        depths[c] = depth - 0.05;
        let shadowed = sun_shadow(
            &set,
            params,
            quality,
            view_depth,
            ground,
            n,
            sun(),
            (640.5, 360.5),
            &disc_map(depths, (u, v), 0.02),
        );
        assert!(shadowed < 0.01, "a covered receiver reads {shadowed}");
        // The same map, a receiver whose projection is nowhere near the blocker.
        let lit = sun_shadow(
            &set,
            params,
            quality,
            view_depth,
            ground,
            n,
            sun(),
            (640.5, 360.5),
            &disc_map(depths, (u + 0.3, v), 0.02),
        );
        assert_eq!(lit, 1.0, "an uncovered receiver must be fully lit");
    }

    #[test]
    fn the_filter_softens_with_the_blocker_gap_which_is_what_pcss_buys() {
        let set = street_set();
        let params = CascadeParams::default();
        let n = Vec3::new(0.0, 1.0, 0.0);
        let ground = Vec3::new(0.0, 0.0, 5.0);
        let view_depth = 5.0_f32;
        let c = select_cascade(view_depth, &set.split(), set.count());
        let (u, v, depth) = project(&set, c, ground, n, n.dot(sun()));
        // Sample just off the edge of a blocker disc: with a contact-tight
        // blocker the penumbra is narrow, with a distant one it is wide, so the
        // same probe reads darker under the distant blocker.
        let edge = (u + 0.0016, v);
        let contact = {
            let mut d = [1.0_f32; MAX_CASCADES];
            d[c] = depth - 0.0005;
            sun_shadow(
                &set, params, quality_tier(3), view_depth, ground, n, sun(),
                (640.5, 360.5), &disc_map(d, edge, 0.001),
            )
        };
        let distant = {
            let mut d = [1.0_f32; MAX_CASCADES];
            d[c] = depth - 0.20;
            sun_shadow(
                &set, params, quality_tier(3), view_depth, ground, n, sun(),
                (640.5, 360.5), &disc_map(d, edge, 0.001),
            )
        };
        assert!(
            distant < contact,
            "a distant blocker ({distant}) must cast a wider penumbra than a contact one ({contact})"
        );
        // Without PCSS the filter radius is the fixed 1.4 texels, so the two are
        // identical — that is the tier difference, stated.
        let flat_contact = {
            let mut d = [1.0_f32; MAX_CASCADES];
            d[c] = depth - 0.0005;
            sun_shadow(
                &set, params, quality_tier(0), view_depth, ground, n, sun(),
                (640.5, 360.5), &disc_map(d, edge, 0.001),
            )
        };
        let flat_distant = {
            let mut d = [1.0_f32; MAX_CASCADES];
            d[c] = depth - 0.20;
            sun_shadow(
                &set, params, quality_tier(0), view_depth, ground, n, sun(),
                (640.5, 360.5), &disc_map(d, edge, 0.001),
            )
        };
        assert_eq!(
            flat_contact, flat_distant,
            "a fixed-radius PCF cannot contact-harden"
        );
    }

    #[test]
    fn the_cascade_boundary_cross_fades_instead_of_stepping() {
        let set = street_set();
        let quality = quality_tier(3);
        let params = CascadeParams::default();
        let n = Vec3::new(0.0, 1.0, 0.0);
        let split = set.split();
        let split_near = set.split_near();
        // Everything blocked in cascade 0, nothing blocked in cascade 1: the
        // blend weight is then directly readable out of the returned term.
        let map = |layer: usize, _u: f32, _v: f32| [0.0, 1.0][usize::from(layer >= 1)];
        let sample = |view_depth: f32| {
            let p = Vec3::new(0.0, 0.0, 10.0 - view_depth);
            sun_shadow(
                &set, params, quality, view_depth, p, n, sun(), (640.5, 360.5), &map,
            )
        };
        let before = mix(split_near[0], split[0], 0.87);
        let inside = mix(split_near[0], split[0], 0.94);
        assert_eq!(sample(before), 0.0, "before the fade, cascade 0 alone");
        let mid = sample(inside);
        assert!(
            (mid > 0.05) & (mid < 0.95),
            "inside the fade the two cascades mix, got {mid}"
        );
        assert!(
            sample(split[0] * 0.9999) > mid,
            "the fade rises monotonically toward the boundary"
        );
        // The last cascade has nothing to fade into, so its own `c < last` gate
        // is false and the term is cascade 3 alone.
        let last_depth = mix(split_near[3], split[3], 0.94);
        let p = Vec3::new(0.0, 0.0, 10.0 - last_depth);
        let all_blocked = |_l: usize, _u: f32, _v: f32| 0.0_f32;
        let last = sun_shadow(
            &set, params, quality, last_depth, p, n, sun(), (640.5, 360.5), &all_blocked,
        );
        // Cascade 3 is fully blocked, but the global fade-out has begun, so the
        // term is lifted off zero rather than being the raw 0.0.
        assert!((last > 0.0) & (last < 1.0), "the far fade-out lifts {last}");
    }

    #[test]
    fn the_term_is_one_wherever_the_source_returns_early() {
        let set = street_set();
        let quality = quality_tier(3);
        let n = Vec3::new(0.0, 1.0, 0.0);
        let blocked = |_l: usize, _u: f32, _v: f32| 0.0_f32;
        let at = |params: CascadeParams, view_depth: f32, normal: Vec3| {
            sun_shadow(
                &set,
                params,
                quality,
                view_depth,
                Vec3::new(0.0, 0.0, 10.0 - view_depth),
                normal,
                sun(),
                (640.5, 360.5),
                &blocked,
            )
        };
        let params = CascadeParams::default();
        // Strength off.
        assert_eq!(
            at(
                CascadeParams {
                    strength: 0.0,
                    ..params
                },
                5.0,
                n
            ),
            1.0
        );
        // Past the last split.
        assert_eq!(at(params, 200.0, n), 1.0);
        assert_eq!(at(params, set.split()[3], n), 1.0, "the far split is exclusive");
        // Facing away from the sun: the surface is already unlit by N.L, so the
        // shadow term must not double-darken it.
        assert_eq!(at(params, 5.0, Vec3::new(0.0, -1.0, 0.0)), 1.0);
        // Behind the map: a receiver far outside the cascade's own lateral
        // extent projects outside [0,1] and reads lit.
        let far_aside = sun_shadow(
            &set,
            params,
            quality,
            5.0,
            Vec3::new(4000.0, 0.0, 5.0),
            n,
            sun(),
            (640.5, 360.5),
            &blocked,
        );
        assert_eq!(far_aside, 1.0, "outside the map is lit, not black");
        // Behind the light's near plane: a receiver above the light itself
        // projects to a negative depth and reads lit.
        let above = sun_shadow(
            &set,
            params,
            quality,
            5.0,
            Vec3::new(0.0, 4000.0, 5.0),
            n,
            sun(),
            (640.5, 360.5),
            &blocked,
        );
        assert_eq!(above, 1.0, "outside the depth range is lit, not black");
        // PCSS with no blocker found returns 1.0 before it can divide by zero.
        let empty = |_l: usize, _u: f32, _v: f32| 1.0_f32;
        assert_eq!(
            sun_shadow(
                &set,
                params,
                quality,
                5.0,
                Vec3::new(0.0, 0.0, 5.0),
                n,
                sun(),
                (640.5, 360.5),
                &empty
            ),
            1.0
        );
        // Partial strength scales the term rather than gating it.
        let half = at(
            CascadeParams {
                strength: 0.5,
                ..params
            },
            5.0,
            n,
        );
        assert!((half - 0.5).abs() < 1.0e-6, "half strength reads {half}");
        // The temporal rotation moves the Vogel phase, so a partially covered
        // probe changes with it — that is what makes the noise temporal.
        let rotated = CascadeParams {
            rotation: 0.37,
            ..params
        };
        let disc = disc_map([0.6; MAX_CASCADES], (0.5, 0.5), 0.5);
        let p = Vec3::new(0.0, 0.0, 5.0);
        let c = select_cascade(5.0, &set.split(), set.count());
        let (u, v, depth) = project(&set, c, p, n, n.dot(sun()));
        let mut d = [1.0_f32; MAX_CASCADES];
        d[c] = depth - 0.02;
        let edge = disc_map(d, (u + 0.0012, v), 0.0008);
        let a = sun_shadow(&set, params, quality, 5.0, p, n, sun(), (7.5, 3.5), &edge);
        let b = sun_shadow(&set, rotated, quality, 5.0, p, n, sun(), (7.5, 3.5), &edge);
        assert_ne!(a, b, "the temporal rotation must move the disc");
        // `disc` is the analytic map's other arm (a tap outside the disc).
        assert_eq!(disc(0, 0.99, 0.99), 1.0);
    }

    /// The invariant the extension must not break: the engine's one-cascade
    /// configuration is the existing `axiom-render-pipeline` fit and the existing
    /// `shadow_factor`, neither of which this module touches. What is asserted
    /// here is the *shape* of that contract — a one-cascade set writes one live
    /// lane and three inert sentinels, so a shader reading these lanes with
    /// `OW_CASCADES = 1` can only ever sample cascade 0.
    #[test]
    fn one_cascade_can_only_ever_sample_cascade_zero() {
        let one = fit(1, street_camera(), sun(), MAP_SIZE).unwrap();
        let split = one.split();
        // Every view depth inside the range selects cascade 0...
        [0.5_f32, 1.0, 37.0, 139.9].into_iter().for_each(|vd| {
            assert_eq!(select_cascade(vd, &split, 1), 0, "depth {vd}");
        });
        // ...and the cross-fade gate is dead, because c == last == 0.
        let n = Vec3::new(0.0, 1.0, 0.0);
        let sentinel_only = |layer: usize, _u: f32, _v: f32| {
            assert_eq!(layer, 0, "a one-cascade term sampled layer {layer}");
            0.0_f32
        };
        let term = sun_shadow(
            &one,
            CascadeParams::default(),
            quality_tier(3),
            5.0,
            Vec3::new(0.0, 0.0, 5.0),
            n,
            sun(),
            (640.5, 360.5),
            &sentinel_only,
        );
        assert_eq!(term, 0.0, "a fully covered one-cascade receiver is black");
    }
}
