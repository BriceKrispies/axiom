//! WGSL transcription of Claude-of-Duty `src/materials/glsl/noise.js` — the
//! tileable procedural noise library every surface generator is built from.
//!
//! The source's own header, verbatim:
//!
//! > Tileable procedural noise library (GLSL, shared by every surface
//! > generator).
//! >
//! > Everything here is *periodic*: each function takes a `per` (period, in
//! > lattice cells) and wraps its hash lattice with mod(), so a texture
//! > generated over uv in \[0,1) with p = uv * per tiles seamlessly. Octaves
//! > double both the frequency and the period, which keeps the whole fbm stack
//! > seamless.
//! >
//! > Hashes are sin-free (Dave Hoskins style) — sin() based hashes band badly
//! > on Apple GPUs at high lattice coordinates.
//!
//! This is the **twin** of [`crate::materials::noise`], which is the same
//! library on the CPU in `f64`. Neither is derived from the other: both are
//! transcribed from `noise.js`, so a disagreement between them is a real
//! finding rather than a shared misreading. That is the whole point of keeping
//! two — the port has already measured what happens when one transcription
//! checks another written by the same hand (ten defects in `sky/`).
//!
//! ## The GLSL-semantics helpers
//!
//! `mix`, `clamp`, `step`, `smoothstep`, `mod` and `sign` all exist in WGSL,
//! and WGSL is permitted to factor them differently from GLSL. They are
//! therefore written out here to their exact GLSL definitions, as `ow*`
//! helpers — the precedent `surface_program::emit` sets, and the shape the CPU
//! twin already has (`gl_mix`, `gl_clamp`, `gl_smoothstep`, `gl_mod`,
//! `gl_fract`). Two of them are not interchangeable with their WGSL
//! namesakes at all:
//!
//! * **`owMod`** is `x - y * floor(x / y)`, not a truncated remainder. Lattice
//!   coordinates go negative at the tile's wrapped edge, where `%` and `mod`
//!   disagree in sign.
//! * **`owSign`** returns `0.0` for zero, which is GLSL's rule and the one
//!   `CORRUGATED`'s ridge crossings rely on.
//!
//! `abs`, `min`, `max`, `floor`, `fract`, `sqrt`, `sin`, `cos`, `pow`, `exp`,
//! `length`, `dot` and `normalize` are exact in both languages and are used as
//! builtins.

/// The GLSL-semantics shims, emitted ahead of the library because the library
/// itself calls them.
///
/// Only the widths the ported generators actually use are declared. A generator
/// that needs another (`owMix4`, `owClamp2`, …) adds it here, next to its
/// siblings, rather than reaching for the WGSL builtin.
pub const GL_SEMANTICS: &str = r#"
// GLSL `mod(x, y)` = x - y * floor(x / y). NOT a truncated remainder: the
// lattice wrap feeds this negative coordinates.
fn owMod(x : f32, y : f32) -> f32 {
  return x - y * floor(x / y);
}
fn owMod2(p : vec2<f32>, q : vec2<f32>) -> vec2<f32> {
  return p - q * floor(p / q);
}

// GLSL `mix(a, b, t)` = a * (1 - t) + b * t.
fn owMix(a : f32, b : f32, t : f32) -> f32 {
  return a * (1.0 - t) + b * t;
}
fn owMix3(a : vec3<f32>, b : vec3<f32>, t : f32) -> vec3<f32> {
  return a * (1.0 - t) + b * t;
}
fn owMix3v(a : vec3<f32>, b : vec3<f32>, t : vec3<f32>) -> vec3<f32> {
  return a * (vec3<f32>(1.0) - t) + b * t;
}

// GLSL `clamp(x, lo, hi)` = min(max(x, lo), hi).
fn owClamp(x : f32, lo : f32, hi : f32) -> f32 {
  return min(max(x, lo), hi);
}
fn owClamp3(x : vec3<f32>, lo : vec3<f32>, hi : vec3<f32>) -> vec3<f32> {
  return min(max(x, lo), hi);
}

// GLSL `step(edge, x)` = 0 when x < edge, else 1.
fn owStep(edge : f32, x : f32) -> f32 {
  return select(1.0, 0.0, x < edge);
}
fn owStep3(edge : vec3<f32>, x : vec3<f32>) -> vec3<f32> {
  return select(vec3<f32>(1.0), vec3<f32>(0.0), x < edge);
}

// GLSL `smoothstep(e0, e1, x)`: t = clamp((x - e0) / (e1 - e0), 0, 1);
// return t * t * (3 - 2 * t).
fn owSmoothstep(e0 : f32, e1 : f32, x : f32) -> f32 {
  let t = owClamp((x - e0) / (e1 - e0), 0.0, 1.0);
  return t * t * (3.0 - 2.0 * t);
}

// GLSL `sign(x)`: -1 below zero, 0 AT zero, +1 above.
fn owSign(x : f32) -> f32 {
  return select(0.0, -1.0, x < 0.0) + select(0.0, 1.0, x > 0.0);
}
"#;

/// `NOISE_GLSL` (`noise.js:12-218`), transcribed function for function.
///
/// Every loop and branch below is inside a `&str`: it is shader text, and a
/// `for` over nine Worley cells is exactly what the source writes.
pub const NOISE: &str = r#"
// ---------------------------------------------------------------- hashes ----
fn owHash11(p0 : f32) -> f32 {
  var p = fract(p0 * 0.1031);
  p = p * (p + 33.33);
  p = p * (p + p);
  return fract(p);
}
fn owHash12(p : vec2<f32>) -> f32 {
  var p3 = fract(p.xyx * 0.1031);
  p3 = p3 + vec3<f32>(dot(p3, p3.yzx + vec3<f32>(33.33)));
  return fract((p3.x + p3.y) * p3.z);
}
fn owHash22(p : vec2<f32>) -> vec2<f32> {
  var p3 = fract(p.xyx * vec3<f32>(0.1031, 0.1030, 0.0973));
  p3 = p3 + vec3<f32>(dot(p3, p3.yzx + vec3<f32>(33.33)));
  return fract((p3.xx + p3.yz) * p3.zy);
}
fn owHash32(p : vec2<f32>) -> vec3<f32> {
  var p3 = fract(p.xyx * vec3<f32>(0.1031, 0.1030, 0.0973));
  p3 = p3 + vec3<f32>(dot(p3, p3.yxz + vec3<f32>(33.33)));
  return fract((p3.xxy + p3.yzz) * p3.zyx);
}
fn owHash42(p : vec2<f32>) -> vec4<f32> {
  var p4 = fract(p.xyxy * vec4<f32>(0.1031, 0.1030, 0.0973, 0.1099));
  p4 = p4 + vec4<f32>(dot(p4, p4.wzxy + vec4<f32>(33.33)));
  return fract((p4.xxyz + p4.yzzw) * p4.zywx);
}

// ------------------------------------------------------- gradient noise ----
fn owGrad2(i : vec2<f32>, per : vec2<f32>) -> vec2<f32> {
  let a = owHash12(owMod2(i, per) + vec2<f32>(0.317)) * 6.28318530718;
  return vec2<f32>(cos(a), sin(a));
}

// Periodic gradient (Perlin) noise. Returns ~[-1,1].
fn owNoise(p : vec2<f32>, per : vec2<f32>) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let u = f * f * f * (f * (f * 6.0 - vec2<f32>(15.0)) + vec2<f32>(10.0));
  let a = dot(owGrad2(i + vec2<f32>(0.0, 0.0), per), f - vec2<f32>(0.0, 0.0));
  let b = dot(owGrad2(i + vec2<f32>(1.0, 0.0), per), f - vec2<f32>(1.0, 0.0));
  let c = dot(owGrad2(i + vec2<f32>(0.0, 1.0), per), f - vec2<f32>(0.0, 1.0));
  let d = dot(owGrad2(i + vec2<f32>(1.0, 1.0), per), f - vec2<f32>(1.0, 1.0));
  return owMix(owMix(a, b, u.x), owMix(c, d, u.x), u.y) * 1.4142;
}
fn owNoise01(p : vec2<f32>, per : vec2<f32>) -> f32 { return owNoise(p, per) * 0.5 + 0.5; }

// Periodic value noise — blockier, good for cell-ish tint variation.
fn owValue(p : vec2<f32>, per : vec2<f32>) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let u = f * f * (vec2<f32>(3.0) - 2.0 * f);
  let a = owHash12(owMod2(i + vec2<f32>(0.0, 0.0), per) + vec2<f32>(1.7));
  let b = owHash12(owMod2(i + vec2<f32>(1.0, 0.0), per) + vec2<f32>(1.7));
  let c = owHash12(owMod2(i + vec2<f32>(0.0, 1.0), per) + vec2<f32>(1.7));
  let d = owHash12(owMod2(i + vec2<f32>(1.0, 1.0), per) + vec2<f32>(1.7));
  return owMix(owMix(a, b, u.x), owMix(c, d, u.x), u.y);
}

// ------------------------------------------------------------------ fbm ----
fn owFbm(p0 : vec2<f32>, per0 : vec2<f32>, oct : i32, gain : f32) -> f32 {
  var p = p0;
  var per = per0;
  var s = 0.0;
  var a = 0.5;
  var n = 0.0;
  for (var i : i32 = 0; i < 10; i = i + 1) {
    if (i >= oct) { break; }
    s = s + a * owNoise(p, per);
    n = n + a;
    p = p * 2.0; per = per * 2.0; a = a * gain;
  }
  return s / max(n, 1e-4);
}
fn owFbm01(p : vec2<f32>, per : vec2<f32>, oct : i32, gain : f32) -> f32 { return owFbm(p, per, oct, gain) * 0.5 + 0.5; }

// Ridged fbm — sharp creases, good for cracks / rock. Returns [0,1].
fn owRidged(p0 : vec2<f32>, per0 : vec2<f32>, oct : i32, gain : f32) -> f32 {
  var p = p0;
  var per = per0;
  var s = 0.0;
  var a = 0.5;
  var n = 0.0;
  for (var i : i32 = 0; i < 10; i = i + 1) {
    if (i >= oct) { break; }
    let v = 1.0 - abs(owNoise(p, per));
    s = s + a * v * v;
    n = n + a;
    p = p * 2.0; per = per * 2.0; a = a * gain;
  }
  return s / max(n, 1e-4);
}

// Billowy fbm — puffy clumps, good for rust blooms and clay.
fn owBillow(p0 : vec2<f32>, per0 : vec2<f32>, oct : i32, gain : f32) -> f32 {
  var p = p0;
  var per = per0;
  var s = 0.0;
  var a = 0.5;
  var n = 0.0;
  for (var i : i32 = 0; i < 10; i = i + 1) {
    if (i >= oct) { break; }
    s = s + a * abs(owNoise(p, per));
    n = n + a;
    p = p * 2.0; per = per * 2.0; a = a * gain;
  }
  return s / max(n, 1e-4);
}

// --------------------------------------------------------- domain warp -----
fn owWarp(p : vec2<f32>, per : vec2<f32>, amp : f32, oct : i32) -> vec2<f32> {
  let q = vec2<f32>(owFbm(p + vec2<f32>(1.7, 9.2), per, oct, 0.5),
                    owFbm(p + vec2<f32>(8.3, 2.8), per, oct, 0.5));
  return p + amp * q;
}

// -------------------------------------------------------------- worley -----
// Periodic Worley/Voronoi.
//  .x = F1 distance, .y = F2 distance, .z = hash id of the F1 cell,
//  .w = second hash of the F1 cell.
fn owWorley(p : vec2<f32>, per : vec2<f32>, jitter : f32) -> vec4<f32> {
  let ip = floor(p);
  let fp = fract(p);
  var f1 = 8.0;
  var f2 = 8.0;
  var id = vec2<f32>(0.0);
  for (var y : i32 = -1; y <= 1; y = y + 1) {
    for (var x : i32 = -1; x <= 1; x = x + 1) {
      let g = vec2<f32>(f32(x), f32(y));
      let cell = owMod2(ip + g, per);
      let o = owHash22(cell + vec2<f32>(0.771)) * jitter + vec2<f32>((1.0 - jitter) * 0.5);
      let r = g + o - fp;
      let d = dot(r, r);
      if (d < f1) { f2 = f1; f1 = d; id = owHash22(cell + vec2<f32>(3.117)); }
      else if (d < f2) { f2 = d; }
    }
  }
  return vec4<f32>(sqrt(f1), sqrt(f2), id);
}

// Distance to the *edge* of the Voronoi cell (Quilez two-pass). Much better
// looking crack networks than F2-F1. Returns [0, ~0.7].
fn owVoronoiEdge(p : vec2<f32>, per : vec2<f32>, jitter : f32) -> f32 {
  let ip = floor(p);
  let fp = fract(p);
  var mr = vec2<f32>(0.0);
  var mg = vec2<f32>(0.0);
  var md = 8.0;
  for (var y : i32 = -1; y <= 1; y = y + 1) {
    for (var x : i32 = -1; x <= 1; x = x + 1) {
      let g = vec2<f32>(f32(x), f32(y));
      let o = owHash22(owMod2(ip + g, per) + vec2<f32>(0.771)) * jitter + vec2<f32>((1.0 - jitter) * 0.5);
      let r = g + o - fp;
      let d = dot(r, r);
      if (d < md) { md = d; mr = r; mg = g; }
    }
  }
  md = 8.0;
  for (var y : i32 = -2; y <= 2; y = y + 1) {
    for (var x : i32 = -2; x <= 2; x = x + 1) {
      let g = mg + vec2<f32>(f32(x), f32(y));
      let o = owHash22(owMod2(ip + g, per) + vec2<f32>(0.771)) * jitter + vec2<f32>((1.0 - jitter) * 0.5);
      let r = g + o - fp;
      let diff = r - mr;
      if (dot(diff, diff) > 1e-5) {
        md = min(md, dot(0.5 * (mr + r), normalize(diff)));
      }
    }
  }
  return md;
}

// Crack network: warped voronoi edges, thinned and broken up so lines
// terminate instead of forming a perfect mesh. Returns [0,1], 1 = deep crack.
fn owCracks(p : vec2<f32>, per : vec2<f32>, jitter : f32, width : f32, breakUp : f32) -> f32 {
  let wp = owWarp(p, per, 0.20, 3);
  let e = owVoronoiEdge(wp, per, jitter);
  var c = 1.0 - owSmoothstep(0.0, width, e);
  // Break the network so it reads as damage, not as a net.
  let mask = owFbm01(p * 1.7 + vec2<f32>(11.3), per * 1.7, 4, 0.55);
  c = c * owSmoothstep(breakUp, breakUp + 0.28, mask);
  return owClamp(c, 0.0, 1.0);
}

// ------------------------------------------------------------ utilities ----
fn owSat(x : f32) -> f32 { return owClamp(x, 0.0, 1.0); }
fn owSat3(x : vec3<f32>) -> vec3<f32> { return owClamp3(x, vec3<f32>(0.0), vec3<f32>(1.0)); }
fn owRemap(x : f32, a : f32, b : f32, c : f32, d : f32) -> f32 {
  return c + (d - c) * owClamp((x - a) / max(b - a, 1e-5), 0.0, 1.0);
}
fn owRot(p : vec2<f32>, a : f32) -> vec2<f32> {
  let s = sin(a);
  let c = cos(a);
  return mat2x2<f32>(vec2<f32>(c, -s), vec2<f32>(s, c)) * p;
}
// sRGB hex-ish helper: authoring colours in gamma space, output linear.
fn owSRGB(c : vec3<f32>) -> vec3<f32> {
  return owMix3v(pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c / 12.92, owStep3(c, vec3<f32>(0.04045)));
}
// Anisotropic shear that preserves tileability: 'k' and 'stretch' must be
// integers so the lattice still wraps on 'per'.
fn owShear(p : vec2<f32>, k : f32, stretch : f32) -> vec2<f32> {
  return vec2<f32>(p.x + p.y * k, p.y * stretch);
}
fn owShearPer(per : vec2<f32>, stretch : f32) -> vec2<f32> {
  return vec2<f32>(per.x, per.y * stretch);
}

// Scratch lines: long thin streaks running along a sheared axis. [0,1].
fn owScratches(p : vec2<f32>, per : vec2<f32>, stretch : f32, k : f32, thin : f32) -> f32 {
  let q = owShear(p, k, stretch);
  let qper = owShearPer(per, stretch);
  let n = owFbm01(q, qper, 4, 0.5);
  return owSmoothstep(thin, thin + 0.06, n) * (1.0 - owSmoothstep(thin + 0.06, thin + 0.2, n));
}
"#;

/// `DETAIL_SRC` (`generator.js:91-120`) — the shared micro-detail tile.
///
/// The source's NYQUIST note (`generator.js:80-90`) is why every band is capped
/// at `K = 20`: "the tile is 1024 px across 0.25 m, so one texel is 0.244 mm …
/// anything past K≈24 is under five texels and bakes as white noise."
pub const DETAIL: &str = r#"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(8.0);
  let p = uv * P + vec2<f32>(U.seed);
  // ~10 mm swell, ~3.5 mm tooth
  let a = owFbm01(p * 3.0, P * 3.0, 4, 0.55);
  let b = owFbm01(p * 9.0, P * 9.0, 4, 0.52);
  // 3.9 mm pits and 1.6 mm grains — both wide enough to survive two mip levels
  let pores = owWorley(p * 8.0, P * 8.0, 1.0);
  let grit  = owWorley(p * 20.0, P * 20.0, 1.0);
  let scr = owScratches(p * 2.5, P * 2.5, 16.0, 1.0, 0.66)
          + owScratches(p * 4.0 + vec2<f32>(5.0), P * 4.0, 11.0, -2.0, 0.70) * 0.8;
  // Proud grains: a solid, rounded bump rather than a threshold speck.
  let gritA = owSmoothstep(0.34, 0.08, pores.x) * owStep(0.38, pores.z);
  let gritB = owSmoothstep(0.30, 0.06, grit.x) * owStep(0.34, grit.z);
  let pit   = owSmoothstep(0.26, 0.0, pores.x) * owStep(0.72, pores.w);
  h = 0.5 + (a - 0.5) * 0.34 + (b - 0.5) * 0.26;
  h = h - pit * 0.38;
  h = h + gritA * 0.26 * (0.5 + grit.z) + gritB * 0.20;
  h = h - owClamp(scr, 0.0, 1.0) * 0.18;
  // Albedo tracks the grain so a proud grain reads light and its trough reads
  // dark; the shader scales this by the per-surface detail albedo amount.
  alb = vec3<f32>(0.5 + (a - 0.5) * 0.22 + (b - 0.5) * 0.15
           + gritA * 0.16 + gritB * 0.10 - pit * 0.14);
  rough = 0.5 + (b - 0.5) * 0.5;
  metal = 0.0;
  ao = 1.0 - pit * 0.45 - gritB * 0.10;
  h = owClamp(h, 0.0, 1.0);
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
"#;

/// `MACRO_SRC` (`generator.js:127-137`) — four bands of low-frequency variation
/// used by every material to break up tiling: R = very low fbm, G = warped
/// blotches, B = mid fbm, A = fine fbm.
pub const MACRO: &str = r#"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  var alb = *albOut; var h = *hOut; var rough = *roughOut; var metal = *metalOut; var ao = *aoOut;
  let P = vec2<f32>(6.0);
  let p = uv * P + vec2<f32>(U.seed * 3.0);
  let a = owFbm01(p * 0.5, P * 0.5, 4, 0.62);
  let b = owFbm01(owWarp(p * 1.0, P, 1.1, 3), P, 4, 0.58);
  let c = owFbm01(p * 2.5, P * 2.5, 4, 0.55);
  let d = owFbm01(p * 7.0, P * 7.0, 4, 0.5);
  alb = vec3<f32>(a, b, c);
  h = d;
  rough = 0.5; metal = 0.0; ao = 1.0;
  *albOut = alb; *hOut = h; *roughOut = rough; *metalOut = metal; *aoOut = ao;
}
"#;
