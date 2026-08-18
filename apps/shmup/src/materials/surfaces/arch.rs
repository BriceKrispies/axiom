//! Ported from Claude-of-Duty `src/materials/glsl/surfaces-arch.js:1-563`.
//!
//! Four architectural `owSurface` generators: [`concrete_surface`] (also
//! backs the `concrete_floor` library entry — see below), [`brick_surface`],
//! [`plaster_surface`], [`tile_surface`]. Each is `owSurface(uv) -> (albedo,
//! height, roughness, metal, ao)`, ported line-for-line as CPU `f64` maths on
//! top of [`crate::materials::noise`] (the periodic noise basis every
//! generator in this port is built from — read that module first) and
//! returning a [`SurfaceSample`], the same contract [`crate::materials::bake`]
//! already defined for `DETAIL_SRC`/`MACRO_SRC`.
//!
//! ## `concrete_floor` is `concrete_surface` with different `uParam`, not a
//! separate generator
//!
//! `mod.rs::LIBRARY`'s `concrete` entry uses `param: [1.0, 0.0, 0.0, 0.0]`
//! (board-formed wall) and `concrete_floor` uses `param: [0.0, 1.0, 0.0,
//! 0.0]` (poured slab with saw-cut joints) — both point at
//! `generator: "concrete"` in the source's own library. [`concrete_surface`]
//! takes the full `uParam` (as [`Vec4`]) and reads `.x` (`formAmt`: board
//! lines + tie-rod holes) and `.y` (`jointAmt`: saw-cut control joints +
//! power-float swirl) exactly as the source does; `brick`/`plaster`/`tile`
//! never reference `uParam` in their GLSL bodies, so their Rust signatures
//! only take `uv` and `seed`.
//!
//! ## The Nyquist budget, preserved exactly
//!
//! Every generator writes `p = uv * P` with `P = vec2(8.0)`, so a term at
//! `p * K` lays `8K` cells across a bake — e.g. concrete's sand fraction at
//! `p * 20.0` is 160 cells across a 1024 texel bake, ~6.4 texels per cell,
//! right at the edge of what resolves. None of these frequency constants are
//! "cleaned up": they are the resolvable-detail budget the source's own
//! `DETAIL_SRC` doc (`generator.js:80-90`) states explicitly, and per the
//! port recipe they are preserved verbatim.
//!
//! ## Transcription risk: this GLSL has no native oracle
//!
//! `surfaces-arch.js` embeds these bodies as GLSL strings inside a JS
//! template literal, the same shape `noise.js`/`generator.js` use — there is
//! no importable JS function to call as ground truth. The capture script at
//! `tests/materials_arch/capture.mjs` re-implements each `owSurface` body in
//! plain JS doubles, transcribed line-by-line from this same source file
//! rather than tidied or reorganized, so a mistake in either the Rust port or
//! the JS capture is a **transcription** risk on both sides, not caught by
//! the other. See that file's header comment and
//! `docs/work-manifests/shmup-port/notes/materials-surfaces-arch.md`
//! for the full caveat.
//!
//! ## GLSL -> Rust notes specific to this file
//!
//! - `owWorley(...)`'s GLSL swizzle `.x`/`.y`/`.z`/`.w` reads as
//!   `f1`/`f2`/`id_x`/`id_y` on [`super::noise::WorleyResult`] — documented
//!   once there, relied on silently at every `owWorley` call site here.
//! - `mix(a, b, t)` on a `vec3` is [`v3_mix`] (componentwise
//!   [`super::noise::gl_mix`]); `clamp(v, vec3(lo), vec3(hi))` (always a
//!   uniform per-channel bound in this file) is [`v3_clamp`]. Neither lives
//!   in `noise.rs`, which mirrors `noise.js` function-for-function and has no
//!   `vec3`-level `mix`/`clamp` of its own (see that module's doc for why
//!   there's no generic vector-algebra layer).
//! - `step(edge, x)` is a bare GLSL builtin (not one of `noise.js`'s named
//!   functions), ported here as a local `gl_step` — same reasoning
//!   `bake.rs`'s own `gl_step` documents.
//! - No `sign()`, no rotation, and no negative-lattice-coordinate `mod()`
//!   subtlety beyond what `noise.rs` already guards inside `ow_hash`/`ow_fbm`
//!   — this file's own two direct `mod()` calls (`brick`'s `mod(row, 2.0)`,
//!   `mod(col, COLS)`) go through [`super::noise::gl_mod`], not Rust's `%`.

use super::super::bake::SurfaceSample;
use super::super::noise::{
    gl_clamp, gl_fract, gl_mix, gl_mod, gl_smoothstep, ow_cracks, ow_fbm01, ow_hash11, ow_hash12,
    ow_hash42, ow_shear, ow_shear_per, ow_srgb, ow_warp, ow_worley, Vec2, Vec3, Vec4,
};

// ---------------------------------------------------------------------------
// Local helpers — GLSL builtins/vector algebra `noise.rs` doesn't define
// (see the module doc for why they live here instead of there).
// ---------------------------------------------------------------------------

/// GLSL `step(edge, x)`: `0.0` when `x < edge`, else `1.0`.
fn gl_step(edge: f64, x: f64) -> f64 {
    if x < edge {
        0.0
    } else {
        1.0
    }
}

/// Componentwise `vec2 abs(vec2)`.
fn v2_abs(v: Vec2) -> Vec2 {
    Vec2::new(v.x.abs(), v.y.abs())
}

/// GLSL `mix(vec3 a, vec3 b, float t)` — componentwise [`gl_mix`].
fn v3_mix(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    Vec3::new(gl_mix(a.x, b.x, t), gl_mix(a.y, b.y, t), gl_mix(a.z, b.z, t))
}

/// GLSL `vec3 + vec3` — componentwise add. [`super::super::noise::Vec3`] has
/// no `add` (only `add_scalar`, since none of its other callers need
/// `vec3 + vec3`); this file's one use site (BRICK's chip colour blend)
/// needs it.
fn v3_add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// GLSL `clamp(vec3 v, vec3(lo), vec3(hi))` — every call site in this file
/// clamps to a uniform per-channel bound, so `lo`/`hi` are scalars rather
/// than `Vec3`.
fn v3_clamp(v: Vec3, lo: f64, hi: f64) -> Vec3 {
    Vec3::new(gl_clamp(v.x, lo, hi), gl_clamp(v.y, lo, hi), gl_clamp(v.z, lo, hi))
}

// ---------------------------------------------------------------------------
// CONCRETE — `surfaces-arch.js:15-179`. Also backs `concrete_floor`
// (`param.y = 1`) — see the module doc.
// ---------------------------------------------------------------------------

/// `owSurface` for `concrete` / `concrete_floor`. `param.x` = `uParam.x`
/// (board-formed wall amount), `param.y` = `uParam.y` (saw-cut joint /
/// power-float amount) — `.z`/`.w` are carried (matching the source's full
/// `uParam` uniform) but never read, exactly as the GLSL body never reads
/// them either.
pub fn concrete_surface(uv: Vec2, seed: f64, param: Vec4) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 13.7);

    // ---- base tone: pour variation, wet/dry patches, cement bloom ----
    let macro_ = ow_fbm01(p.scale(0.5), p_const.scale(0.5), 4, 0.58);
    let mid = ow_fbm01(
        ow_warp(p.scale(2.0), p_const.scale(2.0), 0.7, 3),
        p_const.scale(2.0),
        5,
        0.5,
    );
    let fine = ow_fbm01(p.scale(18.0), p_const.scale(18.0), 4, 0.5);
    let micro = ow_fbm01(p.scale(26.0), p_const.scale(26.0), 3, 0.5);

    let c_light = ow_srgb(Vec3::new(0.520, 0.512, 0.492));
    let c_mid = ow_srgb(Vec3::new(0.395, 0.392, 0.385));
    let c_dark = ow_srgb(Vec3::new(0.255, 0.253, 0.258));
    let mut c = v3_mix(c_mid, c_light, gl_smoothstep(0.35, 0.85, macro_));
    c = v3_mix(c, c_dark, gl_smoothstep(0.55, 0.95, mid) * 0.55);
    c = c.scale(0.93 + 0.14 * fine);
    // The 0.1-1 m band — see PLASTER's long note. Pour blotching and the
    // wash of dirt that runs over any concrete left outdoors.
    // contrast-expanded: see the note in PLASTER
    let mut pour_b = ow_fbm01(
        ow_warp(p.scale(1.5).add_scalar(8.3), p_const.scale(1.5), 0.6, 3),
        p_const.scale(1.5),
        4,
        0.58,
    );
    pour_b = gl_clamp((pour_b - 0.5) * 2.5 + 0.5, 0.0, 1.0);
    c = c.scale(0.82 + 0.38 * pour_b);
    let mut wash = ow_fbm01(p.scale(7.0).add_scalar(2.0), p_const.scale(7.0), 4, 0.5);
    wash = gl_clamp((wash - 0.5) * 2.2 + 0.5, 0.0, 1.0);
    c = c.scale(0.925 + 0.155 * wash);

    let mut h = 0.62 + (fine - 0.5) * 0.035 + (mid - 0.5) * 0.05;
    let mut rough = 0.70 + (mid - 0.5) * 0.16 + (micro - 0.5) * 0.07;
    let mut ao = 1.0;
    let metal = 0.0;

    // ---- exposed aggregate: stone chips sitting just under the skin ----
    let agg = ow_worley(p.scale(13.0), p_const.scale(13.0), 0.95);
    let agg_shape = gl_smoothstep(0.46, 0.10, agg.f1);
    let agg_rnd = agg.id_x;
    // Only some chips break the surface.
    let agg_exposed = agg_shape
        * gl_step(
            0.74,
            ow_fbm01(p.scale(3.0).add_scalar(5.0), p_const.scale(3.0), 3, 0.5) + agg_rnd * 0.35,
        );
    h += agg_exposed * 0.022 * (0.5 + agg_rnd);
    c = v3_mix(
        c,
        v3_mix(
            ow_srgb(Vec3::new(0.335, 0.320, 0.300)),
            ow_srgb(Vec3::new(0.560, 0.545, 0.505)),
            agg_rnd,
        ),
        agg_exposed * 0.7,
    );
    rough += agg_exposed * 0.07 * (agg_rnd - 0.5);

    // ---- coarse sand fraction: the 5-8 mm grit of the cement skin ----
    // The 0.5-2 mm tooth is NOT authored here — see the source's comment
    // (surfaces-arch.js:58-62): a sub-texel grain bakes as white noise and
    // mips to grey; it belongs to the shared detail map instead.
    let sand = ow_worley(p.scale(20.0), p_const.scale(20.0), 1.0);
    let sand_m = gl_smoothstep(0.44, 0.05, sand.f1);
    let sand_sel = 0.40 + 0.60 * gl_step(0.30, sand.id_x);
    h += sand_m * sand_sel * 0.028;
    c = c.scale(1.0 + (sand_m * sand_sel - 0.20) * 0.15);
    rough += (sand.id_x - 0.5) * 0.11 + sand_m * 0.04;
    ao -= sand_m * 0.06;
    let sand_trough = gl_smoothstep(0.52, 0.88, sand.f1);
    c = v3_mix(c, c.scale(0.86), sand_trough * 0.34);

    // ---- air pockets / bug holes from the pour ----
    let pores = ow_worley(p.scale(22.0), p_const.scale(22.0), 1.0);
    let pore = gl_smoothstep(0.26, 0.0, pores.f1) * gl_step(0.84, pores.id_y);
    h -= pore * 0.055;
    ao -= pore * 0.55;
    rough += pore * 0.10;

    // uParam.x = board-formed wall (1) vs poured slab (0)
    // uParam.y = saw-cut control joints, for floors
    let form_amt = param.x;
    let joint_amt = param.y;

    // ---- formwork: horizontal board lines + tie-rod holes ----
    let boards = uv.y * 4.0;
    let bi = boards.floor();
    let bf = gl_fract(boards);
    let mut seam = (1.0 - gl_smoothstep(0.0, 0.030, bf)) + (1.0 - gl_smoothstep(0.0, 0.030, 1.0 - bf));
    seam = gl_clamp(seam, 0.0, 1.0);
    // Boards are never perfectly aligned: each course steps a fraction of a mm.
    let board_step = (ow_hash11(bi + seed) - 0.5) * 0.028 * form_amt;
    h += board_step;
    h -= seam * 0.055 * form_amt;
    ao -= seam * 0.40 * form_amt;
    c = c.scale(1.0 - seam * 0.16 * form_amt);
    // cement bled along the seam and set lighter
    let bleed = (1.0 - gl_smoothstep(0.0, 0.10, (bf - 0.02).abs())) * 0.5 * form_amt;
    c = v3_mix(
        c,
        c_light.scale(1.05),
        bleed * 0.35 * ow_fbm01(p.scale(8.0), p_const.scale(8.0), 3, 0.5),
    );

    // tie holes, plugged, one every other board
    let tf = Vec2::new(uv.x * 3.0, boards * 0.5).fract().add_scalar(-0.5);
    let tie_rnd = ow_hash12(Vec2::new(uv.x * 3.0, boards * 0.5).floor().add_scalar(seed));
    let tie = gl_smoothstep(0.085, 0.05, tf.mul(Vec2::new(1.0, 2.0)).length())
        * gl_step(0.45, tie_rnd)
        * form_amt;
    h -= tie * 0.10;
    ao -= tie * 0.5;
    c = v3_mix(c, c_dark.scale(0.85), tie * 0.6);

    // ---- saw-cut control joints (slabs) + power-float polish ----
    let jd = v2_abs(uv.add_scalar(0.5).fract().add_scalar(-0.5));
    let mut joint = f64::max(1.0 - gl_smoothstep(0.0035, 0.010, jd.x), 1.0 - gl_smoothstep(0.0035, 0.010, jd.y));
    joint *= joint_amt;
    h -= joint * 0.10;
    ao -= joint * 0.55;
    c = v3_mix(c, c_dark.scale(0.62), joint * 0.65);
    // trowel arcs left by the power float
    let swirl = ow_fbm01(
        ow_warp(p.scale(1.1).add_scalar(3.0), p_const.scale(1.1), 1.4, 3),
        p_const.scale(1.1),
        3,
        0.6,
    );
    rough -= joint_amt * gl_smoothstep(0.35, 0.85, swirl) * 0.10;
    c = c.scale(1.0 - joint_amt * gl_smoothstep(0.4, 0.9, swirl) * 0.07);

    // ---- structural cracks: branch from the seams and corners ----
    let crk = ow_cracks(p.scale(2.6), p_const.scale(2.6), 0.85, 0.028, 0.50);
    let crk_fine = ow_cracks(p.scale(7.0).add_scalar(31.0), p_const.scale(7.0), 0.9, 0.020, 0.60) * 0.55;
    let crack = gl_clamp(crk + crk_fine, 0.0, 1.0);
    h -= crack * 0.12;
    ao -= crack * 0.45;
    c = v3_mix(c, c_dark.scale(0.80), crack * 0.42);
    rough += crack * 0.12;

    // ---- spalling: a chunk of the skin has broken off, aggregate showing ----
    let sp = ow_worley(p.scale(1.1).add_scalar(7.3), p_const.scale(1.1), 0.9);
    let spall_cell = gl_step(0.90, sp.id_y);
    let spall = spall_cell
        * gl_smoothstep(0.44, 0.16, sp.f1)
        * gl_smoothstep(
            0.42,
            0.62,
            ow_fbm01(p.scale(4.0).add_scalar(2.0), p_const.scale(4.0), 4, 0.5),
        );
    h -= spall * 0.13;
    ao -= spall * 0.35;
    c = v3_mix(c, v3_mix(c_dark, c_mid, agg_rnd).scale(0.88), spall * 0.8);
    rough += spall * 0.10;
    // rim of the spall catches light
    let spall_rim = spall * (1.0 - spall) * 4.0;
    c = c.scale(1.0 + spall_rim * 0.10);

    // ---- small chips: 2-5 cm bites out of the skin showing darker, wetter
    //      concrete plus the sand fraction underneath (~3% of the surface) ----
    let ck = ow_worley(
        ow_warp(p.scale(5.6).add_scalar(19.0), p_const.scale(5.6), 0.6, 3),
        p_const.scale(5.6),
        0.95,
    );
    let ck_sel = gl_step(0.90, ck.id_y);
    let ck_size = 0.20 + 0.16 * ck.id_x;
    let ck_shape = gl_smoothstep(
        ck_size,
        ck_size * 0.3,
        ck.f1 * (0.72 + 0.56 * ow_fbm01(p.scale(16.0), p_const.scale(16.0), 3, 0.5)),
    );
    let chip = ck_sel * ck_shape;
    c = v3_mix(
        c,
        v3_mix(c.scale(0.74), v3_mix(c_dark, c_mid, sand.id_x), 0.5),
        chip * 0.85,
    );
    h -= chip * 0.045;
    ao -= chip * 0.24;
    rough += chip * 0.08;
    let ck_lip = (ck_sel * (gl_smoothstep(ck_size * 1.25, ck_size, ck.f1) - ck_shape)).max(0.0);
    c = c.scale(1.0 + ck_lip * 0.10);

    // ---- staining: rain runoff, soot, rust bleed from rebar ----
    let streak = ow_fbm01(
        Vec2::new(p.x * 6.0, p.y * 2.0),
        Vec2::new(p_const.x * 6.0, p_const.y * 2.0),
        5,
        0.55,
    );
    let runoff = gl_smoothstep(0.58, 0.95, streak) * (0.35 + 0.65 * gl_smoothstep(0.2, 0.8, macro_));
    c = c.scale(1.0 - runoff * 0.14);
    rough += runoff * 0.05;

    let rust_bleed = gl_smoothstep(0.72, 0.98, streak * (0.6 + 0.5 * tie_rnd)) * gl_step(0.80, tie_rnd);
    c = v3_mix(c, ow_srgb(Vec3::new(0.42, 0.24, 0.12)), rust_bleed * 0.45);

    // dirt collects in every recess
    let cavity = 1.0 - gl_smoothstep(0.42, 0.66, h);
    c = v3_mix(c, ow_srgb(Vec3::new(0.20, 0.19, 0.17)), cavity * 0.35);

    SurfaceSample {
        albedo: v3_clamp(c, 0.02, 0.85),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.48, 0.98),
        metal,
        ao: gl_clamp(ao, 0.15, 1.0),
    }
}

// ---------------------------------------------------------------------------
// BRICK — `surfaces-arch.js:181-336`.
// ---------------------------------------------------------------------------

/// `owSurface` for `brick`.
pub fn brick_surface(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    const COLS: f64 = 6.0; // bricks across the tile
    const ROWS: f64 = 18.0; // courses up the tile
    let p = uv.mul(p_const).add_scalar(seed * 9.1);

    // ---------------- brick lattice, running bond ----------------
    let row_f = uv.y * ROWS;
    let row = row_f.floor();
    let col_f = uv.x * COLS + gl_mod(row, 2.0) * 0.5;
    let col = col_f.floor();
    let id = Vec2::new(gl_mod(col, COLS), row);
    let f = Vec2::new(gl_fract(col_f), gl_fract(row_f));

    let rnd = ow_hash42(id.add_scalar(seed * 3.0));
    let rnd2 = ow_hash42(id.scale(1.37).add_scalar(21.0 + seed));
    let rnd3 = ow_hash42(id.scale(0.73).add_scalar(7.7 + seed * 1.9));

    // Bricks are laid by hand: each one is a hair off square.
    let jitter = Vec2::new(rnd.x - 0.5, rnd.y - 0.5).mul(Vec2::new(0.012, 0.030));
    let fj = f.add(jitter);

    // joint thickness (10mm of a 225mm x 75mm course). The joint is *raked*: a
    // flat mortar bed with a hard arris at the brick edge. Ramping across the
    // whole joint width is what makes mortar read as a painted line.
    const JX: f64 = 0.048;
    const JY: f64 = 0.135;
    let dxj = fj.x.min(1.0 - fj.x);
    let dyj = fj.y.min(1.0 - fj.y);
    let shoulder = 0.74 + 0.16 * rnd3.w; // some joints struck flush, some sharp
    let ex = gl_smoothstep(JX * shoulder, JX * 1.02, dxj);
    let ey = gl_smoothstep(JY * shoulder, JY * 1.02, dyj);
    let face = ex.min(ey); // 1 = brick face, 0 = mortar

    // per-brick surface coords so the face texture never repeats
    let bp = Vec2::new(fj.x, fj.y).mul(Vec2::new(3.0, 1.0)).add(Vec2::new(rnd.z, rnd.w).scale(17.0));
    let bp_const = Vec2::splat(24.0);

    // ---------------- mortar ----------------
    let m_sand = ow_fbm01(p.scale(20.0), p_const.scale(20.0), 4, 0.5);
    let m_grain = ow_worley(p.scale(24.0), p_const.scale(24.0), 1.0);
    let mortar_rough = ow_fbm01(p.scale(20.0), p_const.scale(20.0), 4, 0.55);
    let mut mortar_col = v3_mix(
        ow_srgb(Vec3::new(0.400, 0.388, 0.362)),
        ow_srgb(Vec3::new(0.278, 0.272, 0.260)),
        gl_smoothstep(0.3, 0.8, mortar_rough),
    );
    mortar_col = mortar_col.scale(0.84 + 0.32 * m_sand);
    mortar_col = mortar_col.scale(0.88 + 0.24 * ow_fbm01(p.scale(6.0), p_const.scale(6.0), 4, 0.6));
    mortar_col = v3_mix(
        mortar_col,
        ow_srgb(Vec3::new(0.235, 0.228, 0.215)),
        gl_smoothstep(0.5, 0.06, m_grain.f1) * 0.40,
    );
    mortar_col = v3_mix(
        mortar_col,
        ow_srgb(Vec3::new(0.520, 0.505, 0.470)),
        gl_smoothstep(
            0.30,
            0.02,
            ow_worley(p.scale(25.0).add_scalar(4.0), p_const.scale(25.0), 1.0).f1,
        ) * 0.35,
    );

    // some joints are struck flush, some are raked deep, some crumbled out.
    // 0.10-0.15 of a 0.055 m relief = 5-8 mm of real recess.
    let mut joint_depth = 0.10 + 0.05 * ow_fbm01(p.scale(1.2), p_const.scale(1.2), 3, 0.5);
    let crumble = gl_smoothstep(0.62, 0.86, ow_fbm01(p.scale(9.0).add_scalar(4.0), p_const.scale(9.0), 4, 0.5));
    joint_depth += crumble * 0.09;
    // the mortar bed itself is not flat — it holds the trowel's sand texture
    let mortar_h = -(m_sand - 0.5) * 0.018 - gl_smoothstep(0.5, 0.0, m_grain.f1) * 0.012;

    // ---------------- brick face ----------------
    let face_n = ow_fbm01(bp.scale(2.2), bp_const, 5, 0.5);
    let face_fine = ow_fbm01(bp.scale(5.0), bp_const.scale(2.0), 4, 0.5);
    let face_pore = ow_worley(bp.scale(7.0), bp_const.scale(3.5), 1.0);
    // Pits cluster instead of forming an even dot grid, and their size varies.
    let pore_cluster = gl_smoothstep(0.42, 0.78, ow_fbm01(bp.scale(3.0).add_scalar(8.0), bp_const.scale(1.5), 4, 0.55));
    let pore = gl_smoothstep(0.26 + 0.16 * face_pore.id_x, 0.0, face_pore.f1)
        * gl_step(0.55, face_pore.id_y)
        * pore_cluster;

    // Colour families: red stock, dark burnt header, pale sand-lime, brown.
    let c_a = ow_srgb(Vec3::new(0.430, 0.238, 0.183)); // red stock
    let c_b = ow_srgb(Vec3::new(0.318, 0.183, 0.150)); // deep red
    let c_c = ow_srgb(Vec3::new(0.196, 0.132, 0.120)); // burnt header
    let c_d = ow_srgb(Vec3::new(0.492, 0.392, 0.300)); // sandy
    let c_e = ow_srgb(Vec3::new(0.372, 0.288, 0.218)); // brown

    let mut brick = v3_mix(c_a, c_b, rnd.z);
    brick = v3_mix(brick, c_c, gl_step(0.90, rnd.w) * 0.70);
    brick = v3_mix(brick, c_d, gl_step(0.94, rnd2.x) * 0.62);
    brick = v3_mix(brick, c_e, gl_step(0.55, rnd2.y) * 0.50);
    // every brick came out of the kiln a different shade: +/-12% per brick
    brick = brick.scale(0.88 + 0.24 * rnd3.x);
    // within-brick banding from the extrusion
    brick = brick.scale(0.86 + 0.28 * face_n);
    // fine sand grain across the face — this is what reads at 0.5 m
    let face_grain = ow_fbm01(bp.scale(8.0), bp_const.scale(4.0), 4, 0.55);
    brick = brick.scale(0.87 + 0.26 * face_grain);
    brick = v3_mix(brick, brick.scale(1.22), gl_smoothstep(0.55, 0.9, face_fine) * 0.5);
    // dark iron spots and sand inclusions
    brick = v3_mix(brick, brick.scale(0.62), pore * 0.85);
    brick = v3_mix(
        brick,
        brick.scale(0.72),
        gl_smoothstep(0.34, 0.0, face_pore.f1) * gl_step(0.86, face_pore.id_x),
    );
    brick = v3_mix(brick, ow_srgb(Vec3::new(0.62, 0.58, 0.50)), gl_smoothstep(0.86, 0.98, face_fine) * 0.35);

    let mut face_h = 0.72 + (face_n - 0.5) * 0.05 + (face_fine - 0.5) * 0.025 + (rnd2.z - 0.5) * 0.05; // each brick sits proud/shy
    face_h -= pore * 0.075;

    // Broken arrises: ~5% of the edge length is knocked off, deep enough to
    // catch a shadow, showing pale raw clay under the fired skin.
    let edge_d = (dxj / JX).min(dyj / JY);
    let chip_noise = ow_fbm01(bp.scale(6.0).add_scalar(3.0), bp_const.scale(3.0), 4, 0.5);
    let chip = gl_smoothstep(1.7, 0.30, edge_d) * gl_smoothstep(0.60, 0.80, chip_noise) * gl_step(0.66, rnd3.z);
    face_h -= chip * 0.17;
    brick = v3_mix(brick, v3_add(brick.scale(0.72), ow_srgb(Vec3::new(0.20, 0.13, 0.09))), chip * 0.65);

    // ---------------- combine face + mortar ----------------
    // face is already a shaped profile, so no second smoothstep here: that is
    // what used to smear the arris across the full joint width.
    let m = face;
    let mut h = gl_mix(0.72 - joint_depth + mortar_h, face_h, m);
    let mut c = v3_mix(mortar_col, brick, m);
    // every brick came out of the kiln with a slightly different skin
    let brick_rough = 0.58 + 0.32 * rnd2.z + (rnd3.y - 0.5) * 0.20;
    let mut rough = gl_mix(
        0.88 + 0.10 * m_sand + 0.06 * (mortar_rough - 0.5),
        brick_rough + 0.14 * face_n + 0.10 * (face_grain - 0.5) + chip * 0.14,
        m,
    );
    let mut ao = gl_mix(0.34, 1.0, gl_smoothstep(0.0, 0.75, face));
    ao -= chip * 0.30;
    let metal = 0.0;

    // mortar smeared over the brick edge by the trowel
    let smear =
        gl_smoothstep(0.5, 1.0, 1.0 - face) * gl_smoothstep(0.55, 0.9, ow_fbm01(p.scale(14.0), p_const.scale(14.0), 4, 0.5));
    c = v3_mix(c, mortar_col.scale(1.05), smear * 0.5);

    // ---------------- weathering over the whole wall ----------------
    // The 0.1-1 m band — see the long note in PLASTER.
    let mut soil_b = ow_fbm01(
        ow_warp(p.scale(1.8).add_scalar(27.0), p_const.scale(1.8), 0.6, 3),
        p_const.scale(1.8),
        4,
        0.58,
    );
    soil_b = gl_clamp((soil_b - 0.5) * 2.5 + 0.5, 0.0, 1.0);
    c = c.scale(0.845 + 0.33 * soil_b);

    // efflorescence: salt bloom, strongest around joints
    let mut efflo = gl_smoothstep(
        0.62,
        0.96,
        ow_fbm01(ow_warp(p.scale(2.6), p_const.scale(2.6), 0.8, 3), p_const.scale(2.6), 4, 0.5),
    );
    efflo *= gl_mix(1.0, 0.35, m);
    c = v3_mix(c, ow_srgb(Vec3::new(0.66, 0.652, 0.632)), efflo * 0.5);
    rough += efflo * 0.10;

    // soot / rain runoff — short, shallow and only ~3:1 stretched; the long
    // runs are added at runtime where a real ledge sheds water.
    let streak = ow_fbm01(
        Vec2::new(p.x * 7.0, p.y * 2.3),
        Vec2::new(p_const.x * 7.0, p_const.y * 2.0),
        5,
        0.55,
    );
    let runoff = gl_smoothstep(0.50, 0.92, streak);
    c = c.scale(1.0 - runoff * 0.16);

    // hairline cracks stepping through the joints
    let crack = ow_cracks(p.scale(2.2), p_const.scale(2.2), 0.85, 0.038, 0.58);
    h -= crack * 0.10;
    ao -= crack * 0.45;
    c = v3_mix(c, c.scale(0.35), crack * 0.7);

    // dirt in every crevice
    let cavity = 1.0 - gl_smoothstep(0.50, 0.74, h);
    c = v3_mix(c, ow_srgb(Vec3::new(0.16, 0.15, 0.14)), cavity * 0.32);

    SurfaceSample {
        albedo: v3_clamp(c, 0.02, 0.85),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.35, 0.99),
        metal,
        ao: gl_clamp(ao, 0.12, 1.0),
    }
}

// ---------------------------------------------------------------------------
// PLASTER — `surfaces-arch.js:338-499`.
// ---------------------------------------------------------------------------

/// `owSurface` for `plaster` (aliased as `stucco`).
pub fn plaster_surface(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 5.3);

    // trowel: broad sweeps, anisotropic, with a fine skim on top
    let sw = ow_shear(p.scale(1.5), 1.0, 3.0);
    let trowel = ow_fbm01(sw, ow_shear_per(p_const.scale(1.5), 3.0), 5, 0.55);
    let skim = ow_fbm01(p.scale(12.0), p_const.scale(12.0), 5, 0.5);
    let micro = ow_fbm01(p.scale(24.0), p_const.scale(24.0), 3, 0.5);
    let macro_ = ow_fbm01(p.scale(0.6), p_const.scale(0.6), 3, 0.6);

    let c_base = ow_srgb(Vec3::new(0.598, 0.578, 0.538));
    let c_warm = ow_srgb(Vec3::new(0.512, 0.462, 0.395));
    let c_grey = ow_srgb(Vec3::new(0.382, 0.378, 0.372));
    let mut c = v3_mix(c_base, c_warm, gl_smoothstep(0.3, 0.8, macro_));
    c = c.scale(0.94 + 0.12 * skim);
    c = v3_mix(c, c_grey, gl_smoothstep(0.45, 0.95, trowel) * 0.42);
    c = v3_mix(c, c_base.scale(1.10), gl_smoothstep(0.55, 0.15, trowel) * 0.30);

    let mut h = 0.70 + (trowel - 0.5) * 0.10 + (skim - 0.5) * 0.030 + (micro - 0.5) * 0.012;
    let mut rough = 0.80 + (skim - 0.5) * 0.12 - gl_smoothstep(0.5, 0.9, trowel) * 0.10;
    let mut ao = 1.0;
    let metal = 0.0;

    // ---- skim-coat laps ------------------------------------------------------
    // A plasterer works the wall in ~40 cm passes, and every pass sets a hair
    // lighter or darker than the one before with a faint arris where the
    // trowel lifted off — the mid-frequency signal that separates plaster
    // from paint at 2-5 m.
    let lap_uv = ow_shear(p.scale(0.7), 1.0, 1.0);
    let lap_f = lap_uv.y + ow_fbm01(p.scale(1.1), p_const.scale(1.1), 3, 0.6) * 1.4;
    let lap_i = lap_f.floor();
    let lap_t = gl_fract(lap_f);
    let lap_r = ow_hash11(lap_i * 1.71 + seed * 2.3);
    c = c.scale(0.885 + 0.240 * lap_r);
    rough += (lap_r - 0.5) * 0.10;

    /*
     * THE 0.1-1 m BAND. A wall seen from 2-3 m fills the frame with about
     * half a metre of itself, a hole in the frequency budget: the macro
     * layer varies over 4-12 m and the detail map over 10 mm, so between
     * them the surface has nothing and reads as a flat colour with a
     * sprinkle of specks. Damp bloom, hand-height soiling, and a soft dirt
     * wash sit at 15-90 cm and are what actually makes a plastered wall read
     * as plaster.
     *
     * NB the contrast expansion. A 4-octave fbm01 spans about 0.3-0.7, never
     * 0-1, so writing `0.86 + 0.30 * n` gives a +/-6% wash, not the +/-20%
     * the numbers suggest. Every band here is re-centred and expanded before
     * use: `(n - 0.5) * K + 0.5` before it multiplies `c`.
     */
    let mut damp_b = ow_fbm01(
        ow_warp(p.scale(1.6).add_scalar(3.7), p_const.scale(1.6), 0.7, 3),
        p_const.scale(1.6),
        4,
        0.58,
    );
    damp_b = gl_clamp((damp_b - 0.5) * 2.6 + 0.5, 0.0, 1.0);
    c = c.scale(0.80 + 0.42 * damp_b);
    rough += (damp_b - 0.5) * 0.12;
    let mut soil2 = ow_fbm01(
        ow_warp(p.scale(3.4).add_scalar(21.0), p_const.scale(3.4), 0.55, 3),
        p_const.scale(3.4),
        4,
        0.55,
    );
    soil2 = gl_clamp((soil2 - 0.5) * 2.4 + 0.5, 0.0, 1.0);
    c = c.scale(0.875 + 0.26 * soil2);
    let mut wash = ow_fbm01(p.scale(8.0).add_scalar(6.0), p_const.scale(8.0), 4, 0.5);
    wash = gl_clamp((wash - 0.5) * 2.2 + 0.5, 0.0, 1.0);
    c = c.scale(0.925 + 0.155 * wash);
    let lap_edge = (1.0 - gl_smoothstep(0.0, 0.05, lap_t)) * (0.35 + 0.65 * lap_r);
    h += lap_edge * 0.022 - (lap_r - 0.5) * 0.014;
    c = c.scale(1.0 + lap_edge * 0.07);

    // ---- sand tooth: the 0.5-2 mm grain of the finish coat, with a
    //      matching height channel. Without this the wall is paint, not
    //      plaster. 6-9 mm float grain; the finer 1-2 mm tooth belongs to
    //      the shared detail map (see the Nyquist note in the module doc).
    let tooth = ow_worley(p.scale(20.0), p_const.scale(20.0), 1.0);
    let grain = gl_smoothstep(0.46, 0.06, tooth.f1);
    let grain_sel = 0.40 + 0.60 * gl_step(0.32, tooth.id_x);
    h += grain * grain_sel * 0.030;
    ao -= grain * 0.07;
    c = c.scale(1.0 + (grain * grain_sel - 0.20) * 0.16);
    rough += (tooth.id_x - 0.5) * 0.11 + grain * 0.05;
    // dust and shadow sit in the troughs between grains
    let trough = gl_smoothstep(0.52, 0.86, tooth.f1);
    c = v3_mix(c, c.scale(0.84), trough * 0.40);

    // pinholes from the float
    let ph = ow_worley(p.scale(22.0), p_const.scale(22.0), 1.0);
    let hole = gl_smoothstep(0.24, 0.0, ph.f1) * gl_step(0.80, ph.id_y);
    h -= hole * 0.06;
    ao -= hole * 0.4;

    // hairline crazing — a fine, wide-spread net
    let mut hair = ow_cracks(p.scale(9.0), p_const.scale(9.0), 0.9, 0.016, 0.52);
    hair += ow_cracks(p.scale(16.0).add_scalar(6.0), p_const.scale(16.0), 0.95, 0.015, 0.62) * 0.5;
    hair = gl_clamp(hair, 0.0, 1.0);
    h -= hair * 0.030;
    ao -= hair * 0.18;
    c = v3_mix(c, c.scale(0.80), hair * 0.45);

    // structural cracks — few, wide, branching
    let crack = ow_cracks(p.scale(4.5).add_scalar(17.0), p_const.scale(4.5), 0.8, 0.018, 0.62);
    h -= crack * 0.16;
    ao -= crack * 0.6;
    c = v3_mix(c, ow_srgb(Vec3::new(0.300, 0.278, 0.250)), crack * 0.8);

    // blown plaster: patches spalled off, revealing render/brick beneath
    let blow_mask = ow_fbm01(
        ow_warp(p.scale(1.05).add_scalar(9.0), p_const.scale(1.05), 1.1, 3),
        p_const.scale(1.05),
        4,
        0.55,
    );
    let blow = gl_smoothstep(0.775, 0.845, blow_mask);
    let blow_edge = gl_smoothstep(0.745, 0.790, blow_mask) - blow;
    let mut substrate = v3_mix(
        ow_srgb(Vec3::new(0.360, 0.245, 0.195)),
        ow_srgb(Vec3::new(0.430, 0.400, 0.360)),
        ow_fbm01(p.scale(9.0), p_const.scale(9.0), 4, 0.5),
    );
    substrate = substrate.scale(0.85 + 0.3 * ow_fbm01(p.scale(20.0), p_const.scale(20.0), 3, 0.5));
    c = v3_mix(c, substrate, blow * 0.85);
    h -= blow * 0.13;
    ao -= blow * 0.26;
    rough += blow * 0.10;
    // the lip of the blown patch is bright and sharp
    c = c.add_scalar(blow_edge * 0.06);
    h += blow_edge * 0.02;

    // ---- chipped patches: 6-9 cm flakes knocked off the skim, showing the
    //      darker browncoat. Deliberately fewer and larger than a fine
    //      speckle: a dense sprinkle of 3 cm dark dots reads as fly dirt,
    //      not damage.
    let ck = ow_worley(
        ow_warp(p.scale(4.2).add_scalar(13.0), p_const.scale(4.2), 0.6, 3),
        p_const.scale(4.2),
        0.95,
    );
    let ck_sel = gl_step(0.930, ck.id_y);
    let ck_size = 0.22 + 0.20 * ck.id_x;
    let ck_shape = gl_smoothstep(
        ck_size,
        ck_size * 0.3,
        ck.f1 * (0.70 + 0.60 * ow_fbm01(p.scale(16.0), p_const.scale(16.0), 3, 0.5)),
    );
    let chip = ck_sel * ck_shape;
    // The browncoat is the same family as the finish, just darker and
    // coarser — a chip is a shallow flake, not a hole punched in the wall.
    let mut coat = v3_mix(c, ow_srgb(Vec3::new(0.392, 0.336, 0.284)), 0.52);
    coat = coat.scale(0.90 + 0.20 * ow_fbm01(p.scale(18.0), p_const.scale(18.0), 3, 0.5));
    c = v3_mix(c, coat, chip * 0.58);
    h -= chip * 0.05;
    ao -= chip * 0.26;
    rough += chip * 0.09;
    let ck_lip = (ck_sel * (gl_smoothstep(ck_size * 1.25, ck_size, ck.f1) - ck_shape)).max(0.0);
    c = c.scale(1.0 + ck_lip * 0.10);
    h += ck_lip * 0.010;

    // water staining: tide marks and slow brown bleed
    let stain = ow_fbm01(
        Vec2::new(p.x * 1.6, p.y * 3.2),
        Vec2::new(p_const.x * 1.6, p_const.y * 3.0),
        5,
        0.6,
    );
    let tide = gl_smoothstep(0.60, 0.78, stain) * (1.0 - gl_smoothstep(0.78, 0.94, stain));
    c = v3_mix(c, ow_srgb(Vec3::new(0.400, 0.330, 0.245)), tide * 0.45);
    c = c.scale(1.0 - gl_smoothstep(0.50, 0.95, stain) * 0.34);
    rough += tide * 0.05;

    // black mould in the damp corners
    let mould = gl_smoothstep(0.72, 0.95, ow_fbm01(p.scale(4.0).add_scalar(25.0), p_const.scale(4.0), 5, 0.6))
        * gl_smoothstep(0.45, 0.8, stain);
    c = v3_mix(c, ow_srgb(Vec3::new(0.085, 0.090, 0.080)), mould * 0.7);
    rough += mould * 0.08;

    // grime in recesses
    let cavity = 1.0 - gl_smoothstep(0.48, 0.72, h);
    c = v3_mix(c, ow_srgb(Vec3::new(0.22, 0.21, 0.19)), cavity * 0.30);

    SurfaceSample {
        albedo: v3_clamp(c, 0.02, 0.88),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.35, 0.99),
        metal,
        ao: gl_clamp(ao, 0.15, 1.0),
    }
}

// ---------------------------------------------------------------------------
// TILE — `surfaces-arch.js:501-563`.
// ---------------------------------------------------------------------------

/// `owSurface` for `tile` (ceramic tile).
pub fn tile_surface(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    const N: f64 = 6.0;
    let p = uv.mul(p_const).add_scalar(seed * 4.4);

    let tp = uv.scale(N);
    let id = tp.floor();
    let f = tp.fract();
    let rnd = ow_hash42(id.add_scalar(seed));

    // Flat grout bed with a hard arris at the tile edge: a full-width ramp is
    // what makes a joint read as a drawn line instead of a recess.
    const J: f64 = 0.045;
    let dxj = f.x.min(1.0 - f.x);
    let dyj = f.y.min(1.0 - f.y);
    let ex = gl_smoothstep(J * 0.70, J * 1.02, dxj);
    let ey = gl_smoothstep(J * 0.70, J * 1.02, dyj);
    let face = ex.min(ey);

    let glaze = ow_fbm01(f.scale(6.0).add(Vec2::new(rnd.x, rnd.y).scale(21.0)), Vec2::splat(48.0), 4, 0.5);
    let mut c_tile = v3_mix(
        ow_srgb(Vec3::new(0.700, 0.690, 0.660)),
        ow_srgb(Vec3::new(0.470, 0.500, 0.505)),
        rnd.z * 0.7,
    );
    c_tile = c_tile.scale(0.93 + 0.13 * glaze);
    c_tile = c_tile.scale(0.92 + 0.16 * rnd.y); // per-tile batch shade

    let grout = ow_fbm01(p.scale(20.0), p_const.scale(20.0), 4, 0.5);
    let mut c_grout = ow_srgb(Vec3::new(0.400, 0.385, 0.360)).scale(0.85 + 0.3 * grout);
    c_grout = v3_mix(c_grout, ow_srgb(Vec3::new(0.13, 0.13, 0.12)), 0.45); // grout is always filthy

    let m = face;
    // 0.06 of a 0.03 m relief = 1.8 mm of grout recess.
    let mut h = gl_mix(0.76 - (grout - 0.5) * 0.02, 0.82 + (rnd.w - 0.5) * 0.04, m);
    let mut c = v3_mix(c_grout, c_tile, m);
    // glazed tile has to stay glossy enough to actually catch a highlight
    let mut rough = gl_mix(0.92, 0.20 + 0.22 * glaze + (rnd.z - 0.5) * 0.14, m);
    let mut ao = gl_mix(0.40, 1.0, gl_smoothstep(0.0, 0.8, face));
    let metal = 0.0;

    // chipped / cracked / missing tiles
    let broken = gl_step(0.90, rnd.x);
    let crack = ow_cracks(f.scale(3.0).add(Vec2::new(rnd.y, rnd.z).scale(9.0)), Vec2::splat(24.0), 0.85, 0.04, 0.45) * m;
    c = v3_mix(c, c.scale(0.3), crack * 0.8);
    h -= crack * 0.08;
    ao -= crack * 0.5;
    let sub = ow_srgb(Vec3::new(0.330, 0.300, 0.270));
    c = v3_mix(c, sub, broken * m * 0.9);
    h -= broken * m * 0.14;
    rough = gl_mix(rough, 0.95, broken * m);

    // scuffs and traffic wear
    let wear = gl_smoothstep(0.45, 0.95, ow_fbm01(p.scale(2.0), p_const.scale(2.0), 4, 0.55));
    rough += wear * 0.20 * m;
    c = c.scale(1.0 - wear * 0.12);

    let cavity = 1.0 - gl_smoothstep(0.68, 0.80, h);
    c = v3_mix(c, ow_srgb(Vec3::new(0.14, 0.13, 0.12)), cavity * 0.35);

    SurfaceSample {
        albedo: v3_clamp(c, 0.02, 0.85),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.12, 0.95),
        metal,
        ao: gl_clamp(ao, 0.15, 1.0),
    }
}
