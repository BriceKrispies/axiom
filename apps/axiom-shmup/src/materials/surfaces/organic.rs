//! Ported from Claude-of-Duty `src/materials/glsl/surfaces-organic.js:1-416`
//! — the whole file: `WOOD`, `FABRIC`, `BURLAP`, `FOLIAGE`, `RUBBER`, `GLASS`.
//!
//! Six `owSurface(uv) -> (alb, h, rough, metal, ao)` GLSL bodies, ported as
//! CPU-evaluable `f64` maths on top of [`crate::materials::noise`] — see that
//! module's doc for the GLSL -> Rust mapping (`Vec2`/`Vec3`/`Vec4`, `gl_mod`
//! vs `%`, `gl_fract`, the sin-free hashes) that every function below
//! inherits. Each generator here mirrors its GLSL source line-for-line
//! rather than being tidied, per the port recipe's faithfulness rule; where a
//! source line is genuinely dead code (a variable computed and then
//! immediately discarded, never reaching any output), that is called out and
//! *not* transcribed as inert arithmetic — see `wood`'s `nd` and
//! `foliage`'s `cover`/`best_h` for the two cases.
//!
//! ## `foliage`'s `h` is a cutout mask, not a height
//!
//! Every other generator here writes a genuine height/parallax value into
//! `h`. `foliage` does not: `h = bestCover`, the alpha-test coverage
//! of whichever leaf won the per-cell depth sort. The bake pipeline
//! ([`crate::materials::bake`]) packs `h` into `albedo.a` the same way for
//! every surface, but for foliage the material layer reads that channel as an
//! alpha-test cutout mask (`alphaTest 0.45`, `crate::materials::MatParams::
//! alpha_mask`), not as parallax relief — the same byte, a different meaning.
//! `tests/materials_surfaces_organic_port.rs` asserts this directly: foliage's
//! `h` clusters near `0.0`/`1.0` (binary-ish, a cutout mask) rather than
//! spreading continuously like every other surface's height channel.
//!
//! ## `owSurface`'s uniforms as explicit parameters
//!
//! The source closes over three GLSL uniforms per material:
//! `uSeed` (every generator here takes it as `seed: f64`) and, for `fabric`
//! only, `uTintA`/`uTintB` (`tint_a`/`tint_b: Vec3` — the two colours
//! `crate::materials::BakeParams::tint_a`/`tint_b` decode into). No other
//! generator in this file references `uTintA`/`uTintB`/`uParam`.

use super::super::bake::SurfaceSample;
use super::super::noise::{
    gl_clamp, gl_fract, gl_mix, gl_mod, gl_smoothstep, ow_cracks, ow_fbm, ow_fbm01, ow_hash11,
    ow_hash12, ow_hash42, ow_rot, ow_scratches, ow_shear, ow_shear_per, ow_srgb, ow_warp,
    ow_worley, Vec2, Vec3,
};

// ---------------------------------------------------------------------------
// Local GLSL-vocabulary helpers not already provided by `noise` — same
// reasoning as `bake.rs`'s private `gl_step`: these are bare GLSL builtins
// the source calls directly, not one of `noise.js`'s named functions, so each
// file that needs them owns its own copy rather than reaching into a sibling
// module's private items.
// ---------------------------------------------------------------------------

/// GLSL `step(edge, x)`: `1.0` when `x >= edge`, else `0.0`.
fn gl_step(edge: f64, x: f64) -> f64 {
    if x < edge {
        0.0
    } else {
        1.0
    }
}

/// Component-wise `mix(vec3, vec3, float)` — every `vec3` mix call site in
/// this file blends with a single scalar `t`, never a per-channel one.
fn mix3(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    Vec3::new(gl_mix(a.x, b.x, t), gl_mix(a.y, b.y, t), gl_mix(a.z, b.z, t))
}

/// Component-wise `vec3 + vec3` — `Vec3` has no `add`; only two call sites in
/// this file need real vector addition (`fabric`'s stain mix, `glass`'s
/// scratch tint), everywhere else a scalar `add_scalar`/`mix3` suffices.
fn add3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// `clamp(v, vec3(lo), vec3(hi))` — every call site in this file clamps to a
/// scalar-broadcast range (`vec3(0.02)`, never a per-channel bound).
fn clamp3(v: Vec3, lo: f64, hi: f64) -> Vec3 {
    Vec3::new(gl_clamp(v.x, lo, hi), gl_clamp(v.y, lo, hi), gl_clamp(v.z, lo, hi))
}

// ---------------------------------------------------------------------------
// WOOD — surfaces-organic.js:8-120.
// ---------------------------------------------------------------------------

/// `WOOD`'s `owSurface` (`surfaces-organic.js:9-119`): 5 plank rows of 2
/// staggered boards, growth rings pulled into a radial swirl by knots, fibre
/// shear noise, silvering weather, splits, saw marks, bevelled edges, and
/// nails with a rust weep.
pub fn wood(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let planks = 5.0;
    let p = uv.mul(p_const).add_scalar(seed * 12.9);

    // ---- plank layout: rows running along X, staggered butt joints ----
    let row_f = uv.y * planks;
    let row = row_f.floor();
    let rf = gl_fract(row_f);
    let stagger = ow_hash11(row + seed * 2.0);
    let len_f = uv.x * 2.0 + stagger;
    let board = len_f.floor();
    let lf = gl_fract(len_f);
    let rnd = ow_hash42(Vec2::new(board, row).add_scalar(seed));

    // gaps between boards
    let gy = 0.035;
    let gx = 0.010;
    let ey = gl_smoothstep(0.0, gy, rf).min(gl_smoothstep(0.0, gy, 1.0 - rf));
    let ex = gl_smoothstep(0.0, gx, lf).min(gl_smoothstep(0.0, gx, 1.0 - lf));
    let face = ex.min(ey);

    // ---- grain: rings stretched along the board, warped, with knots ----
    let gp = Vec2::new(lf * 2.0 + rnd.x * 13.0, rf + rnd.y * 7.0);
    let gp_const = Vec2::new(16.0, 8.0);
    let warp = ow_fbm(
        Vec2::new(gp.x * 3.0, gp.y * 12.0),
        Vec2::new(gp_const.x * 3.0, gp_const.y * 12.0),
        4,
        0.55,
    );
    let mut ring_coord = gp.y * (14.0 + rnd.z * 12.0) + warp * 2.2 + rnd.w * 5.0;

    // knots pull the rings into a tight radial swirl
    let knot_p = Vec2::new(0.25 + rnd.x * 0.5, 0.35 + rnd.y * 0.3);
    let kd = Vec2::new(lf, rf).sub(knot_p).mul(Vec2::new(2.2, 1.0)).length();
    let has_knot = gl_step(0.68, rnd.z);
    let knot_pull = has_knot * (-kd * 9.0).exp();
    ring_coord = gl_mix(ring_coord, kd * 42.0, gl_clamp(knot_pull * 1.6, 0.0, 1.0));

    let rings = gl_fract(ring_coord);
    let ring_dark = gl_smoothstep(0.42, 0.5, rings) * (1.0 - gl_smoothstep(0.5, 0.62, rings));
    let latewood = gl_smoothstep(0.30, 0.52, rings);

    // fine fibre along the grain
    let fibre = ow_fbm01(
        ow_shear(p.scale(6.0), 0.0, 40.0),
        ow_shear_per(p_const.scale(6.0), 40.0),
        4,
        0.5,
    );
    let micro = ow_fbm01(p.scale(22.0), p_const.scale(22.0), 3, 0.5);

    // ---- colour ----
    let w_light = ow_srgb(Vec3::new(0.505, 0.408, 0.290));
    let w_mid = ow_srgb(Vec3::new(0.362, 0.272, 0.180));
    let w_dark = ow_srgb(Vec3::new(0.205, 0.142, 0.092));
    let w_grey = ow_srgb(Vec3::new(0.372, 0.355, 0.328)); // weathered silver-grey
    let mut c = mix3(w_light, w_mid, rnd.w * 0.8 + latewood * 0.5);
    c = mix3(c, w_dark, ring_dark * 0.65);
    c = c.scale(0.90 + 0.18 * fibre);
    c = mix3(c, w_dark.scale(0.7), gl_clamp(knot_pull * 2.2, 0.0, 1.0) * 0.8);

    // weathering: UV-bleached, silvered, worst on the exposed boards
    let weather =
        gl_smoothstep(0.20, 0.85, ow_fbm01(p.scale(0.8), p_const.scale(0.8), 3, 0.6)) * (0.4 + 0.6 * rnd.x);
    c = mix3(c, w_grey, weather * 0.68);

    let mut face_h = 0.74 - ring_dark * 0.02 - latewood * 0.012 + (fibre - 0.5) * 0.03
        + (micro - 0.5) * 0.008;
    face_h += (rnd.y - 0.5) * 0.035; // boards cup and sit at different heights
    face_h -= gl_clamp(knot_pull * 1.5, 0.0, 1.0) * 0.03;

    // splits and checks running along the grain
    let split = ow_scratches(p.scale(2.0), p_const.scale(2.0), 30.0, 0.0, 0.66) * weather;
    face_h -= split * 0.10;
    c = mix3(c, w_dark.scale(0.45), split * 0.7);

    // saw marks across the board
    let saw = ow_fbm01(
        ow_shear(p.scale(3.0), 0.0, 1.0).mul(Vec2::new(30.0, 1.0)),
        Vec2::new(p_const.x * 90.0, p_const.y * 3.0),
        3,
        0.5,
    );
    face_h += (saw - 0.5) * 0.012;

    // rounded / bashed board edges
    let edge_d = (rf.min(1.0 - rf) / gy).min(lf.min(1.0 - lf) / gx);
    let bevel = 1.0 - gl_smoothstep(0.0, 2.4, edge_d);
    face_h -= bevel * 0.035;
    c = c.scale(1.0 - bevel * 0.10);
    c = mix3(
        c,
        w_light.scale(1.15),
        bevel * gl_smoothstep(0.5, 0.9, ow_fbm01(p.scale(20.0), p_const.scale(20.0), 3, 0.5)) * 0.35,
    );

    // ---- gap between boards: dark, deep ----
    let m = gl_smoothstep(0.05, 0.7, face);
    let mut h = gl_mix(0.44, face_h, m);
    c = mix3(w_dark.scale(0.25), c, m);
    let mut rough = gl_mix(0.95, 0.62 + 0.22 * fibre + weather * 0.20 + split * 0.15, m);
    let mut ao = gl_mix(0.25, 1.0, gl_smoothstep(0.0, 0.5, face)) - bevel * 0.12 * m;
    let mut metal = 0.0;

    // ---- nails ----
    // Source quirk, omitted (surfaces-organic.js:94-96): the source computes
    // an `nf`/`nd` pair (`nd = length(nf * vec2(3,1) / vec2(3,1) *
    // vec2(1,1))`, algebraically `length(nf)`) and then immediately
    // reassigns `nd` to the formula below, so the first pair never reaches
    // any output — dead code in the source, not transcribed here.
    let nd = Vec2::new(gl_fract(lf * 3.0 + 0.5) - 0.5, rf - 0.22)
        .mul(Vec2::new(1.4, 1.0))
        .length();
    let nail = gl_smoothstep(0.055, 0.030, nd) * m * gl_step(0.3, rnd.w);
    h -= nail * 0.02;
    c = mix3(c, ow_srgb(Vec3::new(0.230, 0.200, 0.170)), nail * 0.85);
    rough = gl_mix(rough, 0.55, nail);
    metal = gl_mix(metal, 0.85, nail * 0.7);
    ao -= nail * 0.25;
    // rust weep under the nail
    let weep = gl_smoothstep(0.11, 0.05, nd)
        * gl_step(0.3, rnd.w)
        * gl_smoothstep(0.0, 0.6, rf - 0.22)
        * m;
    c = mix3(
        c,
        ow_srgb(Vec3::new(0.330, 0.185, 0.095)),
        gl_clamp(weep, 0.0, 1.0) * 0.4,
    );

    // grime
    let cavity = 1.0 - gl_smoothstep(0.55, 0.78, h);
    c = mix3(c, ow_srgb(Vec3::new(0.120, 0.106, 0.088)), cavity * 0.45);
    // ground-in dirt over the whole board
    let soil = gl_smoothstep(
        0.40,
        0.88,
        ow_fbm01(
            ow_warp(p.scale(2.2).add_scalar(5.0), p_const.scale(2.2), 0.9, 3),
            p_const.scale(2.2),
            5,
            0.6,
        ),
    );
    c = mix3(c, ow_srgb(Vec3::new(0.185, 0.160, 0.128)), soil * 0.40);
    rough += soil * 0.08;

    SurfaceSample {
        albedo: clamp3(c, 0.02, 0.80),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.25, 0.99),
        metal,
        ao: gl_clamp(ao, 0.12, 1.0),
    }
}

// ---------------------------------------------------------------------------
// FABRIC — surfaces-organic.js:122-199.
// ---------------------------------------------------------------------------

/// `FABRIC`'s `owSurface` (`surfaces-organic.js:123-198`): a plain weave
/// (warp over weft on alternating cells), fuzz and slubs, a ~10 cm drape fold
/// field, threadbare wear, pulled threads, stains and dust.
pub fn fabric(uv: Vec2, seed: f64, tint_a: Vec3, tint_b: Vec3) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let threads = 96.0;
    let p = uv.mul(p_const).add_scalar(seed * 3.9);

    // ---- plain weave: warp over weft on alternating cells ----
    let t = uv.scale(threads);
    let cell = t.floor();
    let f = t.fract().add_scalar(-0.5);
    let over = gl_mod(cell.x + cell.y, 2.0); // 0 -> warp on top, 1 -> weft on top

    let warp_profile = (f.x * 3.14159).cos();
    let weft_profile = (f.y * 3.14159).cos();
    let top = gl_mix(warp_profile, weft_profile, over);
    let bot = gl_mix(weft_profile, warp_profile, over) * 0.45;
    let weave = top.max(bot);
    let thread_id = ow_hash12(cell.add_scalar(seed));

    // ---- fuzz and slubs ----
    let fuzz = ow_fbm01(p.scale(12.0), p_const.scale(12.0), 3, 0.55);
    let slub = ow_fbm01(p.scale(14.0), p_const.scale(14.0), 4, 0.5);
    let macro_ = ow_fbm01(p.scale(1.2), p_const.scale(1.2), 4, 0.6);

    let c_a = tint_a;
    let c_b = tint_b;
    let mut c = mix3(c_a, c_b, thread_id * 0.6 + slub * 0.4);
    c = c.scale(0.865 + 0.215 * (weave * 0.5 + 0.5));
    c = c.scale(0.960 + 0.075 * fuzz);
    c = c.scale(0.90 + 0.20 * macro_);

    let mut h = 0.55 + weave * 0.30 + (fuzz - 0.5) * 0.03 + (slub - 0.5) * 0.05;
    let mut rough = 0.86 + (1.0 - weave) * 0.08 + (fuzz - 0.5) * 0.06;
    let metal = 0.0;
    let mut ao = gl_mix(0.82, 1.0, gl_smoothstep(-0.4, 0.9, weave));

    // ---- drape folds ---------------------------------------------------------
    // Cloth under tension gathers into soft parallel ridges roughly a hand's
    // width apart, wandering as they run. At the 0.26 m mapping the awnings
    // use, 2.6 cycles across the tile is a ~10 cm fold. A weave alone reads
    // as printed canvas; the fold field is what gives a canopy its shape
    // between its poles.
    let fold_c =
        uv.y * 2.6 + uv.x * 0.55 + ow_fbm01(p.scale(0.9), p_const.scale(0.9), 3, 0.62) * 2.2;
    let fold_t = (gl_fract(fold_c) - 0.5).abs() * 2.0; // 0 at crest, 1 in trough
    let crest = 1.0 - fold_t;
    let fold_r = ow_hash11(fold_c.floor() * 2.13 + seed);
    let fold = crest * crest * (0.55 + 0.75 * fold_r);
    h += (fold - 0.30) * 0.115;
    c = c.scale(0.895 + 0.21 * fold);
    ao -= (1.0 - crest) * 0.14;
    // the crease line itself is polished by handling and holds the dust
    let crease_line = 1.0 - gl_smoothstep(0.0, 0.10, fold_t);
    rough -= crease_line * 0.06;
    c = c.scale(1.0 + crease_line * 0.05);

    // ---- wear: threadbare patches, fraying, pulled threads ----
    let wear_field = gl_smoothstep(
        0.58,
        0.82,
        ow_fbm01(
            ow_warp(p.scale(2.0), p_const.scale(2.0), 0.8, 3),
            p_const.scale(2.0),
            4,
            0.55,
        ),
    );
    c = mix3(c, c.scale(1.35).add_scalar(0.02), wear_field * 0.5);
    rough += wear_field * 0.06;
    h -= wear_field * 0.05;

    let pulled = ow_scratches(p.scale(3.0), p_const.scale(3.0), 18.0, 1.0, 0.68);
    h += pulled * 0.05;
    c = c.scale(1.0 - pulled * 0.10);

    // ---- stains and dust ----
    let stain = gl_smoothstep(
        0.55,
        0.9,
        ow_fbm01(
            ow_warp(p.scale(1.5).add_scalar(7.0), p_const.scale(1.5), 1.0, 3),
            p_const.scale(1.5),
            5,
            0.6,
        ),
    );
    c = mix3(
        c,
        add3(c.scale(0.42), ow_srgb(Vec3::new(0.09, 0.08, 0.06))),
        stain * 0.55,
    );
    rough += stain * 0.05;

    let dust = gl_smoothstep(0.4, 0.85, ow_fbm01(p.scale(6.0), p_const.scale(6.0), 4, 0.5));
    c = mix3(c, ow_srgb(Vec3::new(0.400, 0.375, 0.335)), dust * 0.14);

    SurfaceSample {
        albedo: clamp3(c, 0.02, 0.85),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.5, 0.99),
        metal,
        ao: gl_clamp(ao, 0.25, 1.0),
    }
}

// ---------------------------------------------------------------------------
// BURLAP — surfaces-organic.js:201-257.
// ---------------------------------------------------------------------------

/// `BURLAP`'s `owSurface` (`surfaces-organic.js:202-256`): coarse 34-thread
/// hessian with per-thread irregular thickness, sun rot, loose standing
/// fibres, and sand caught in the weave.
pub fn burlap(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let threads = 34.0; // hessian is coarse
    let p = uv.mul(p_const).add_scalar(seed * 4.7);

    let t = uv.scale(threads);
    let cell = t.floor();
    let f = t.fract().add_scalar(-0.5);
    let over = gl_mod(cell.x + cell.y, 2.0);

    // hessian threads are irregular: each one has its own thickness
    let twx = 0.62 + 0.30 * ow_hash12(Vec2::new(cell.x, 0.0).add_scalar(seed));
    let twy = 0.62 + 0.30 * ow_hash12(Vec2::new(0.0, cell.y).add_scalar(seed * 1.7));
    let warp_p = (gl_clamp(f.x / twx, -0.5, 0.5) * 3.14159).cos();
    let weft_p = (gl_clamp(f.y / twy, -0.5, 0.5) * 3.14159).cos();
    let top = gl_mix(warp_p, weft_p, over);
    let bot = gl_mix(weft_p, warp_p, over) * 0.40;
    let weave = top.max(bot);

    let fibre = ow_fbm01(
        ow_shear(p.scale(12.0), 0.0, 8.0),
        ow_shear_per(p_const.scale(12.0), 8.0),
        3,
        0.5,
    );
    let macro_ = ow_fbm01(p.scale(1.0), p_const.scale(1.0), 4, 0.62);
    let dirt = ow_fbm01(
        ow_warp(p.scale(2.5), p_const.scale(2.5), 0.8, 3),
        p_const.scale(2.5),
        5,
        0.55,
    );

    let c_jute = ow_srgb(Vec3::new(0.520, 0.430, 0.275));
    let c_pale = ow_srgb(Vec3::new(0.640, 0.560, 0.400));
    let c_soil = ow_srgb(Vec3::new(0.230, 0.180, 0.120));
    let mut c = mix3(c_jute, c_pale, ow_hash12(cell.add_scalar(3.0)) * 0.5 + fibre * 0.15);
    c = c.scale(0.855 + 0.235 * (weave * 0.5 + 0.5));
    c = c.scale(0.90 + 0.18 * macro_);
    c = mix3(c, c_soil, gl_smoothstep(0.42, 0.85, dirt) * 0.60);

    let mut h = 0.50 + weave * 0.38 + (fibre - 0.5) * 0.05;
    let mut rough = 0.90 + (1.0 - weave) * 0.06;
    let metal = 0.0;
    let ao = gl_mix(0.74, 1.0, gl_smoothstep(-0.4, 0.9, weave));

    // sun rot: bleached and frayed on the exposed side
    let rot = gl_smoothstep(0.55, 0.9, ow_fbm01(p.scale(0.7).add_scalar(11.0), p_const.scale(0.7), 3, 0.6));
    c = mix3(c, c_pale.scale(1.15), rot * 0.4);
    rough += rot * 0.05;

    // loose fibres standing off the surface
    let loose = ow_scratches(p.scale(4.0), p_const.scale(4.0), 10.0, 2.0, 0.70);
    h += loose * 0.06;
    c = mix3(c, c_pale, loose * 0.3);

    // spilled sand caught in the weave
    let sand = gl_smoothstep(0.5, 0.85, ow_fbm01(p.scale(12.0), p_const.scale(12.0), 4, 0.5))
        * (1.0 - gl_smoothstep(0.2, 0.7, weave));
    c = mix3(c, ow_srgb(Vec3::new(0.640, 0.545, 0.390)), sand * 0.45);

    SurfaceSample {
        albedo: clamp3(c, 0.02, 0.80),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.6, 0.99),
        metal,
        ao: gl_clamp(ao, 0.2, 1.0),
    }
}

// ---------------------------------------------------------------------------
// FOLIAGE — surfaces-organic.js:259-328.
// ---------------------------------------------------------------------------

/// `FOLIAGE`'s `owSurface` (`surfaces-organic.js:260-327`): a 3x3 cell
/// neighbourhood, one rotated leaf per cell, a pinched ellipse with a
/// serrated edge, depth-sorted overlap, and veins.
///
/// **`h` here is the alpha-test cutout mask (`bestCover`), not a height** —
/// see the module doc.
pub fn foliage(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let cells = 5.0;
    let p = uv.mul(p_const).add_scalar(seed * 5.9);

    // Each cell holds one leaf, rotated and scaled by its hash. Sampling the
    // 3x3 neighbourhood lets leaves overlap into their neighbours' cells.
    let lp = uv.scale(cells);
    let ip = lp.floor();
    let fp = lp.fract();

    let mut best_cover = 0.0_f64;
    let mut best_depth = -1.0_f64;
    let mut best_col = Vec3::default();
    let mut best_vein = 0.0_f64;

    for y in -1..=1 {
        for x in -1..=1 {
            let g = Vec2::new(f64::from(x), f64::from(y));
            let cell = ip.add(g).modulo(Vec2::splat(cells));
            let r = ow_hash42(cell.add_scalar(seed * 2.0));
            // `cell * 1.7 + 9.0 + uSeed` — two sequential scalar adds, kept
            // in the source's left-to-right order rather than combined into
            // one `add_scalar(9.0 + seed)` (same real value, different float
            // rounding order).
            let r2 = ow_hash42(cell.scale(1.7).add_scalar(9.0).add_scalar(seed));
            let centre = g.add_scalar(0.15).add(Vec2::new(r.x, r.y).scale(0.7)).sub(fp);
            let ang = r.z * 6.28318;
            let q = ow_rot(centre, ang);
            // leaf shape: an ellipse pinched at both ends
            let s = Vec2::new(0.30 + r.w * 0.16, 0.13 + r2.x * 0.07);
            let e = Vec2::new(q.x / s.x, q.y / s.y);
            let d = e.length();
            let pinch = 1.0 - 0.55 * e.x.abs() * 0.5;
            // Source quirk, omitted (surfaces-organic.js:290-293): the source
            // computes `cover` once without serration and immediately
            // overwrites it with the serrated version below — the first
            // value never reaches any output, so only the live formula is
            // transcribed.
            let serr = (e.y.atan2(e.x) * 26.0).sin() * 0.03;
            let cover = gl_smoothstep(1.02 + serr, 0.88 + serr, d / pinch.max(0.3));
            if cover > 0.01 {
                let depth = r2.y;
                if depth > best_depth {
                    let vein = 1.0 - gl_smoothstep(0.0, 0.05, (e.y * s.y).abs());
                    let side_v =
                        gl_smoothstep(0.75, 1.0, (gl_fract(e.x * 5.0 + e.y * 2.0) * 2.0 - 1.0).abs());
                    let vein = gl_clamp(vein + side_v * 0.45 * cover, 0.0, 1.0);
                    let c_young = ow_srgb(Vec3::new(0.180, 0.330, 0.090));
                    let c_old = ow_srgb(Vec3::new(0.095, 0.185, 0.060));
                    let c_dry = ow_srgb(Vec3::new(0.390, 0.320, 0.110));
                    let mut lc = mix3(c_young, c_old, r2.z);
                    lc = mix3(lc, c_dry, gl_smoothstep(0.55, 1.0, r2.w) * 0.8);
                    // blotches and mildew spots
                    let spots = ow_fbm01(p.scale(22.0), p_const.scale(22.0), 3, 0.5);
                    lc = lc.scale(0.85 + 0.30 * spots);
                    lc = mix3(lc, c_dry.scale(0.7), gl_smoothstep(0.78, 0.95, spots) * 0.5);
                    lc = mix3(lc, lc.scale(1.35), vein * 0.5);
                    best_depth = depth;
                    best_cover = cover;
                    best_col = lc;
                    // Source quirk, preserved (surfaces-organic.js:313): `bestH`
                    // is computed every winning iteration but the final `h`
                    // below is `bestCover`, not `bestH` — it never reaches any
                    // output. See the module doc: foliage's `h` is a cutout
                    // mask, not a height. Computed and discarded here to keep
                    // the port line-for-line diffable against the source.
                    let best_h = 0.45 + depth * 0.35 + (1.0 - gl_smoothstep(0.0, 1.0, d)) * 0.12
                        + vein * 0.05;
                    let _ = best_h;
                    best_vein = vein;
                }
            }
        }
    }

    let fine = ow_fbm01(p.scale(12.0), p_const.scale(12.0), 3, 0.5);
    SurfaceSample {
        albedo: clamp3(best_col.scale(0.955 + 0.085 * fine), 0.02, 0.7),
        // h doubles as the cutout mask for foliage (see the module doc).
        height: best_cover,
        roughness: gl_clamp(0.62 + (1.0 - best_vein) * 0.14 + (fine - 0.5) * 0.10, 0.35, 0.95),
        metal: 0.0,
        ao: gl_clamp(0.55 + best_depth * 0.45, 0.3, 1.0),
    }
}

// ---------------------------------------------------------------------------
// RUBBER — surfaces-organic.js:330-380.
// ---------------------------------------------------------------------------

/// `RUBBER`'s `owSurface` (`surfaces-organic.js:331-379`): moulded pebble
/// grain, a mould seam, chalky abrasion scuffs, and ozone cracking.
pub fn rubber(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 9.6);

    // moulded pebble grain
    let pb = ow_worley(p.scale(12.0), p_const.scale(12.0), 1.0);
    let pebble = gl_smoothstep(0.42, 0.10, pb.f1);
    let fine = ow_fbm01(p.scale(12.0), p_const.scale(12.0), 3, 0.5);
    let macro_ = ow_fbm01(p.scale(1.5), p_const.scale(1.5), 4, 0.6);

    let mut h = 0.60 + pebble * 0.10 + (fine - 0.5) * 0.02 + (macro_ - 0.5) * 0.03;
    // 0.20 sRGB ~= 0.031 linear. Anything darker lands under the 0.02 albedo
    // floor applied below, which clamps the entire surface flat (a black,
    // detail-free rubber that violates the "no flat surfaces" bar).
    let mut c = ow_srgb(Vec3::new(0.200, 0.200, 0.206));
    c = c.scale(0.85 + 0.25 * (pebble * 0.5 + 0.5));
    c = c.scale(0.94 + 0.10 * fine);

    let mut rough = 0.88 - pebble * 0.06 + (fine - 0.5) * 0.08;
    let mut ao = gl_mix(0.6, 1.0, pebble * 0.5 + 0.5);

    // mould seam
    let seam = 1.0 - gl_smoothstep(0.0, 0.012, (gl_fract(uv.y * 2.0 + 0.5) - 0.5).abs());
    h += seam * 0.03;
    c = c.scale(1.0 + seam * 0.35);
    rough -= seam * 0.10;

    // scuffs: rubber goes chalky-grey where it abrades
    let scuff = gl_smoothstep(
        0.55,
        0.88,
        ow_fbm01(
            ow_warp(p.scale(3.0), p_const.scale(3.0), 0.8, 3),
            p_const.scale(3.0),
            4,
            0.55,
        ),
    );
    c = mix3(c, ow_srgb(Vec3::new(0.220, 0.218, 0.212)), scuff * 0.45);
    rough += scuff * 0.06;
    h -= scuff * 0.015;

    // cracking from ozone / age
    let crack = ow_cracks(p.scale(7.0), p_const.scale(7.0), 0.9, 0.028, 0.62);
    h -= crack * 0.06;
    c = c.scale(1.0 - crack * 0.35);
    ao -= crack * 0.35;

    // dust
    let dust = gl_smoothstep(0.5, 0.9, ow_fbm01(p.scale(8.0), p_const.scale(8.0), 4, 0.5));
    c = mix3(c, ow_srgb(Vec3::new(0.290, 0.275, 0.250)), dust * 0.16);

    SurfaceSample {
        albedo: clamp3(c, 0.02, 0.35),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.55, 0.99),
        metal: 0.0,
        ao: gl_clamp(ao, 0.3, 1.0),
    }
}

// ---------------------------------------------------------------------------
// GLASS — surfaces-organic.js:382-415.
// ---------------------------------------------------------------------------

/// `GLASS`'s `owSurface` (`surfaces-organic.js:383-414`): near-black albedo —
/// the look is carried entirely by roughness (smear, dust, water spots,
/// scratches).
pub fn glass(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 2.2);

    let smear = ow_fbm01(
        ow_shear(p.scale(3.0), 1.0, 6.0),
        ow_shear_per(p_const.scale(3.0), 6.0),
        4,
        0.5,
    );
    let dust_f = ow_fbm01(p.scale(5.0), p_const.scale(5.0), 5, 0.55);
    let spots = ow_worley(p.scale(24.0), p_const.scale(24.0), 1.0).f1;
    let fine = ow_fbm01(p.scale(12.0), p_const.scale(12.0), 3, 0.5);

    // glass itself is almost black in albedo; the look comes from reflections
    let mut c = ow_srgb(Vec3::new(0.045, 0.050, 0.052));

    let dirty = gl_smoothstep(0.45, 0.85, dust_f);
    c = mix3(c, ow_srgb(Vec3::new(0.300, 0.290, 0.265)), dirty * 0.35);

    let mut rough = 0.045 + smear * 0.10 * gl_smoothstep(0.3, 0.9, dust_f) + dirty * 0.22;
    rough += gl_smoothstep(0.30, 0.05, spots) * 0.25; // water spots
    rough += (fine - 0.5) * 0.02;

    // fine scratches
    let scr = ow_scratches(p.scale(2.0), p_const.scale(2.0), 24.0, 1.0, 0.70);
    rough += scr * 0.25;
    c = c.add_scalar(scr * 0.02);

    let h = 0.5 + (smear - 0.5) * 0.004;
    let ao = 1.0 - dirty * 0.1;

    SurfaceSample {
        albedo: clamp3(c, 0.02, 0.5),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.02, 0.7),
        metal: 0.0,
        ao,
    }
}
