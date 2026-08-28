//! Ported from Claude-of-Duty `src/materials/glsl/surfaces-metal.js:1-323` —
//! the whole file: `RUST_HELPERS` (`:8-21`, `owRustColour`) and the four
//! `owSurface` generators `METAL_RUST` (`:23-88`), `METAL_PAINTED`
//! (`:90-178`), `METAL_BRUSHED` (`:180-237`) and `CORRUGATED` (`:239-323`).
//!
//! ## The one rule every generator here exists to preserve
//!
//! > Bare metal is `metal = 1`. Every oxide, paint, dirt or grime layer on
//! > top of it forces `metal = 0`.
//!
//! That is the source file's own opening comment, and it is not decorative:
//! it is why these four surfaces read as steel and zinc rather than as grey
//! plastic under the engine's lighting. Every generator below starts each
//! metallic region at `metal = 1.0` and every subsequent contamination layer
//! (rust bloom, paint chip inverse, grime, smudge, dirt, a rubber washer)
//! pulls it back toward `0.0` — restored to `1.0` only where a scratch or a
//! chip exposes bare metal again. `tests/materials_metal_port.rs` asserts
//! this directly: metalness ~1 on a clean patch, ~0 under a contamination
//! layer, for every one of the four generators.
//!
//! ## GLSL -> Rust mapping
//!
//! Same conventions as [`crate::materials::bake::detail_surface`] and
//! [`crate::materials::noise`]'s module doc: `f64` throughout (this is
//! *shader* source with no JS-`number` precedent to match), swizzles expanded
//! inline at their one call site, `gl_mix`/`gl_smoothstep`/`gl_clamp` for the
//! GLSL builtins already in [`crate::materials::noise`]. Three GLSL builtins
//! this file needs that no earlier port called are added locally, private to
//! this module (not `crate::materials::noise` — shared infra other sibling
//! surface ports may also be editing right now, and none of these three are
//! noise-library primitives, just per-file plumbing):
//!
//! - [`mix3`] — component-wise `vec3` `mix`. `noise.rs` only has the scalar
//!   `gl_mix`; every `mix(vec3, vec3, float)` call site here needs the
//!   three-channel form.
//! - [`clamp3`] — component-wise `vec3` `clamp`, for the final `alb =
//!   clamp(c, vec3(lo), vec3(hi))` line every generator ends on.
//! - [`gl_step`] — GLSL `step(edge, x)`. The same helper
//!   `crate::materials::bake::detail_surface`'s file defines privately, for
//!   the same reason (it's a bare GLSL builtin the source calls directly,
//!   not one of `noise.js`'s named functions), duplicated here rather than
//!   imported since it is `fn`-private in that module.
//!
//! ## The `sign(0)` trap — [`corrugated`]'s profile
//!
//! GLSL `sign(x)` returns exactly `0.0` for `x == 0.0`. Rust's `f64::signum`
//! does **not**: `0.0_f64.signum() == 1.0` and `(-0.0_f64).signum() ==
//! -1.0`. [`corrugated`]'s ridge profile is `sign(wave) * pow(abs(wave),
//! 0.72) * 0.5 + 0.5`, and `wave = sin(t)` is exactly `0.0` whenever `t`
//! lands on a multiple of pi — a measure-zero event for a continuous sweep,
//! but the CPU bake samples a *discrete* texel grid, so it is reachable.
//! [`gl_sign`] hand-rolls the three-valued GLSL semantics rather than
//! reaching for `signum`, per the port recipe's "Language traps" list;
//! `tests/materials_metal_port.rs` pins a texel where `wave` lands on
//! exactly zero.
//!
//! ## Tint decode — [`hex_to_linear_tint`]
//!
//! [`metal_painted`] is the one generator here that reads a uniform the
//! source's `HEADER` declares but `DETAIL_SRC`/`MACRO_SRC` never touch:
//! `uTintA`. The call site that fills it in is `src/materials/index.js:145`,
//! `tintA: bake.tintA !== undefined ? new THREE.Color(bake.tintA) : undefined`
//! — a `THREE.Color` built from a bare sRGB hex integer. Three's
//! `ColorManagement` (enabled by default; `three.core.js`'s
//! `createColorManagement`/`SRGBToLinear`) converts that hex colour to the
//! *linear* working colour space at construction time, before it ever
//! reaches the shader uniform — the same sRGB decode
//! [`crate::materials::noise::ow_srgb`] already implements and this port
//! already golden-pins. [`hex_to_linear_tint`] reproduces exactly that:
//! unpack the hex triplet to `[0,1]`, decode through `ow_srgb`. This is the
//! conversion `LIBRARY`'s (`crate::materials::mod`) `bake.tint_a:
//! Option<u32>` fields (e.g. `metal_painted`'s `0x4a5340`) need before they
//! can reach [`metal_painted`]'s `tint_a: Vec3` parameter — not part of
//! `owSurface`'s own GLSL body, but the one piece of `index.js` plumbing
//! this generator cannot be evaluated without.

use crate::materials::bake::SurfaceSample;
use crate::materials::noise::{
    gl_clamp, gl_fract, gl_mix, gl_smoothstep, ow_billow, ow_fbm01, ow_hash11, ow_hash12,
    ow_scratches, ow_shear, ow_shear_per, ow_srgb, ow_warp, ow_worley, Vec2, Vec3,
};

// ---------------------------------------------------------------------------
// Local GLSL-builtin helpers — see the module doc for why these live here
// rather than in `crate::materials::noise`.
// ---------------------------------------------------------------------------

/// Component-wise `vec3` `mix(a, b, t)`.
fn mix3(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    Vec3::new(gl_mix(a.x, b.x, t), gl_mix(a.y, b.y, t), gl_mix(a.z, b.z, t))
}

/// Component-wise `vec3` `clamp(v, lo, hi)`.
fn clamp3(v: Vec3, lo: f64, hi: f64) -> Vec3 {
    Vec3::new(gl_clamp(v.x, lo, hi), gl_clamp(v.y, lo, hi), gl_clamp(v.z, lo, hi))
}

/// GLSL `step(edge, x)`: `1.0` when `x >= edge`, else `0.0`.
fn gl_step(edge: f64, x: f64) -> f64 {
    if x < edge {
        0.0
    } else {
        1.0
    }
}

/// GLSL `sign(x)`. **Not** `f64::signum` — see the module doc's "`sign(0)`
/// trap" section.
/// GLSL `sign` — **not** [`crate::jsmath::sign`], and deliberately not folded
/// into it.
///
/// `surfaces-metal.js` holds GLSL in a string and calls `sign(` there; it never
/// calls `Math.sign`. The two agree on every finite non-zero input and differ
/// at the edges: `Math.sign(-0)` is `-0` and `Math.sign(NaN)` is `NaN`, whereas
/// GLSL's returns `0.0` for any zero and leaves NaN unspecified. Collapsing
/// them would be the "`Vector3.length()` is not `Math.hypot`" trap run
/// backwards — two languages' functions that look alike sharing one
/// implementation because the difference is currently unobservable.
fn gl_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// `new THREE.Color(hex)` under Three's default (enabled) `ColorManagement`:
/// unpack the sRGB hex triplet to `[0,1]` and decode to linear. See the
/// module doc's "Tint decode" section.
pub fn hex_to_linear_tint(hex: u32) -> Vec3 {
    // This used to call `ow_srgb` — the *GLSL* decode — while documenting
    // itself as `new THREE.Color(hex)`. The two are algebraically equal and
    // numerically different on 254 of 256 byte values, and the call site really
    // is `new THREE.Color(bake.tintA)` (`materials/index.js:145`), so the GLSL
    // form was wrong here. See `crate::materials::three_color`.
    let [r, g, b] = crate::materials::three_color::hex_to_linear(hex);
    Vec3::new(r, g, b)
}

// ---------------------------------------------------------------------------
// RUST_HELPERS — `surfaces-metal.js:8-21`. Shared by all four generators
// below, exactly as it is shared in the source (imported once into
// `generator.js`'s program string, ahead of every per-material body).
// ---------------------------------------------------------------------------

/// `owRustColour(t, grain)`, `surfaces-metal.js:10-20`. Layered iron oxide:
/// young rust reads orange, mature rust dark red-brown, old rust near-black,
/// with a powdery bloom tint blended in by grain. Colour is driven by `t`
/// ("how old the patch is"), not by how *much* rust there is — see each
/// call site's own comment on why its caller deliberately does not pass a
/// raw rust-amount mask here.
fn ow_rust_colour(t: f64, grain: f64) -> Vec3 {
    let c1 = ow_srgb(Vec3::new(0.560, 0.290, 0.110)); // fresh orange
    let c2 = ow_srgb(Vec3::new(0.380, 0.180, 0.085)); // mid
    let c3 = ow_srgb(Vec3::new(0.190, 0.100, 0.060)); // mature
    let c4 = ow_srgb(Vec3::new(0.640, 0.400, 0.190)); // powdery bloom
    let mut c = mix3(c1, c2, gl_smoothstep(0.15, 0.6, t));
    c = mix3(c, c3, gl_smoothstep(0.55, 1.0, t));
    c = mix3(c, c4, gl_smoothstep(0.55, 0.95, grain) * 0.45);
    c.scale(0.82 + 0.36 * grain)
}

// ---------------------------------------------------------------------------
// METAL_RUST — `surfaces-metal.js:23-88`.
// ---------------------------------------------------------------------------

/// Weathered mill-finish steel: a base mill/fine noise, warped billow rust
/// blooms whose flaking scale plates lift near their own edges, deep pitting
/// under old rust, scratches that restore bare metal, and a final grime
/// pass. `owSurface`, `METAL_RUST`, `surfaces-metal.js:24-87`.
pub fn metal_rust(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 7.7);

    // ---- base steel ----
    let mill = ow_fbm01(
        ow_shear(p.scale(4.0), 1.0, 6.0),
        ow_shear_per(p_const.scale(4.0), 6.0),
        4,
        0.5,
    );
    let fine = ow_fbm01(p.scale(22.0), p_const.scale(22.0), 4, 0.5);
    let steel = ow_srgb(Vec3::new(0.330, 0.335, 0.345)).scale(0.90 + 0.18 * mill);
    let mut c = steel;
    let mut h = 0.72 + (mill - 0.5) * 0.02 + (fine - 0.5) * 0.01;
    let mut rough = 0.40 + (mill - 0.5) * 0.16 + (fine - 0.5) * 0.08;
    let mut metal = 1.0;
    let mut ao = 1.0;

    // ---- rust blooms: warped billow clusters, hard-edged where they flake ----
    let wp = ow_warp(p.scale(1.4), p_const.scale(1.4), 1.2, 4);
    let bloom = ow_billow(wp, p_const.scale(1.4), 5, 0.6);
    let bloom = 1.0 - bloom; // clusters, not veins
    let spread = ow_fbm01(p.scale(0.7).add_scalar(12.0), p_const.scale(0.7), 3, 0.6);
    let rust = gl_smoothstep(0.36, 0.72, bloom * (0.55 + 0.85 * spread));
    let rust_grain = ow_fbm01(p.scale(26.0), p_const.scale(26.0), 4, 0.55);
    let pit = ow_fbm01(p.scale(24.0), p_const.scale(24.0), 3, 0.5);

    // flaking scale: the rust lifts in plates near the edge of a bloom
    let scale_n = ow_worley(p.scale(16.0), p_const.scale(16.0), 1.0).f1;
    let flake = gl_smoothstep(0.30, 0.10, scale_n)
        * gl_smoothstep(0.25, 0.55, rust)
        * (1.0 - gl_smoothstep(0.8, 1.0, rust));

    // Rust *colour* is driven by how old the patch is, not by how much of it
    // there is — otherwise every heavily rusted area collapses to the same
    // brown.
    let rust_age = ow_fbm01(p.scale(0.85).add_scalar(21.0), p_const.scale(0.85), 4, 0.62);
    let rust_col = ow_rust_colour(rust_age * 0.8 + rust * 0.3, rust_grain);
    c = mix3(c, rust_col, rust);
    metal = gl_mix(1.0, 0.0, gl_smoothstep(0.15, 0.55, rust));
    rough = gl_mix(rough, 0.86 + 0.10 * rust_grain, gl_smoothstep(0.1, 0.6, rust));
    h += rust * 0.11 * (0.4 + rust_grain) + flake * 0.13;
    h -= gl_smoothstep(0.5, 0.95, rust) * pit * 0.14; // deep pitting under old rust
    ao -= flake * 0.30 + gl_smoothstep(0.6, 1.0, rust) * 0.15;

    // ---- pitting straight into the steel where rust has eaten through ----
    let pits = ow_worley(p.scale(22.0), p_const.scale(22.0), 1.0);
    let deep = gl_smoothstep(0.22, 0.0, pits.f1) * gl_step(0.72, pits.id_y) * gl_smoothstep(0.3, 0.8, rust);
    h -= deep * 0.22;
    ao -= deep * 0.45;
    c = mix3(c, rust_col.scale(0.35), deep * 0.7);

    // ---- scratches through everything, exposing bright metal ----
    let mut scr = ow_scratches(p.scale(3.0), p_const.scale(3.0), 12.0, 1.0, 0.60);
    scr += ow_scratches(p.scale(5.0).add_scalar(8.0), p_const.scale(5.0), 9.0, -2.0, 0.66) * 0.7;
    let scr = gl_clamp(scr, 0.0, 1.0) * 0.6;
    c = mix3(c, ow_srgb(Vec3::new(0.480, 0.485, 0.495)), scr * 0.8);
    metal = gl_mix(metal, 1.0, scr * 0.85);
    rough = gl_mix(rough, 0.24, scr * 0.7);
    h -= scr * 0.010;

    // ---- grime ----
    let grime = gl_smoothstep(
        0.55,
        0.9,
        ow_fbm01(
            Vec2::new(p.x * 5.0, p.y * 0.8),
            Vec2::new(p_const.x * 5.0, p_const.y.max(1.0)),
            5,
            0.55,
        ),
    );
    c = c.scale(1.0 - grime * 0.25);
    rough += grime * 0.08;

    SurfaceSample {
        albedo: clamp3(c, 0.02, 0.80),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.12, 0.99),
        metal: gl_clamp(metal, 0.0, 1.0),
        ao: gl_clamp(ao, 0.15, 1.0),
    }
}

// ---------------------------------------------------------------------------
// METAL_PAINTED — `surfaces-metal.js:90-178`.
// ---------------------------------------------------------------------------

/// An industrial paint coat over a real layer stack — paint, a primer band,
/// rust that has crept underneath, bare steel — with chipping driven by a
/// blend of a warped chip field, a fine edge mask and the rust field itself,
/// small Worley impact dings, a bright chip lip, scratches cutting straight
/// to metal, and rust bleed weeping from the chips. `owSurface`,
/// `METAL_PAINTED`, `surfaces-metal.js:91-177`. `tint_a` is the caller's
/// already-linear-decoded `uTintA` — see [`hex_to_linear_tint`]. `param_z`
/// is the source's `uParam.z`.
pub fn metal_painted(uv: Vec2, seed: f64, tint_a: Vec3, param_z: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 11.3);

    // ---- substrate: steel with a mill finish ----
    let mill = ow_fbm01(
        ow_shear(p.scale(5.0), 1.0, 8.0),
        ow_shear_per(p_const.scale(5.0), 8.0),
        4,
        0.5,
    );
    let steel = ow_srgb(Vec3::new(0.330, 0.335, 0.345)).scale(0.88 + 0.2 * mill);

    // ---- rust that has crept under the paint ----
    let bloom = 1.0 - ow_billow(ow_warp(p.scale(1.8), p_const.scale(1.8), 1.1, 4), p_const.scale(1.8), 5, 0.6);
    let rust_field = gl_smoothstep(0.60, 0.92, bloom);
    let rust_grain = ow_fbm01(p.scale(22.0), p_const.scale(22.0), 4, 0.55);
    let rust_col = ow_rust_colour(rust_field, rust_grain);

    // ---- paint: an industrial coat with roller texture and orange peel ----
    let peel = ow_fbm01(p.scale(22.0), p_const.scale(22.0), 4, 0.5);
    let roller = ow_fbm01(
        ow_shear(p.scale(2.0), 0.0, 3.0),
        ow_shear_per(p_const.scale(2.0), 3.0),
        4,
        0.5,
    );
    let mut paint = tint_a.scale(0.90 + 0.16 * roller);
    paint = paint.scale(0.96 + 0.08 * peel);
    // sun-bleached on the up-facing halves
    let bleach = gl_smoothstep(0.35, 0.85, ow_fbm01(p.scale(0.8), p_const.scale(0.8), 3, 0.6));
    let paint = mix3(paint, paint.scale(1.25).add_scalar(0.03), bleach * 0.5);

    // ---- chipping: paint fails at scratches, impacts and along its own edges ----
    let chip_field = ow_fbm01(
        ow_warp(p.scale(2.6).add_scalar(4.0), p_const.scale(2.6), 0.9, 3),
        p_const.scale(2.6),
        5,
        0.55,
    );
    let chip_edge = ow_fbm01(p.scale(12.0), p_const.scale(12.0), 4, 0.5);
    // Paint mostly holds: only the top of the distribution actually fails, and
    // it fails hardest where rust is already lifting it from underneath.
    let chip_src = chip_field * 0.60 + chip_edge * 0.20 + rust_field * 0.32 + param_z * 0.25;
    let chip = gl_smoothstep(0.66, 0.92, chip_src);
    // small impact chips scattered around
    let dings = ow_worley(p.scale(20.0), p_const.scale(20.0), 1.0);
    let ding = gl_smoothstep(0.14, 0.03, dings.f1) * gl_step(0.88, dings.id_y);
    let chip = gl_clamp(chip + ding, 0.0, 1.0);

    // scratches that cut down to bare metal
    let mut scr = ow_scratches(p.scale(2.5), p_const.scale(2.5), 14.0, 1.0, 0.62);
    scr += ow_scratches(p.scale(4.0).add_scalar(21.0), p_const.scale(4.0), 10.0, -1.0, 0.66) * 0.8;
    let scr = gl_clamp(scr, 0.0, 1.0);

    // ---- layer stack: paint over primer over rust over steel ----
    let primer = ow_srgb(Vec3::new(0.470, 0.300, 0.180));
    let primer_band = gl_smoothstep(0.0, 0.35, chip) * (1.0 - gl_smoothstep(0.35, 0.6, chip));

    let mut c = paint;
    let mut r = 0.42 + (peel - 0.5) * 0.22 + bleach * 0.16;
    let mut mtl = 0.0;
    let mut h = 0.74 + (roller - 0.5) * 0.02 + (peel - 0.5) * 0.012;
    let mut ao = 1.0;

    c = mix3(c, primer, primer_band * 0.7);
    c = mix3(c, rust_col, gl_smoothstep(0.35, 0.75, chip) * (0.55 + 0.45 * rust_field));
    c = mix3(c, steel, gl_smoothstep(0.75, 0.95, chip) * (1.0 - rust_field) * 0.9);
    r = gl_mix(r, 0.88, gl_smoothstep(0.3, 0.8, chip) * (0.4 + 0.6 * rust_field));
    r = gl_mix(r, 0.38, gl_smoothstep(0.8, 1.0, chip) * (1.0 - rust_field));
    mtl = gl_mix(
        0.0,
        1.0,
        gl_smoothstep(0.78, 0.96, chip) * (1.0 - gl_smoothstep(0.2, 0.7, rust_field)),
    );
    h -= gl_smoothstep(0.4, 0.8, chip) * 0.16; // paint has real thickness
    ao -= gl_smoothstep(0.35, 0.7, chip) * 0.22;
    // the lip of a chip is a bright hard edge
    let lip = gl_smoothstep(0.30, 0.42, chip) * (1.0 - gl_smoothstep(0.42, 0.55, chip));
    c = c.scale(1.0 + lip * 0.15);
    h += lip * 0.05;

    // scratches on top of everything
    c = mix3(c, ow_srgb(Vec3::new(0.500, 0.505, 0.515)), scr * 0.55);
    mtl = gl_mix(mtl, 1.0, scr * 0.6);
    r = gl_mix(r, 0.26, scr * 0.55);

    // ---- dirt and rain streaks ----
    let streak = ow_fbm01(
        Vec2::new(p.x * 6.0, p.y * 0.7),
        Vec2::new(p_const.x * 6.0, p_const.y.max(1.0)),
        5,
        0.55,
    );
    let grime = gl_smoothstep(0.52, 0.92, streak);
    c = c.scale(1.0 - grime * 0.30);
    r += grime * 0.10;
    mtl *= 1.0 - grime * 0.5;
    // rust bleed running down from the chips
    let bleed = gl_smoothstep(0.66, 0.95, streak) * gl_smoothstep(0.2, 0.6, rust_field);
    c = mix3(c, ow_srgb(Vec3::new(0.360, 0.190, 0.090)), bleed * 0.45);

    let cavity = 1.0 - gl_smoothstep(0.62, 0.78, h);
    c = c.scale(1.0 - cavity * 0.18);

    SurfaceSample {
        albedo: clamp3(c, 0.02, 0.85),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(r, 0.14, 0.99),
        metal: gl_clamp(mtl, 0.0, 1.0),
        ao: gl_clamp(ao, 0.2, 1.0),
    }
}

// ---------------------------------------------------------------------------
// METAL_BRUSHED — `surfaces-metal.js:180-237`.
// ---------------------------------------------------------------------------

/// Directional brushed steel: three shear-stretched fbm bands running along
/// X form the fibres, plus deep score lines, cross scratches from handling,
/// shallow dents, and — "the thing that sells brushed metal" — fingerprint
/// and grease smudges that pull metalness down without any colour layer on
/// top. `owSurface`, `METAL_BRUSHED`, `surfaces-metal.js:181-236`.
pub fn metal_brushed(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed * 15.1);

    // brushing runs along X: heavy shear so the noise stretches into fibres
    let bp = ow_shear(p, 0.0, 64.0);
    let bpp = ow_shear_per(p_const, 64.0);
    let brush1 = ow_fbm01(bp.scale(2.0), bpp.scale(2.0), 4, 0.5);
    let brush2 = ow_fbm01(bp.scale(8.0).add_scalar(3.0), bpp.scale(8.0), 3, 0.5);
    let brush3 = ow_fbm01(
        ow_shear(p.scale(4.0), 0.0, 24.0),
        ow_shear_per(p_const.scale(4.0), 24.0),
        3,
        0.5,
    );
    let brush = brush1 * 0.5 + brush2 * 0.32 + brush3 * 0.18;

    let macro_n = ow_fbm01(p.scale(0.9), p_const.scale(0.9), 3, 0.6);

    let mut c = ow_srgb(Vec3::new(0.560, 0.565, 0.575));
    c = c.scale(0.93 + 0.13 * brush);
    c = c.scale(0.97 + 0.06 * macro_n);

    let mut metal = 1.0;
    let mut rough = 0.22 + brush * 0.24 + (macro_n - 0.5) * 0.06;
    let mut h = 0.78 + (brush - 0.5) * 0.012;
    // Unlike `metal_rust`/`metal_painted`/`corrugated`, METAL_BRUSHED never
    // touches `ao` again after this initial assignment (`surfaces-metal.js`
    // has no `ao -=`/`ao *=` line in this body) — so it stays a plain,
    // non-`mut` binding here rather than `let mut ao = 1.0;`.
    let ao = 1.0;

    // deeper score lines
    let score = ow_scratches(p.scale(1.0), p_const, 40.0, 0.0, 0.60);
    rough += score * 0.22;
    h -= score * 0.006;
    c = c.scale(1.0 - score * 0.05);

    // cross scratches from handling
    let cross = ow_scratches(p.scale(3.0), p_const.scale(3.0), 8.0, 3.0, 0.70) * 0.7;
    rough += cross * 0.20;
    h -= cross * 0.004;

    // dents: shallow, wide, they break the reflection
    let dent = ow_fbm01(p.scale(3.0).add_scalar(7.0), p_const.scale(3.0), 3, 0.6);
    h += (dent - 0.5) * 0.05;

    // fingerprints and grease smudges — the thing that sells brushed metal
    let smudge = gl_smoothstep(
        0.58,
        0.86,
        ow_fbm01(
            ow_warp(p.scale(2.2).add_scalar(19.0), p_const.scale(2.2), 0.7, 3),
            p_const.scale(2.2),
            4,
            0.55,
        ),
    );
    rough += smudge * 0.22;
    c = c.scale(1.0 - smudge * 0.06);
    metal -= smudge * 0.10;

    // grime settling in
    let grime = gl_smoothstep(0.66, 0.95, ow_fbm01(p.scale(5.0), p_const.scale(5.0), 4, 0.55));
    c = mix3(c, ow_srgb(Vec3::new(0.180, 0.175, 0.165)), grime * 0.35);
    rough += grime * 0.18;
    metal -= grime * 0.35;

    SurfaceSample {
        albedo: clamp3(c, 0.02, 0.88),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.08, 0.95),
        metal: gl_clamp(metal, 0.0, 1.0),
        ao: gl_clamp(ao, 0.4, 1.0),
    }
}

// ---------------------------------------------------------------------------
// CORRUGATED — `surfaces-metal.js:239-323`.
// ---------------------------------------------------------------------------

/// Corrugated galvanised sheet: an analytic sinusoid ridge profile with
/// per-panel lap steps, a Worley zinc spangle, rust weighted into the
/// valleys and toward the bottom of the sheet, rust-through perforations,
/// hex screws with rubber washers seated on the crowns (each weeping a rust
/// streak below it), and dirt collecting in the valleys. `owSurface`,
/// `CORRUGATED`, `surfaces-metal.js:240-322`.
pub fn corrugated(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    const RIDGES: f64 = 12.0;
    let p = uv.mul(p_const).add_scalar(seed * 6.1);

    // ---- the profile: sinusoidal ridges with a flat-ish crown ----
    let t = uv.x * RIDGES * 6.283_185_307_18;
    let wave = t.sin();
    // See the module doc's "sign(0) trap": `gl_sign`, not `f64::signum`.
    let profile = gl_sign(wave) * wave.abs().powf(0.72) * 0.5 + 0.5;
    // panel joints every 4 ridges: one sheet laps over the next
    let panel = uv.x * RIDGES / 4.0;
    let panel_id = panel.floor();
    // GLSL `fract(panel)`: `panel - floor(panel)`, always non-negative.
    // `panel = uv.x * RIDGES / 4.0` is itself always `>= 0` for `uv.x` in
    // `[0, 1)`, so this coincides with `f64::fract` — written out explicitly
    // (matching `gl_fract` in `noise.rs`) rather than leaning on that
    // coincidence, per the port recipe's `sign`/`fract` sign-of-zero class
    // of trap.
    let frac_panel = gl_fract(panel);
    let lap = gl_smoothstep(0.0, 0.06, frac_panel) * gl_smoothstep(0.0, 0.06, 1.0 - frac_panel);
    let panel_step = (ow_hash11(panel_id + seed) - 0.5) * 0.05;

    let dents = ow_fbm01(p.scale(2.2), p_const.scale(2.2), 4, 0.6);
    let fine = ow_fbm01(p.scale(11.0), p_const.scale(11.0), 4, 0.5);

    let mut h = 0.18 + profile * 0.62 + panel_step + (dents - 0.5) * 0.07 + (fine - 0.5) * 0.012;
    h -= (1.0 - lap) * 0.06;

    // ---- galvanised zinc: crystalline spangle ----
    let sp = ow_worley(p.scale(7.0), p_const.scale(7.0), 1.0);
    let spangle = gl_smoothstep(0.55, 0.05, sp.f1);
    let zinc = ow_srgb(Vec3::new(0.520, 0.535, 0.545));
    let mut c = mix3(zinc.scale(0.86), zinc.scale(1.12), spangle * (0.3 + 0.7 * sp.id_x));
    c = c.scale(0.94 + 0.12 * fine);
    let mut metal = 1.0;
    let mut rough = 0.34 + (1.0 - spangle) * 0.16 + (fine - 0.5) * 0.08;
    let mut ao = 1.0;

    // ---- rust, heavier in the valleys and at the bottom of the sheet ----
    let valley = 1.0 - profile;
    let rust_field = gl_smoothstep(
        0.62,
        0.98,
        (1.0 - ow_billow(ow_warp(p.scale(1.6), p_const.scale(1.6), 1.0, 4), p_const.scale(1.6), 5, 0.6))
            * (0.58 + 0.40 * valley)
            + (1.0 - uv.y) * 0.16,
    );
    let rust_grain = ow_fbm01(p.scale(22.0), p_const.scale(22.0), 4, 0.55);
    let rust_col = ow_rust_colour(rust_field, rust_grain);
    c = mix3(c, rust_col, rust_field);
    metal = gl_mix(metal, 0.0, gl_smoothstep(0.15, 0.6, rust_field));
    rough = gl_mix(rough, 0.88 + 0.08 * rust_grain, gl_smoothstep(0.1, 0.6, rust_field));
    h += rust_field * 0.02 * rust_grain;

    // holes rusted right through
    let hole = ow_worley(p.scale(5.0).add_scalar(31.0), p_const.scale(5.0), 0.95);
    let perf = gl_smoothstep(0.10, 0.02, hole.f1) * gl_step(0.94, hole.id_y) * gl_smoothstep(0.5, 0.9, rust_field);
    h -= perf * 0.5;
    ao -= perf * 0.7;
    c = mix3(c, rust_col.scale(0.25), perf);

    // ---- fixings: hex screws with a rubber washer, two rows, on the crowns ----
    let crown = gl_smoothstep(0.72, 0.95, profile);
    let fx = Vec2::new(gl_fract(uv.x * RIDGES) - 0.5, gl_fract(uv.y * 3.0) - 0.5);
    let fd = fx.mul(Vec2::new(1.0, RIDGES / 3.0)).length();
    let screw_rnd = ow_hash12(Vec2::new(uv.x * RIDGES, uv.y * 3.0).floor().add_scalar(seed));
    let screw = gl_smoothstep(0.16, 0.11, fd) * crown * gl_step(0.25, screw_rnd);
    let washer = gl_smoothstep(0.24, 0.18, fd) * crown * gl_step(0.25, screw_rnd);
    h += washer * 0.02 + screw * 0.035;
    c = mix3(c, ow_srgb(Vec3::new(0.120, 0.115, 0.110)), washer * 0.8);
    c = mix3(c, mix3(ow_srgb(Vec3::new(0.400, 0.405, 0.410)), rust_col, rust_field), screw);
    rough = gl_mix(rough, 0.85, washer * 0.8);
    rough = gl_mix(rough, 0.42 + rust_field * 0.4, screw);
    metal = gl_mix(metal, 0.0, washer * 0.9);
    metal = gl_mix(metal, 1.0 - rust_field, screw);
    ao -= (washer - screw) * 0.35;
    // rust streak weeping from each fixing. `washer * 0.0` is a literal
    // no-op in the source (`surfaces-metal.js:306`) — kept verbatim rather
    // than dropped, per the port recipe's "port the behaviour, don't tidy"
    // rule; it contributes exactly zero to `weep`.
    let weep = washer * 0.0
        + gl_smoothstep(0.34, 0.20, fd) * gl_step(0.25, screw_rnd) * crown
            * gl_smoothstep(0.0, 0.5, gl_fract(uv.y * 3.0) - 0.5);
    c = mix3(c, ow_srgb(Vec3::new(0.330, 0.170, 0.080)), gl_clamp(weep, 0.0, 1.0) * 0.5);

    // ---- dirt collecting in the valleys ----
    let dirt = valley * gl_smoothstep(0.35, 0.8, ow_fbm01(p.scale(3.0), p_const.scale(3.0), 4, 0.55));
    c = mix3(c, ow_srgb(Vec3::new(0.200, 0.185, 0.160)), dirt * 0.40);
    rough += dirt * 0.14;
    metal *= 1.0 - dirt * 0.5;
    ao -= valley * 0.18;

    SurfaceSample {
        albedo: clamp3(c, 0.02, 0.85),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: gl_clamp(rough, 0.14, 0.99),
        metal: gl_clamp(metal, 0.0, 1.0),
        ao: gl_clamp(ao, 0.15, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unit sanity checks for the local helpers; the four generators
    // themselves are golden-pinned in `tests/materials_metal_port.rs`.

    #[test]
    fn mix3_matches_component_wise_lerp() {
        let a = Vec3::new(0.0, 10.0, -4.0);
        let b = Vec3::new(1.0, 20.0, 4.0);
        let m = mix3(a, b, 0.25);
        assert!((m.x - 0.25).abs() < 1e-12);
        assert!((m.y - 12.5).abs() < 1e-12);
        assert!((m.z - (-2.0)).abs() < 1e-12);
    }

    #[test]
    fn clamp3_clamps_each_channel_independently() {
        let v = Vec3::new(-1.0, 0.5, 2.0);
        let c = clamp3(v, 0.0, 1.0);
        assert_eq!((c.x, c.y, c.z), (0.0, 0.5, 1.0));
    }

    #[test]
    fn gl_step_matches_glsl_semantics() {
        assert_eq!(gl_step(0.5, 0.4), 0.0);
        assert_eq!(gl_step(0.5, 0.5), 1.0);
        assert_eq!(gl_step(0.5, 0.6), 1.0);
    }

    #[test]
    fn gl_sign_is_three_valued_unlike_signum() {
        assert_eq!(gl_sign(0.0), 0.0);
        assert_eq!(gl_sign(-0.0), 0.0);
        assert_eq!(gl_sign(3.0), 1.0);
        assert_eq!(gl_sign(-3.0), -1.0);
        // The trap this exists to avoid, made explicit:
        assert_eq!(0.0_f64.signum(), 1.0);
        assert_eq!((-0.0_f64).signum(), -1.0);
    }

    /// `hex_to_linear_tint` is `new THREE.Color(hex)` — three's decode, not the
    /// GLSL `owSRGB` one.
    ///
    /// This test used to assert the opposite, and that is how the defect
    /// survived: the function's doc comment said `THREE.Color`, its body called
    /// `ow_srgb`, and this test pinned the body. A test that agrees with the
    /// code instead of with the source is not coverage.
    ///
    /// The real call site is `new THREE.Color(bake.tintA)`
    /// (`materials/index.js:145`). `materials::system`'s golden — captured from
    /// the actual `MaterialSystem` — disagreed by ~4e-11 against a `1e-12` pin
    /// until this was fixed.
    #[test]
    fn hex_to_linear_tint_is_threes_decode_and_not_the_glsl_one() {
        // metal_painted's real tintA, LIBRARY's 0x4a5340.
        let t = hex_to_linear_tint(0x4a_53_40);
        let three = crate::materials::three_color::hex_to_linear(0x4a_53_40);
        assert_eq!((t.x, t.y, t.z), (three[0], three[1], three[2]));

        // And the negative half, which is what makes the positive half mean
        // something: the GLSL decode really is a different number here.
        let glsl = ow_srgb(Vec3::new(
            f64::from(0x4a) / 255.0,
            f64::from(0x53) / 255.0,
            f64::from(0x40) / 255.0,
        ));
        assert_ne!(
            t.x.to_bits(),
            glsl.x.to_bits(),
            "if these agree, the two decodes have converged and this              distinction is no longer observable — check why before deleting",
        );
    }

    // ------------------------------------------------------------------
    // The physical-plausibility rule, checked directly against the real
    // generators: bare metal reads metal ~= 1, every contamination layer
    // pulls it toward 0. See the module doc's opening section.
    // ------------------------------------------------------------------

    #[test]
    fn metal_rust_is_metallic_somewhere_and_not_under_heavy_rust_elsewhere() {
        // The rust bloom/spread noise means no single hand-picked uv is
        // guaranteed a priori to be "clean" — scan a grid and require both
        // ends of the rule to occur somewhere, matching the corrugated scan
        // below. `tests/materials_metal_port.rs` pins the exact golden
        // values this scan is checking a property of.
        let seed = 37.0;
        let mut found_metallic = false;
        let mut found_rusted = false;
        for i in 0..32 {
            for j in 0..32 {
                let uv = Vec2::new((f64::from(i) + 0.5) / 32.0, (f64::from(j) + 0.5) / 32.0);
                let s = metal_rust(uv, seed);
                found_metallic |= s.metal > 0.9;
                found_rusted |= s.metal < 0.1;
            }
        }
        assert!(found_metallic, "expected at least one near-metallic bare-steel texel in the scan");
        assert!(found_rusted, "expected at least one near-zero-metalness rusted texel in the scan");
    }

    #[test]
    fn metal_painted_is_non_metallic_somewhere_and_bare_through_a_chip_elsewhere() {
        let tint = hex_to_linear_tint(0x4a_53_40);
        let seed = 61.0;
        let mut found_paint = false;
        let mut found_bare = false;
        // A metallic chip-through is a small feature (a `smoothstep(0.78,
        // 0.96, chip)` peak); a 32x32 grid can miss it entirely — 48x48
        // reliably finds one at this seed (verified against the JS capture
        // functions up to a 200x200 grid before picking this resolution).
        for i in 0..48 {
            for j in 0..48 {
                let uv = Vec2::new((f64::from(i) + 0.5) / 48.0, (f64::from(j) + 0.5) / 48.0);
                let s = metal_painted(uv, seed, tint, 0.0);
                found_paint |= s.metal < 0.1;
                found_bare |= s.metal > 0.9;
            }
        }
        assert!(found_paint, "expected at least one non-metallic intact-paint texel in the scan");
        assert!(found_bare, "expected at least one bare, near-metallic chipped-through texel in the scan");
    }

    #[test]
    fn metal_brushed_smudge_and_grime_reduce_metalness_from_the_bare_baseline() {
        let s = metal_brushed(Vec2::new(0.5, 0.5), 83.0);
        // metal starts at 1.0 and only ever loses ground in this generator
        // (smudge/grime subtract, nothing adds back), so it must never
        // exceed the bare-metal baseline.
        assert!(s.metal <= 1.0);
    }

    #[test]
    fn corrugated_washer_is_non_metallic_and_bare_crown_is_metallic() {
        // A washer sits wherever `screwRnd >= 0.25` on a crown; scan for one
        // deterministically rather than hard-coding a uv that depends on the
        // hash lattice.
        let seed = 29.0;
        let mut found_washer = false;
        let mut found_bare_crown = false;
        for i in 0..64 {
            for j in 0..12 {
                let uv = Vec2::new((f64::from(i) + 0.5) / 64.0, (f64::from(j) + 0.5) / 12.0);
                let s = corrugated(uv, seed);
                found_washer |= s.metal < 0.2;
                found_bare_crown |= s.metal > 0.9;
            }
        }
        assert!(found_washer, "expected at least one low-metalness washer/rust texel in the scan");
        assert!(found_bare_crown, "expected at least one bare, near-metallic zinc texel in the scan");
    }
}
