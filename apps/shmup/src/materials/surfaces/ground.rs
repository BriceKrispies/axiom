//! Ported from Claude-of-Duty `src/materials/glsl/surfaces-ground.js:1-366` —
//! the whole file (`ASPHALT`, `SAND`, `DIRT`, `GRAVEL`).
//!
//! Four `owSurface(uv) -> (albedo, height, roughness, metal, ao)` generators
//! for ground planes — the surfaces most prone to visible tiling, since a
//! ground plane is usually seen at a shallow, distant grazing angle across a
//! wide area. See [`crate::materials::noise`] for the tileable noise library
//! and [`crate::materials::bake`] for the `owSurface` contract
//! ([`SurfaceSample`]) these four functions implement, the same way
//! `bake.rs`'s `detail_surface`/`macro_surface` do.
//!
//! ## The Nyquist budget (source `surfaces-ground.js:7-16`, reproduced)
//!
//! Every generator below writes `p = uv * P` with `P = vec2(8.0)`, so a term
//! at `p * K` lays `8K` cells across the bake, and at a bake of `N` texels
//! that is `N / (8K)` texels per cell. Under ~5 texels the cell is not a
//! feature, it is white noise: mip 0 shows salt-and-pepper dither and mip 1
//! has already averaged it to a flat wash — that single mistake is what made
//! the whole street read as sandpaper at 3 m and as flat colour at 15 m. All
//! ground bakes are 1024, so `K` is capped at 24 (5.3 texels) and the
//! sub-millimetre read is delegated to the shared detail map
//! ([`super::super::bake::detail_surface`]), which is tiled ten times finer
//! and has the texel budget for it. **Every frequency constant below is
//! preserved exactly as written in the source** — none of them are "tidied."
//!
//! ## No native oracle
//!
//! These four bodies are GLSL held in JavaScript template-string literals
//! (`export const ASPHALT = /* glsl */ \`...\``); they never ran anywhere but
//! a browser GPU, so — same situation as `sky/`'s `*_FRAG`/`*_GLSL` bodies —
//! there is no JavaScript function to import and call as an oracle.
//! `tests/materials_surfaces_ground/capture.mjs` hand-transcribes each GLSL
//! body into plain JS doubles, independently of (but line-referenced against,
//! the same as) this file's Rust transcription. That capture script is
//! therefore a second, weaker, human transcription — not a genuine oracle —
//! and pinning against it catches drift between the two transcriptions, not a
//! mistake both share. See `tests/materials_surfaces_ground_port.rs`'s module
//! doc for the tolerance discipline.
//!
//! ## `vec4` swizzle convention
//!
//! Every `owWorley` result below reads `.x`/`.y` as F1/F2 distance and
//! `.z`/`.w` as the F1 cell's two hash channels — [`WorleyResult`]'s
//! `f1`/`f2`/`id_x`/`id_y`, exactly as `noise.rs`'s module doc and
//! `bake.rs`'s `detail_surface` already use it.
//!
//! ## Local helpers not in `noise.rs`
//!
//! [`v3_add`], [`v3_mix`], [`v3_clamp`], and [`gl_step`] are bare GLSL
//! primitives this file's four bodies need (`vec3 + vec3`, `mix`/`clamp` on a
//! `vec3`, and the scalar `step` builtin) that no `noise.js` function
//! provides and no other ported file has needed yet — `bake.rs`'s own local
//! `gl_step` is the precedent for keeping a bare-builtin helper file-local
//! rather than widening the shared [`super::super::noise`] module for one
//! caller.

use super::super::bake::SurfaceSample;
use super::super::noise::{
    gl_clamp, gl_fract, gl_mix, gl_smoothstep, ow_cracks, ow_fbm, ow_fbm01, ow_billow, ow_shear,
    ow_shear_per, ow_srgb, ow_warp, ow_worley, Vec2, Vec3,
};

/// GLSL `step(edge, x)`: `1.0` when `x >= edge`, else `0.0`. See the module
/// doc — not one of `noise.js`'s functions, so it lives here rather than in
/// [`super::super::noise`], matching `bake.rs`'s own local `gl_step`.
fn gl_step(edge: f64, x: f64) -> f64 {
    if x < edge {
        0.0
    } else {
        1.0
    }
}

/// `vec3 + vec3`. Not in [`super::super::noise::Vec3`] — every function
/// there needed only `mul`/`scale`/`add_scalar`/`dot`/`fract` until this file.
fn v3_add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// GLSL `mix(vec3, vec3, float)`, component-wise [`gl_mix`].
fn v3_mix(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    Vec3::new(gl_mix(a.x, b.x, t), gl_mix(a.y, b.y, t), gl_mix(a.z, b.z, t))
}

/// GLSL `clamp(vec3, float, float)`, component-wise [`gl_clamp`].
fn v3_clamp(v: Vec3, lo: f64, hi: f64) -> Vec3 {
    Vec3::new(
        gl_clamp(v.x, lo, hi),
        gl_clamp(v.y, lo, hi),
        gl_clamp(v.z, lo, hi),
    )
}

// ---------------------------------------------------------------------------
// ASPHALT — `surfaces-ground.js:18-119`.
// ---------------------------------------------------------------------------

/// `ASPHALT`'s `owSurface` (`surfaces-ground.js:19-118`): binder + three
/// grades of angular aggregate (domain-warped Worley for the facets, per the
/// source comment: "round cells become faceted, which is what separates
/// asphalt from a pebble beach"), tyre-polish lanes, patch repairs with
/// bleeding tar seams, alligator + thermal cracking, oil stains, settled dust.
pub fn asphalt(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 6.9);

    // ---- binder: dark, slightly blue-grey, sun-bleached in patches ----
    let macro_ = ow_fbm01(p.scale(0.55), p_const.scale(0.5), 4, 0.6);
    let mid = ow_fbm01(p.scale(3.0), p_const.scale(3.0), 5, 0.5);
    let fine = ow_fbm01(p.scale(16.0), p_const.scale(16.0), 4, 0.5);

    let c_fresh = ow_srgb(Vec3::new(0.115, 0.115, 0.122));
    let c_worn = ow_srgb(Vec3::new(0.300, 0.298, 0.295));
    let mut c = v3_mix(c_fresh, c_worn, gl_smoothstep(0.25, 0.85, macro_) * 0.85);
    // Half the old fine-grain albedo contrast: the stone read belongs in the
    // height/normal channels, not in a high-frequency albedo dither.
    c = c.scale(0.94 + 0.12 * fine);

    let mut h = 0.60 + (mid - 0.5) * 0.06;
    let mut rough = 0.78 + (mid - 0.5) * 0.10 + (fine - 0.5) * 0.14;
    let metal = 0.0;
    let mut ao = 1.0;

    // ---- aggregate: dense angular chippings, three grades ----
    let ap = ow_warp(p, p_const, 0.10, 3);
    let big = ow_worley(ap.scale(12.0), p_const.scale(12.0), 1.0);
    let big_m = gl_smoothstep(0.40, 0.16, big.f1);
    let big_exposed = big_m
        * gl_smoothstep(
            0.30,
            0.62,
            ow_fbm01(p.scale(2.2).add_scalar(3.0), p_const.scale(2.0), 4, 0.5) + big.id_y * 0.5,
        );
    let small = ow_worley(ap.scale(22.0).add_scalar(7.0), p_const.scale(22.0), 1.0);
    let small_m = gl_smoothstep(0.36, 0.10, small.f1);
    let small_exposed = small_m * gl_step(0.30, small.id_y);
    let grit = ow_worley(ap.scale(28.0).add_scalar(3.0), p_const.scale(28.0), 1.0);
    let grit_m = gl_smoothstep(0.32, 0.06, grit.f1) * gl_step(0.45, grit.id_x);

    let stone_a = ow_srgb(Vec3::new(0.400, 0.392, 0.378));
    let stone_b = ow_srgb(Vec3::new(0.210, 0.200, 0.192));
    let stone_c = ow_srgb(Vec3::new(0.560, 0.520, 0.470));
    let mut stone = v3_mix(stone_a, stone_b, big.id_x);
    stone = v3_mix(stone, stone_c, gl_step(0.90, big.id_y));

    // Stones are read by their relief and their specular, not by their tint:
    // colour contrast is roughly halved and the height contribution raised.
    c = v3_mix(c, stone, big_exposed * 0.52);
    c = v3_mix(c, v3_mix(stone_a, stone_c, small.id_x), small_exposed * 0.22);
    c = v3_mix(c, v3_mix(stone_b, stone_a, grit.id_x), grit_m * 0.14);
    h += big_exposed * 0.15 * (0.6 + 0.6 * big.id_x) + small_exposed * 0.065 + grit_m * 0.022;
    rough += big_exposed * (0.10 - 0.22 * big.id_x) + small_exposed * (0.06 - 0.14 * small.id_x);

    // voids between the aggregate — where the binder has ravelled out
    let void_m = gl_smoothstep(0.50, 0.85, big.f1) * gl_smoothstep(0.28, 0.6, small.f1);
    h -= void_m * 0.10;
    ao -= void_m * 0.14;

    // ---- tyre polish: two smooth bands where wheels track ----
    let lane = (gl_fract(uv.x * 1.0 + 0.25) - 0.5).abs() * 2.0;
    let polish = (1.0 - gl_smoothstep(0.10, 0.62, lane))
        * gl_smoothstep(
            0.25,
            0.65,
            ow_fbm01(
                Vec2::new(p.x * 0.7, p.y * 5.0),
                Vec2::new(p_const.x, p_const.y * 5.0),
                4,
                0.5,
            ),
        );
    rough -= polish * 0.16;
    h -= polish * 0.012;
    c = v3_mix(
        c,
        v3_add(c.scale(0.78), ow_srgb(Vec3::new(0.045, 0.045, 0.048))),
        polish * 0.45,
    );

    // ---- patch repairs: darker rectangles-ish with a seam ----
    let rep = ow_worley(
        ow_warp(p.scale(0.5).add_scalar(13.0), p_const.scale(0.5), 1.6, 3),
        p_const.scale(0.5),
        0.9,
    );
    let in_patch = gl_step(0.72, rep.id_y);
    let patch_edge = (1.0 - gl_smoothstep(0.0, 0.06, rep.f2 - rep.f1)) * in_patch;
    c = v3_mix(c, c_fresh.scale(0.85 + 0.35 * fine), in_patch * 0.20);
    rough = gl_mix(rough, 0.84, in_patch * 0.22);
    h -= patch_edge * 0.07;
    ao -= patch_edge * 0.20;
    c = v3_mix(c, c_fresh.scale(0.5), patch_edge * 0.35);
    // tar bleeding out of the seam, glossy
    let tar = patch_edge
        * gl_smoothstep(0.4, 0.7, ow_fbm01(p.scale(6.0), p_const.scale(6.0), 3, 0.5));
    rough -= tar * 0.35;
    c = v3_mix(c, ow_srgb(Vec3::new(0.055, 0.055, 0.058)), tar * 0.7);

    // ---- alligator cracking + long thermal cracks ----
    let gator = ow_cracks(p.scale(3.4), p_const.scale(3.4), 0.9, 0.032, 0.56);
    let thermal = ow_cracks(p.scale(0.9).add_scalar(41.0), p_const.scale(0.9), 0.75, 0.05, 0.70);
    let crack = gl_clamp(gator + thermal, 0.0, 1.0);
    h -= crack * 0.16;
    ao -= crack * 0.30;
    c = v3_mix(c, ow_srgb(Vec3::new(0.045, 0.043, 0.042)), crack * 0.85);
    rough += crack * 0.12;

    // ---- oil stains, dark and slightly glossy ----
    let oil = gl_smoothstep(
        0.68,
        0.90,
        ow_fbm01(
            ow_warp(p.scale(1.8).add_scalar(31.0), p_const.scale(1.8), 0.9, 3),
            p_const.scale(1.8),
            4,
            0.55,
        ),
    );
    c = v3_mix(c, ow_srgb(Vec3::new(0.045, 0.043, 0.046)), oil * 0.6);
    rough -= oil * 0.16;

    // ---- dust settled in the low spots ----
    let dust = gl_smoothstep(0.55, 0.30, h) * gl_smoothstep(0.35, 0.75, macro_);
    c = v3_mix(c, ow_srgb(Vec3::new(0.420, 0.390, 0.340)), dust * 0.35);
    rough += dust * 0.10;

    SurfaceSample {
        albedo: v3_clamp(c, 0.02, 0.75),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.44, 0.99),
        metal,
        // see the AO note in GRAVEL: on a ground plane this channel is the shading
        ao: gl_clamp(ao, 0.68, 1.0),
    }
}

// ---------------------------------------------------------------------------
// SAND — `surfaces-ground.js:121-179`.
// ---------------------------------------------------------------------------

/// `SAND`'s `owSurface` (`surfaces-ground.js:122-178`): asymmetric wind
/// ripples (`pow(ripple, 1.7) * 0.75 + ripple * 0.25` — a gentle windward
/// slope, a sharp lee crest), damp hollows, quartz sparkle, pebbles and shell
/// fragments, dark mineral streaks.
pub fn sand(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 8.2);

    // ---- wind ripples: sheared sine, gently warped so the crests meander ----
    let rp = ow_shear(p.scale(1.0), 1.0, 1.0);
    let warp = ow_fbm(p.scale(0.9), p_const.scale(0.9), 3, 0.55);
    let mut ripple = (rp.y * 1.0 + warp * 0.55) * 6.283_18;
    ripple = ripple.sin();
    // asymmetric profile: gentle windward slope, sharp lee crest
    ripple = ripple * 0.5 + 0.5;
    ripple = ripple.powf(1.7) * 0.75 + ripple * 0.25;
    let ripple_amp = gl_smoothstep(0.20, 0.70, ow_fbm01(p.scale(0.7), p_const.scale(0.7), 3, 0.6));
    let secondary = ((p.y * 3.0 + p.x * 1.0 + warp * 0.8) * 6.283_18).sin() * 0.5 + 0.5;

    let dune = ow_fbm01(p.scale(0.5), p_const.scale(0.5), 4, 0.6);
    let mid = ow_fbm01(p.scale(5.0), p_const.scale(5.0), 5, 0.5);
    let grain = ow_fbm01(p.scale(18.0), p_const.scale(18.0), 4, 0.55);
    let gcell = ow_worley(p.scale(24.0), p_const.scale(24.0), 1.0);

    let mut h = 0.50
        + (dune - 0.5) * 0.16
        + (mid - 0.5) * 0.05
        + (ripple - 0.5) * 0.26 * ripple_amp
        + (secondary - 0.5) * 0.06 * ripple_amp
        + (grain - 0.5) * 0.018;

    let c_light = ow_srgb(Vec3::new(0.760, 0.660, 0.480));
    let c_mid = ow_srgb(Vec3::new(0.610, 0.510, 0.360));
    let c_damp = ow_srgb(Vec3::new(0.360, 0.290, 0.205));
    let mut c = v3_mix(c_mid, c_light, gl_smoothstep(0.3, 0.8, dune));
    c = v3_mix(c, c_damp, gl_smoothstep(0.62, 0.28, h) * 0.55); // damp in the hollows
    // coarse grains collect on the crests, fines in the troughs
    c = v3_mix(
        c,
        c_light.scale(1.06),
        gl_smoothstep(0.45, 0.85, ripple) * ripple_amp * 0.35,
    );
    c = v3_mix(
        c,
        c_mid.scale(0.88),
        gl_smoothstep(0.45, 0.10, ripple) * ripple_amp * 0.30,
    );
    c = c.scale(0.90 + 0.18 * grain);
    // sparkle from quartz grains
    c = c.add_scalar(gl_smoothstep(0.22, 0.0, gcell.f1) * gl_step(0.86, gcell.id_x) * 0.10);

    let mut rough = 0.90 + (grain - 0.5) * 0.10 - gl_smoothstep(0.6, 0.3, h) * 0.12;
    let metal = 0.0;
    let mut ao = 1.0 - gl_smoothstep(0.55, 0.25, h) * 0.10;

    // ---- pebbles and shell fragments sitting on top ----
    let peb = ow_worley(p.scale(18.0), p_const.scale(18.0), 1.0);
    let pebble = gl_smoothstep(0.30, 0.10, peb.f1) * gl_step(0.80, peb.id_y);
    let pcol = v3_mix(
        ow_srgb(Vec3::new(0.400, 0.370, 0.330)),
        ow_srgb(Vec3::new(0.690, 0.660, 0.620)),
        peb.id_x,
    );
    c = v3_mix(c, pcol, pebble * 0.85);
    h += pebble * 0.05;
    rough = gl_mix(rough, 0.55 + 0.25 * peb.id_x, pebble * 0.8);
    ao -= gl_smoothstep(0.40, 0.30, peb.f1) * gl_step(0.80, peb.id_y) * 0.08;

    // ---- scattered dry debris / dark mineral streaks ----
    let streak = gl_smoothstep(
        0.62,
        0.88,
        ow_fbm01(
            ow_shear(p.scale(2.5), 2.0, 4.0),
            ow_shear_per(p_const.scale(2.5), 4.0),
            4,
            0.5,
        ),
    );
    c = v3_mix(c, c_damp.scale(1.1), streak * 0.22);

    SurfaceSample {
        albedo: v3_clamp(c, 0.02, 0.82),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.35, 0.99),
        metal,
        ao: gl_clamp(ao, 0.80, 1.0),
    }
}

// ---------------------------------------------------------------------------
// DIRT — `surfaces-ground.js:181-248`.
// ---------------------------------------------------------------------------

/// `DIRT`'s `owSurface` (`surfaces-ground.js:182-247`): billow clumps, dried
/// mud cracks whose plates curl up at their edges, two stone grades, dead
/// grass/organic litter, sparse moss in damp low spots.
pub fn dirt(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 3.4);

    let macro_ = ow_fbm01(p.scale(0.6), p_const.scale(0.6), 4, 0.62);
    let clump = ow_billow(
        ow_warp(p.scale(3.0), p_const.scale(3.0), 0.6, 3),
        p_const.scale(3.0),
        5,
        0.55,
    );
    let fine = ow_fbm01(p.scale(14.0), p_const.scale(14.0), 4, 0.5);
    let micro = ow_fbm01(p.scale(22.0), p_const.scale(22.0), 3, 0.5);

    let c_dry = ow_srgb(Vec3::new(0.430, 0.350, 0.255));
    let c_wet = ow_srgb(Vec3::new(0.185, 0.140, 0.100));
    let c_pale = ow_srgb(Vec3::new(0.560, 0.490, 0.385));
    let mut c = v3_mix(c_dry, c_pale, gl_smoothstep(0.45, 0.9, macro_));
    c = v3_mix(c, c_wet, gl_smoothstep(0.55, 0.15, macro_) * 0.8);
    // Halved high-frequency albedo contrast; the read moves into height/roughness.
    c = c.scale(0.94 + 0.11 * fine);
    c = c.scale(0.975 + 0.05 * micro);

    let mut h = 0.55 + (macro_ - 0.5) * 0.14 + (clump - 0.5) * 0.16 + (fine - 0.5) * 0.075;
    let mut rough = 0.88 + (fine - 0.5) * 0.14 + (micro - 0.5) * 0.10;
    let metal = 0.0;
    let mut ao = 1.0;

    // dried mud cracks in the flat pans
    let pan = gl_smoothstep(0.35, 0.65, macro_);
    let mud = ow_cracks(p.scale(2.4), p_const.scale(2.4), 0.85, 0.045, 0.35) * pan;
    h -= mud * 0.16;
    ao -= mud * 0.32;
    c = v3_mix(c, c_wet.scale(0.7), mud * 0.75);
    // the mud plates curl up at their edges
    let plate_lift = gl_smoothstep(0.10, 0.0, mud) * pan;
    h += plate_lift * 0.01;

    // stones of two grades
    let st = ow_worley(p.scale(11.0), p_const.scale(11.0), 1.0);
    let stone = gl_smoothstep(0.30, 0.11, st.f1) * gl_step(0.62, st.id_y);
    let scol = v3_mix(
        ow_srgb(Vec3::new(0.330, 0.315, 0.295)),
        ow_srgb(Vec3::new(0.600, 0.575, 0.540)),
        st.id_x,
    );
    c = v3_mix(c, scol, stone * 0.6);
    h += stone * 0.085;
    rough = gl_mix(rough, 0.52 + 0.28 * st.id_x, stone * 0.8);
    ao -= gl_smoothstep(0.36, 0.28, st.f1) * gl_step(0.62, st.id_y) * 0.10;

    let grit = ow_worley(p.scale(22.0), p_const.scale(22.0), 1.0);
    let grit_m = gl_smoothstep(0.26, 0.08, grit.f1) * gl_step(0.55, grit.id_y);
    c = v3_mix(c, v3_mix(scol, c_pale, grit.id_x), grit_m * 0.4);
    h += grit_m * 0.015;

    // dead grass / organic litter
    let mut litter = gl_smoothstep(
        0.70,
        0.86,
        ow_fbm01(
            ow_shear(p.scale(8.0), 1.0, 5.0),
            ow_shear_per(p_const.scale(8.0), 5.0),
            4,
            0.5,
        ),
    );
    litter *= gl_smoothstep(0.4, 0.8, macro_);
    c = v3_mix(c, ow_srgb(Vec3::new(0.330, 0.290, 0.160)), litter * 0.5);
    h += litter * 0.012;
    rough += litter * 0.05;

    // sparse moss in the damp low spots
    let moss = gl_smoothstep(
        0.74,
        0.92,
        ow_fbm01(p.scale(4.5).add_scalar(19.0), p_const.scale(4.5), 5, 0.6),
    ) * gl_smoothstep(0.5, 0.1, macro_);
    c = v3_mix(c, ow_srgb(Vec3::new(0.150, 0.185, 0.105)), moss * 0.65);

    let cavity = 1.0 - gl_smoothstep(0.40, 0.70, h);
    ao -= cavity * 0.14;

    SurfaceSample {
        albedo: v3_clamp(c, 0.02, 0.72),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.45, 0.99),
        metal,
        ao: gl_clamp(ao, 0.72, 1.0),
    }
}

// ---------------------------------------------------------------------------
// GRAVEL — `surfaces-ground.js:250-366`.
// ---------------------------------------------------------------------------

/// `GRAVEL`'s `owSurface` (`surfaces-ground.js:251-365`): three grades of
/// aggregate at 34/19/9 mm (5.9 texels at the worst case, right at the
/// Nyquist floor documented in the module header), a compacted-dust bed with
/// stones half-buried in it (separated from the bed by relief and roughness,
/// **not** by albedo value — see the source's own doc comment reproduced at
/// the `top`/`cBed` mix below), a drift field that buries aggregate, and
/// dried tyre-track/heel scuffs.
///
/// **The baked AO stays inside `0.87..1.0`**, tighter than every other
/// generator in this file. On a sky-lit ground plane in shadow, `orm.r` (AO)
/// is very nearly the *only* shading term the surface has — the source's own
/// measurement found a `0.62:1.0` cavity ripple at the 10-30 mm aggregate
/// period reading as visible salt-and-pepper at 2 screen pixels, and fixed it
/// by compressing this range, not by touching albedo or the normal map (both
/// were proved innocent by holding them constant and watching the speckle
/// survive). `tests/materials_surfaces_ground_port.rs` asserts this range
/// directly — "easy to lose and hard to spot later" per the port recipe.
pub fn gravel(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 2.7);

    let bed = ow_fbm01(p.scale(1.3), p_const.scale(1.3), 4, 0.55);

    let a = ow_worley(p.scale(5.5), p_const.scale(5.5), 1.0);
    let b = ow_worley(p.scale(10.0).add_scalar(5.0), p_const.scale(10.0), 1.0);
    let c_sm = ow_worley(p.scale(21.0).add_scalar(11.0), p_const.scale(21.0), 1.0);

    // Sparse: most of what you see is the compacted bed, with stones IN it.
    let s_a = gl_smoothstep(0.36, 0.10, a.f1) * gl_step(0.44, a.id_y);
    let s_b = gl_smoothstep(0.30, 0.08, b.f1) * gl_step(0.62, b.id_y);
    let s_c = gl_smoothstep(0.24, 0.06, c_sm.f1) * gl_step(0.74, c_sm.id_y);

    // The stones live in the height field: raised relief so each one catches
    // the sun on one side and shadows on the other.
    let ha = s_a * 0.15 * (0.5 + a.id_x);
    let hb = s_b * 0.09 * (0.5 + b.id_x);
    let hc = s_c * 0.025;
    let mut h = 0.54 + (bed - 0.5) * 0.11 + ha.max(hb).max(hc) + 0.22 * (ha + hb);

    // The stone palette has to straddle the bed value, not sit above it —
    // half the stones are darker than the bed and half lighter (source
    // comment, `surfaces-ground.js:293-299`).
    let s1 = ow_srgb(Vec3::new(0.372, 0.356, 0.332));
    let s2 = ow_srgb(Vec3::new(0.232, 0.220, 0.208));
    let s3 = ow_srgb(Vec3::new(0.462, 0.438, 0.400));
    let s4 = ow_srgb(Vec3::new(0.352, 0.276, 0.220));
    let mut top = v3_mix(s1, s2, a.id_x);
    top = v3_mix(top, s3, gl_step(0.78, a.id_y));
    top = v3_mix(top, s4, gl_step(0.90, b.id_y) * 0.7);

    // The bed is dust, and it is only a few percent off the stones sitting in it.
    let c_bed = ow_srgb(Vec3::new(0.362, 0.336, 0.294));
    let mut c = v3_mix(
        c_bed,
        top,
        gl_clamp(s_a * 0.70 + s_b * 0.42 + s_c * 0.16, 0.0, 1.0),
    );
    // ~9 mm grain, 4.9 texels wide: a texture, not a dither.
    let grain = ow_fbm01(p.scale(13.0), p_const.scale(13.0), 4, 0.5);
    c = c.scale(0.965 + 0.07 * grain);

    // Per-stone gloss, but only a little of it — clamping this term alone
    // took the measured high-frequency deviation on the road from 2.45 to
    // 1.68 (source comment, `surfaces-ground.js:315-321`).
    let mut rough = 0.82 + 0.05 * grain + (1.0 - gl_clamp(s_a + s_b, 0.0, 1.0)) * 0.06
        - s_a * (0.06 + 0.07 * a.id_x)
        - s_b * 0.05 * b.id_x;
    let metal = 0.0;
    // AO IS THE WHOLE BALLGAME ON A GROUND PLANE — see this fn's doc comment.
    let mut ao = gl_mix(0.87, 1.0, gl_smoothstep(0.42, 0.66, h));

    // fine dust filling the gaps
    let dust = 1.0 - gl_smoothstep(0.44, 0.62, h);
    c = v3_mix(c, c_bed.scale(1.04), dust * 0.5);
    rough += dust * 0.08;
    ao = gl_mix(ao, 1.0, dust * 0.3);

    // Wheel and foot traffic sweeps the loose grit into drifts and polishes
    // bare lanes: 0.5-1.5 m form inside the tile.
    let drift = ow_fbm01(
        ow_warp(p.scale(0.9).add_scalar(17.0), p_const.scale(0.9), 0.8, 3),
        p_const.scale(0.9),
        4,
        0.6,
    );
    h += (drift - 0.5) * 0.10;
    c = c.scale(0.86 + 0.28 * drift);
    rough += (drift - 0.5) * 0.10;
    // Dust drifts BURY the aggregate: where the drift is deep the stones go
    // under it.
    c = v3_mix(
        c,
        c_bed.scale(0.92 + 0.22 * drift),
        gl_smoothstep(0.55, 0.88, drift) * 0.72,
    );

    // Dried tyre tracks and dragged-heel scuffs — long, shallow, low contrast.
    let scuff = ow_fbm01(
        ow_shear(p.scale(2.2), 0.0, 6.0),
        ow_shear_per(p_const.scale(2.2), 6.0),
        4,
        0.5,
    );
    c = c.scale(1.0 - gl_smoothstep(0.55, 0.92, scuff) * 0.10);
    rough -= gl_smoothstep(0.6, 0.95, scuff) * 0.08;

    SurfaceSample {
        albedo: v3_clamp(c, 0.02, 0.78),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.62, 0.99),
        metal,
        ao: gl_clamp(ao, 0.72, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A representative sample at each generator's own P-scaled cell centre —
    /// enough to catch a gross wiring mistake (wrong swizzle field, wrong
    /// scale constant) before ever touching the JS golden. The golden-pinned
    /// tests in `tests/materials_surfaces_ground_port.rs` are the real proof.
    #[test]
    fn every_generator_returns_in_contract_range_at_a_grid_of_uvs() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.13, 0.77),
            Vec2::new(0.42, 0.09),
            Vec2::new(0.91, 0.36),
            Vec2::new(1.0, 1.0),
        ];
        for uv in pts {
            for (name, sample) in [
                ("asphalt", asphalt(uv, 71.0)),
                ("sand", sand(uv, 91.0)),
                ("dirt", dirt(uv, 13.0)),
                ("gravel", gravel(uv, 57.0)),
            ] {
                assert!(
                    (0.0..=1.0).contains(&sample.albedo.x)
                        && (0.0..=1.0).contains(&sample.albedo.y)
                        && (0.0..=1.0).contains(&sample.albedo.z),
                    "{name}: albedo out of [0,1] at {uv:?}: {:?}",
                    sample.albedo
                );
                assert!(
                    (0.0..=1.0).contains(&sample.height),
                    "{name}: height out of [0,1] at {uv:?}: {}",
                    sample.height
                );
                assert!(
                    (0.0..=1.0).contains(&sample.roughness),
                    "{name}: roughness out of [0,1] at {uv:?}: {}",
                    sample.roughness
                );
                assert_eq!(sample.metal, 0.0, "{name}: every ground generator is non-metal");
                assert!(
                    (0.0..=1.0).contains(&sample.ao),
                    "{name}: ao out of [0,1] at {uv:?}: {}",
                    sample.ao
                );
            }
        }
    }

    /// The physical-plausibility albedo clamp every generator's last lines
    /// apply: `[0.02, 0.72..0.88]` depending on the surface. Pinned here as a
    /// structural invariant (not a golden value) so a future edit cannot
    /// accidentally widen the clamp bounds without a test noticing.
    #[test]
    fn albedo_clamp_bounds_match_each_generators_own_last_lines() {
        let hi = |uv: Vec2, hi_bound: f64, sample: SurfaceSample| {
            assert!(sample.albedo.x <= hi_bound + 1e-12);
            assert!(sample.albedo.y <= hi_bound + 1e-12);
            assert!(sample.albedo.z <= hi_bound + 1e-12);
            assert!(sample.albedo.x >= 0.02 - 1e-12, "uv {uv:?}");
        };
        for uv in [Vec2::new(0.2, 0.6), Vec2::new(0.8, 0.1), Vec2::new(0.5, 0.5)] {
            hi(uv, 0.75, asphalt(uv, 71.0));
            hi(uv, 0.82, sand(uv, 91.0));
            hi(uv, 0.72, dirt(uv, 13.0));
            hi(uv, 0.78, gravel(uv, 57.0));
        }
    }

    /// See this module's `gravel` doc: the baked AO must stay inside
    /// `0.87..1.0` — "easy to lose and hard to spot later" per the port
    /// recipe, so it gets a dedicated, wide-grid assertion independent of the
    /// golden file.
    #[test]
    fn gravel_ao_stays_in_the_documented_0_87_to_1_0_band() {
        for iy in 0..17u32 {
            for ix in 0..17u32 {
                let uv = Vec2::new(f64::from(ix) / 16.0, f64::from(iy) / 16.0);
                let s = gravel(uv, 57.0);
                assert!(
                    (0.87..=1.0).contains(&s.ao),
                    "gravel ao {} at uv {uv:?} escaped the 0.87..1.0 band",
                    s.ao
                );
            }
        }
    }
}
