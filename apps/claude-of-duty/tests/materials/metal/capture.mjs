/**
 * Golden capture for `apps/claude-of-duty/src/materials/surfaces/metal.rs`.
 *
 * `src/materials/glsl/surfaces-metal.js` (RUST_HELPERS + METAL_RUST +
 * METAL_PAINTED + METAL_BRUSHED + CORRUGATED) and the `noise.js` functions it
 * calls are GLSL held in JavaScript template-string literals — they never ran
 * anywhere but a browser GPU, so there is no JavaScript function to import
 * and call as a genuine oracle (same situation as `tests/sky/capture.mjs`'s
 * `*_FRAG` bodies and `tests/materials_surfaces_ground/capture.mjs`). Every
 * function below is therefore a hand transcription of the GLSL to plain JS
 * doubles, kept line-for-line faithful to the source rather than tidied, so
 * it is a second, independently-reviewable translation to compare the Rust
 * port against — not a ground truth neither transcription can be wrong
 * against. Read the GLSL, this file, and `metal.rs` side by side.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 */

// ---------------------------------------------------------------------------
// vec2/vec3/vec4 helpers.
// ---------------------------------------------------------------------------
const v2 = (x, y) => ({ x, y });
const v2add = (a, b) => v2(a.x + b.x, a.y + b.y);
const v2addS = (a, s) => v2(a.x + s, a.y + s);
const v2mul = (a, b) => v2(a.x * b.x, a.y * b.y);
const v2scale = (a, s) => v2(a.x * s, a.y * s);
const v2floor = (a) => v2(Math.floor(a.x), Math.floor(a.y));
const v2fract = (a) => v2(glFract(a.x), glFract(a.y));
const v2length = (a) => Math.sqrt(a.x * a.x + a.y * a.y);

const v3 = (x, y, z) => ({ x, y, z });
const v3scale = (a, s) => v3(a.x * s, a.y * s, a.z * s);
const v3addS = (a, s) => v3(a.x + s, a.y + s, a.z + s);
const v3mix = (a, b, t) => v3(glMix(a.x, b.x, t), glMix(a.y, b.y, t), glMix(a.z, b.z, t));
const v3clamp = (a, lo, hi) => v3(glClamp(a.x, lo, hi), glClamp(a.y, lo, hi), glClamp(a.z, lo, hi));

// ---------------------------------------------------------------------------
// GLSL scalar primitives (noise.js:196-217 + bare builtins).
// ---------------------------------------------------------------------------
const glFract = (x) => x - Math.floor(x);
const glMod = (x, y) => x - y * Math.floor(x / y);
const glMix = (a, b, t) => a + (b - a) * t;
const glClamp = (x, a, b) => Math.min(Math.max(x, a), b);
const glStep = (edge, x) => (x < edge ? 0 : 1);
const glSign = (x) => (x > 0 ? 1 : x < 0 ? -1 : 0);
const glSmoothstep = (e0, e1, x) => {
  const t = glClamp((x - e0) / (e1 - e0), 0, 1);
  return t * t * (3 - 2 * t);
};

// ---------------------------------------------------------------------------
// noise.js hashes (noise.js:15-40).
// ---------------------------------------------------------------------------
function owHash11(p) {
  p = glFract(p * 0.1031);
  p *= p + 33.33;
  p *= p + p;
  return glFract(p);
}

function owHash12(p) {
  let p3 = v3(p.x, p.y, p.x);
  p3 = v3(glFract(p3.x * 0.1031), glFract(p3.y * 0.1031), glFract(p3.z * 0.1031));
  const yzx = v3addS(v3(p3.y, p3.z, p3.x), 33.33);
  const d = p3.x * yzx.x + p3.y * yzx.y + p3.z * yzx.z;
  p3 = v3addS(p3, d);
  return glFract((p3.x + p3.y) * p3.z);
}

function owHash22(p) {
  let p3 = v3(p.x * 0.1031, p.y * 0.1030, p.x * 0.0973);
  p3 = v3(glFract(p3.x), glFract(p3.y), glFract(p3.z));
  const yzx = v3addS(v3(p3.y, p3.z, p3.x), 33.33);
  const d = p3.x * yzx.x + p3.y * yzx.y + p3.z * yzx.z;
  p3 = v3addS(p3, d);
  const sum = v2(p3.x + p3.y, p3.x + p3.z);
  const zy = v2(p3.z, p3.y);
  return v2fract(v2mul(sum, zy));
}

// ---------------------------------------------------------------------------
// Gradient noise + fbm family (noise.js:42-107).
// ---------------------------------------------------------------------------
function owGrad2(i, per) {
  const a = owHash12(v2addS(v2(glMod(i.x, per.x), glMod(i.y, per.y)), 0.317)) * 6.28318530718;
  return v2(Math.cos(a), Math.sin(a));
}

function owNoise(p, per) {
  const i = v2floor(p);
  const f = v2fract(p);
  const fade = (v) => v * v * v * (v * (v * 6 - 15) + 10);
  const u = v2(fade(f.x), fade(f.y));
  const dot = (g, d) => g.x * d.x + g.y * d.y;
  const a = dot(owGrad2(v2add(i, v2(0, 0)), per), v2(f.x - 0, f.y - 0));
  const b = dot(owGrad2(v2add(i, v2(1, 0)), per), v2(f.x - 1, f.y - 0));
  const c = dot(owGrad2(v2add(i, v2(0, 1)), per), v2(f.x - 0, f.y - 1));
  const d = dot(owGrad2(v2add(i, v2(1, 1)), per), v2(f.x - 1, f.y - 1));
  return glMix(glMix(a, b, u.x), glMix(c, d, u.x), u.y) * 1.4142;
}

function owFbm(p, per, oct, gain) {
  let s = 0,
    a = 0.5,
    n = 0;
  for (let i = 0; i < 10; i++) {
    if (i >= oct) break;
    s += a * owNoise(p, per);
    n += a;
    p = v2scale(p, 2);
    per = v2scale(per, 2);
    a *= gain;
  }
  return s / Math.max(n, 1e-4);
}

function owFbm01(p, per, oct, gain) {
  return owFbm(p, per, oct, gain) * 0.5 + 0.5;
}

function owBillow(p, per, oct, gain) {
  let s = 0,
    a = 0.5,
    n = 0;
  for (let i = 0; i < 10; i++) {
    if (i >= oct) break;
    s += a * Math.abs(owNoise(p, per));
    n += a;
    p = v2scale(p, 2);
    per = v2scale(per, 2);
    a *= gain;
  }
  return s / Math.max(n, 1e-4);
}

function owWarp(p, per, amp, oct) {
  const q = v2(owFbm(v2add(p, v2(1.7, 9.2)), per, oct, 0.5), owFbm(v2add(p, v2(8.3, 2.8)), per, oct, 0.5));
  return v2add(p, v2scale(q, amp));
}

// ---------------------------------------------------------------------------
// Worley (noise.js:122-138). Returns {f1, f2, idX, idY} matching this port's
// WorleyResult naming, not raw vec4 swizzles.
// ---------------------------------------------------------------------------
function owWorley(p, per, jitter) {
  const ip = v2floor(p);
  const fp = v2fract(p);
  let f1 = 8,
    f2 = 8;
  let id = v2(0, 0);
  for (let y = -1; y <= 1; y++) {
    for (let x = -1; x <= 1; x++) {
      const g = v2(x, y);
      const cell = v2(glMod(ip.x + g.x, per.x), glMod(ip.y + g.y, per.y));
      const h = owHash22(v2addS(cell, 0.771));
      const o = v2addS(v2scale(h, jitter), (1 - jitter) * 0.5);
      const r = v2(g.x + o.x - fp.x, g.y + o.y - fp.y);
      const d = r.x * r.x + r.y * r.y;
      if (d < f1) {
        f2 = f1;
        f1 = d;
        id = owHash22(v2addS(cell, 3.117));
      } else if (d < f2) {
        f2 = d;
      }
    }
  }
  return { f1: Math.sqrt(f1), f2: Math.sqrt(f2), idX: id.x, idY: id.y };
}

// ---------------------------------------------------------------------------
// Shear + scratches (noise.js:204-217).
// ---------------------------------------------------------------------------
function owShear(p, k, stretch) {
  return v2(p.x + p.y * k, p.y * stretch);
}
function owShearPer(per, stretch) {
  return v2(per.x, per.y * stretch);
}
function owScratches(p, per, stretch, k, thin) {
  const q = owShear(p, k, stretch);
  const qper = owShearPer(per, stretch);
  const n = owFbm01(q, qper, 4, 0.5);
  return glSmoothstep(thin, thin + 0.06, n) * (1 - glSmoothstep(thin + 0.06, thin + 0.2, n));
}

// ---------------------------------------------------------------------------
// owSRGB (noise.js:197-199).
// ---------------------------------------------------------------------------
function owSRGB(c) {
  const decode = (ci) => (ci > 0.04045 ? Math.pow((ci + 0.055) / 1.055, 2.4) : ci / 12.92);
  return v3(decode(c.x), decode(c.y), decode(c.z));
}

// ---------------------------------------------------------------------------
// RUST_HELPERS (surfaces-metal.js:8-21).
// ---------------------------------------------------------------------------
function owRustColour(t, grain) {
  const c1 = owSRGB(v3(0.56, 0.29, 0.11));
  const c2 = owSRGB(v3(0.38, 0.18, 0.085));
  const c3 = owSRGB(v3(0.19, 0.1, 0.06));
  const c4 = owSRGB(v3(0.64, 0.4, 0.19));
  let c = v3mix(c1, c2, glSmoothstep(0.15, 0.6, t));
  c = v3mix(c, c3, glSmoothstep(0.55, 1.0, t));
  c = v3mix(c, c4, glSmoothstep(0.55, 0.95, grain) * 0.45);
  return v3scale(c, 0.82 + 0.36 * grain);
}

// ---------------------------------------------------------------------------
// METAL_RUST (surfaces-metal.js:23-88).
// ---------------------------------------------------------------------------
function metalRust(uv, uSeed) {
  const P = v2(8, 8);
  const p = v2addS(v2mul(uv, P), uSeed * 7.7);

  const mill = owFbm01(owShear(v2scale(p, 4), 1, 6), owShearPer(v2scale(P, 4), 6), 4, 0.5);
  const fine = owFbm01(v2scale(p, 22), v2scale(P, 22), 4, 0.5);
  const steel = v3scale(owSRGB(v3(0.33, 0.335, 0.345)), 0.9 + 0.18 * mill);
  let c = steel;
  let h = 0.72 + (mill - 0.5) * 0.02 + (fine - 0.5) * 0.01;
  let rough = 0.4 + (mill - 0.5) * 0.16 + (fine - 0.5) * 0.08;
  let metal = 1.0;
  let ao = 1.0;

  const wp = owWarp(v2scale(p, 1.4), v2scale(P, 1.4), 1.2, 4);
  let bloom = owBillow(wp, v2scale(P, 1.4), 5, 0.6);
  bloom = 1.0 - bloom;
  const spread = owFbm01(v2addS(v2scale(p, 0.7), 12.0), v2scale(P, 0.7), 3, 0.6);
  const rust = glSmoothstep(0.36, 0.72, bloom * (0.55 + 0.85 * spread));
  const rustGrain = owFbm01(v2scale(p, 26), v2scale(P, 26), 4, 0.55);
  const pit = owFbm01(v2scale(p, 24), v2scale(P, 24), 3, 0.5);

  const scaleN = owWorley(v2scale(p, 16), v2scale(P, 16), 1.0).f1;
  const flake =
    glSmoothstep(0.3, 0.1, scaleN) * glSmoothstep(0.25, 0.55, rust) * (1 - glSmoothstep(0.8, 1.0, rust));

  const rustAge = owFbm01(v2addS(v2scale(p, 0.85), 21.0), v2scale(P, 0.85), 4, 0.62);
  const rustCol = owRustColour(rustAge * 0.8 + rust * 0.3, rustGrain);
  c = v3mix(c, rustCol, rust);
  metal = glMix(1.0, 0.0, glSmoothstep(0.15, 0.55, rust));
  rough = glMix(rough, 0.86 + 0.1 * rustGrain, glSmoothstep(0.1, 0.6, rust));
  h += rust * 0.11 * (0.4 + rustGrain) + flake * 0.13;
  h -= glSmoothstep(0.5, 0.95, rust) * pit * 0.14;
  ao -= flake * 0.3 + glSmoothstep(0.6, 1.0, rust) * 0.15;

  const pits = owWorley(v2scale(p, 22), v2scale(P, 22), 1.0);
  const deep = glSmoothstep(0.22, 0.0, pits.f1) * glStep(0.72, pits.idY) * glSmoothstep(0.3, 0.8, rust);
  h -= deep * 0.22;
  ao -= deep * 0.45;
  c = v3mix(c, v3scale(rustCol, 0.35), deep * 0.7);

  let scr = owScratches(v2scale(p, 3), v2scale(P, 3), 12.0, 1.0, 0.6);
  scr += owScratches(v2addS(v2scale(p, 5), 8.0), v2scale(P, 5), 9.0, -2.0, 0.66) * 0.7;
  scr = glClamp(scr, 0, 1) * 0.6;
  c = v3mix(c, owSRGB(v3(0.48, 0.485, 0.495)), scr * 0.8);
  metal = glMix(metal, 1.0, scr * 0.85);
  rough = glMix(rough, 0.24, scr * 0.7);
  h -= scr * 0.01;

  const grime = glSmoothstep(
    0.55,
    0.9,
    owFbm01(v2(p.x * 5.0, p.y * 0.8), v2(P.x * 5.0, Math.max(P.y, 1.0)), 5, 0.55)
  );
  c = v3scale(c, 1.0 - grime * 0.25);
  rough += grime * 0.08;

  return {
    alb: v3clamp(c, 0.02, 0.8),
    h: glClamp(h, 0, 1),
    rough: glClamp(rough, 0.12, 0.99),
    metal: glClamp(metal, 0, 1),
    ao: glClamp(ao, 0.15, 1.0),
  };
}

// ---------------------------------------------------------------------------
// METAL_PAINTED (surfaces-metal.js:90-178).
// ---------------------------------------------------------------------------
function metalPainted(uv, uSeed, uTintA, paramZ) {
  const P = v2(8, 8);
  const p = v2addS(v2mul(uv, P), uSeed * 11.3);

  const mill = owFbm01(owShear(v2scale(p, 5), 1, 8), owShearPer(v2scale(P, 5), 8), 4, 0.5);
  const steel = v3scale(owSRGB(v3(0.33, 0.335, 0.345)), 0.88 + 0.2 * mill);

  const bloom = 1.0 - owBillow(owWarp(v2scale(p, 1.8), v2scale(P, 1.8), 1.1, 4), v2scale(P, 1.8), 5, 0.6);
  const rustField = glSmoothstep(0.6, 0.92, bloom);
  const rustGrain = owFbm01(v2scale(p, 22), v2scale(P, 22), 4, 0.55);
  const rustCol = owRustColour(rustField, rustGrain);

  const peel = owFbm01(v2scale(p, 22), v2scale(P, 22), 4, 0.5);
  const roller = owFbm01(owShear(v2scale(p, 2), 0, 3), owShearPer(v2scale(P, 2), 3), 4, 0.5);
  let paint = v3scale(uTintA, 0.9 + 0.16 * roller);
  paint = v3scale(paint, 0.96 + 0.08 * peel);
  const bleach = glSmoothstep(0.35, 0.85, owFbm01(v2scale(p, 0.8), v2scale(P, 0.8), 3, 0.6));
  paint = v3mix(paint, v3addS(v3scale(paint, 1.25), 0.03), bleach * 0.5);

  const chipField = owFbm01(
    owWarp(v2addS(v2scale(p, 2.6), 4.0), v2scale(P, 2.6), 0.9, 3),
    v2scale(P, 2.6),
    5,
    0.55
  );
  const chipEdge = owFbm01(v2scale(p, 12), v2scale(P, 12), 4, 0.5);
  const chipSrc = chipField * 0.6 + chipEdge * 0.2 + rustField * 0.32 + paramZ * 0.25;
  let chip = glSmoothstep(0.66, 0.92, chipSrc);
  const dings = owWorley(v2scale(p, 20), v2scale(P, 20), 1.0);
  const ding = glSmoothstep(0.14, 0.03, dings.f1) * glStep(0.88, dings.idY);
  chip = glClamp(chip + ding, 0, 1);

  let scr = owScratches(v2scale(p, 2.5), v2scale(P, 2.5), 14.0, 1.0, 0.62);
  scr += owScratches(v2addS(v2scale(p, 4), 21.0), v2scale(P, 4), 10.0, -1.0, 0.66) * 0.8;
  scr = glClamp(scr, 0, 1);

  const primer = owSRGB(v3(0.47, 0.3, 0.18));
  const primerBand = glSmoothstep(0.0, 0.35, chip) * (1 - glSmoothstep(0.35, 0.6, chip));

  let c = paint;
  let r = 0.42 + (peel - 0.5) * 0.22 + bleach * 0.16;
  let mtl = 0.0;
  let h = 0.74 + (roller - 0.5) * 0.02 + (peel - 0.5) * 0.012;
  let ao = 1.0;

  c = v3mix(c, primer, primerBand * 0.7);
  c = v3mix(c, rustCol, glSmoothstep(0.35, 0.75, chip) * (0.55 + 0.45 * rustField));
  c = v3mix(c, steel, glSmoothstep(0.75, 0.95, chip) * (1 - rustField) * 0.9);
  r = glMix(r, 0.88, glSmoothstep(0.3, 0.8, chip) * (0.4 + 0.6 * rustField));
  r = glMix(r, 0.38, glSmoothstep(0.8, 1.0, chip) * (1 - rustField));
  mtl = glMix(0.0, 1.0, glSmoothstep(0.78, 0.96, chip) * (1 - glSmoothstep(0.2, 0.7, rustField)));
  h -= glSmoothstep(0.4, 0.8, chip) * 0.16;
  ao -= glSmoothstep(0.35, 0.7, chip) * 0.22;
  const lip = glSmoothstep(0.3, 0.42, chip) * (1 - glSmoothstep(0.42, 0.55, chip));
  c = v3scale(c, 1.0 + lip * 0.15);
  h += lip * 0.05;

  c = v3mix(c, owSRGB(v3(0.5, 0.505, 0.515)), scr * 0.55);
  mtl = glMix(mtl, 1.0, scr * 0.6);
  r = glMix(r, 0.26, scr * 0.55);

  const streak = owFbm01(v2(p.x * 6.0, p.y * 0.7), v2(P.x * 6.0, Math.max(P.y, 1.0)), 5, 0.55);
  const grime = glSmoothstep(0.52, 0.92, streak);
  c = v3scale(c, 1.0 - grime * 0.3);
  r += grime * 0.1;
  mtl *= 1.0 - grime * 0.5;
  const bleed = glSmoothstep(0.66, 0.95, streak) * glSmoothstep(0.2, 0.6, rustField);
  c = v3mix(c, owSRGB(v3(0.36, 0.19, 0.09)), bleed * 0.45);

  const cavity = 1.0 - glSmoothstep(0.62, 0.78, h);
  c = v3scale(c, 1.0 - cavity * 0.18);

  return {
    alb: v3clamp(c, 0.02, 0.85),
    h: glClamp(h, 0, 1),
    rough: glClamp(r, 0.14, 0.99),
    metal: glClamp(mtl, 0, 1),
    ao: glClamp(ao, 0.2, 1.0),
  };
}

// ---------------------------------------------------------------------------
// METAL_BRUSHED (surfaces-metal.js:180-237).
// ---------------------------------------------------------------------------
function metalBrushed(uv, uSeed) {
  const P = v2(8, 8);
  const p = v2addS(v2mul(uv, P), uSeed * 15.1);

  const bp = owShear(p, 0, 64);
  const BP = owShearPer(P, 64);
  const brush1 = owFbm01(v2scale(bp, 2), v2scale(BP, 2), 4, 0.5);
  const brush2 = owFbm01(v2addS(v2scale(bp, 8), 3.0), v2scale(BP, 8), 3, 0.5);
  const brush3 = owFbm01(owShear(v2scale(p, 4), 0, 24), owShearPer(v2scale(P, 4), 24), 3, 0.5);
  const brush = brush1 * 0.5 + brush2 * 0.32 + brush3 * 0.18;

  const macro = owFbm01(v2scale(p, 0.9), v2scale(P, 0.9), 3, 0.6);

  let c = owSRGB(v3(0.56, 0.565, 0.575));
  c = v3scale(c, 0.93 + 0.13 * brush);
  c = v3scale(c, 0.97 + 0.06 * macro);

  let metal = 1.0;
  let rough = 0.22 + brush * 0.24 + (macro - 0.5) * 0.06;
  let h = 0.78 + (brush - 0.5) * 0.012;
  let ao = 1.0;

  const score = owScratches(v2scale(p, 1.0), P, 40.0, 0.0, 0.6);
  rough += score * 0.22;
  h -= score * 0.006;
  c = v3scale(c, 1.0 - score * 0.05);

  const cross = owScratches(v2scale(p, 3), v2scale(P, 3), 8.0, 3.0, 0.7) * 0.7;
  rough += cross * 0.2;
  h -= cross * 0.004;

  const dent = owFbm01(v2addS(v2scale(p, 3), 7.0), v2scale(P, 3), 3, 0.6);
  h += (dent - 0.5) * 0.05;

  const smudge = glSmoothstep(
    0.58,
    0.86,
    owFbm01(owWarp(v2addS(v2scale(p, 2.2), 19.0), v2scale(P, 2.2), 0.7, 3), v2scale(P, 2.2), 4, 0.55)
  );
  rough += smudge * 0.22;
  c = v3scale(c, 1.0 - smudge * 0.06);
  metal -= smudge * 0.1;

  const grime = glSmoothstep(0.66, 0.95, owFbm01(v2scale(p, 5), v2scale(P, 5), 4, 0.55));
  c = v3mix(c, owSRGB(v3(0.18, 0.175, 0.165)), grime * 0.35);
  rough += grime * 0.18;
  metal -= grime * 0.35;

  return {
    alb: v3clamp(c, 0.02, 0.88),
    h: glClamp(h, 0, 1),
    rough: glClamp(rough, 0.08, 0.95),
    metal: glClamp(metal, 0, 1),
    ao: glClamp(ao, 0.4, 1.0),
  };
}

// ---------------------------------------------------------------------------
// CORRUGATED (surfaces-metal.js:239-323).
// ---------------------------------------------------------------------------
function corrugated(uv, uSeed) {
  const P = v2(8, 8);
  const RIDGES = 12.0;
  const p = v2addS(v2mul(uv, P), uSeed * 6.1);

  const t = uv.x * RIDGES * 6.28318530718;
  const wave = Math.sin(t);
  const profile = glSign(wave) * Math.pow(Math.abs(wave), 0.72) * 0.5 + 0.5;
  const panel = (uv.x * RIDGES) / 4.0;
  const panelId = Math.floor(panel);
  const lap = glSmoothstep(0.0, 0.06, glFract(panel)) * glSmoothstep(0.0, 0.06, 1.0 - glFract(panel));
  const panelStep = (owHash11(panelId + uSeed) - 0.5) * 0.05;

  const dents = owFbm01(v2scale(p, 2.2), v2scale(P, 2.2), 4, 0.6);
  const fine = owFbm01(v2scale(p, 11), v2scale(P, 11), 4, 0.5);

  let h = 0.18 + profile * 0.62 + panelStep + (dents - 0.5) * 0.07 + (fine - 0.5) * 0.012;
  h -= (1.0 - lap) * 0.06;

  const sp = owWorley(v2scale(p, 7), v2scale(P, 7), 1.0);
  const spangle = glSmoothstep(0.55, 0.05, sp.f1);
  const zinc = owSRGB(v3(0.52, 0.535, 0.545));
  let c = v3mix(v3scale(zinc, 0.86), v3scale(zinc, 1.12), spangle * (0.3 + 0.7 * sp.idX));
  c = v3scale(c, 0.94 + 0.12 * fine);
  let metal = 1.0;
  let rough = 0.34 + (1.0 - spangle) * 0.16 + (fine - 0.5) * 0.08;
  let ao = 1.0;

  const valley = 1.0 - profile;
  const rustField = glSmoothstep(
    0.62,
    0.98,
    (1.0 - owBillow(owWarp(v2scale(p, 1.6), v2scale(P, 1.6), 1.0, 4), v2scale(P, 1.6), 5, 0.6)) *
      (0.58 + 0.4 * valley) +
      (1.0 - uv.y) * 0.16
  );
  const rustGrain = owFbm01(v2scale(p, 22), v2scale(P, 22), 4, 0.55);
  const rustCol = owRustColour(rustField, rustGrain);
  c = v3mix(c, rustCol, rustField);
  metal = glMix(metal, 0.0, glSmoothstep(0.15, 0.6, rustField));
  rough = glMix(rough, 0.88 + 0.08 * rustGrain, glSmoothstep(0.1, 0.6, rustField));
  h += rustField * 0.02 * rustGrain;

  const hole = owWorley(v2addS(v2scale(p, 5), 31.0), v2scale(P, 5), 0.95);
  const perf = glSmoothstep(0.1, 0.02, hole.f1) * glStep(0.94, hole.idY) * glSmoothstep(0.5, 0.9, rustField);
  h -= perf * 0.5;
  ao -= perf * 0.7;
  c = v3mix(c, v3scale(rustCol, 0.25), perf);

  const crown = glSmoothstep(0.72, 0.95, profile);
  const fx = v2(glFract(uv.x * RIDGES) - 0.5, glFract(uv.y * 3.0) - 0.5);
  const fd = v2length(v2mul(fx, v2(1.0, RIDGES / 3.0)));
  const screwRnd = owHash12(v2addS(v2floor(v2(uv.x * RIDGES, uv.y * 3.0)), uSeed));
  const screw = glSmoothstep(0.16, 0.11, fd) * crown * glStep(0.25, screwRnd);
  const washer = glSmoothstep(0.24, 0.18, fd) * crown * glStep(0.25, screwRnd);
  h += washer * 0.02 + screw * 0.035;
  c = v3mix(c, owSRGB(v3(0.12, 0.115, 0.11)), washer * 0.8);
  c = v3mix(c, v3mix(owSRGB(v3(0.4, 0.405, 0.41)), rustCol, rustField), screw);
  rough = glMix(rough, 0.85, washer * 0.8);
  rough = glMix(rough, 0.42 + rustField * 0.4, screw);
  metal = glMix(metal, 0.0, washer * 0.9);
  metal = glMix(metal, 1.0 - rustField, screw);
  ao -= (washer - screw) * 0.35;
  const weep =
    washer * 0.0 +
    glSmoothstep(0.34, 0.2, fd) * glStep(0.25, screwRnd) * crown * glSmoothstep(0.0, 0.5, glFract(uv.y * 3.0) - 0.5);
  c = v3mix(c, owSRGB(v3(0.33, 0.17, 0.08)), glClamp(weep, 0, 1) * 0.5);

  const dirt = valley * glSmoothstep(0.35, 0.8, owFbm01(v2scale(p, 3), v2scale(P, 3), 4, 0.55));
  c = v3mix(c, owSRGB(v3(0.2, 0.185, 0.16)), dirt * 0.4);
  rough += dirt * 0.14;
  metal *= 1.0 - dirt * 0.5;
  ao -= valley * 0.18;

  return {
    alb: v3clamp(c, 0.02, 0.85),
    h: glClamp(h, 0, 1),
    rough: glClamp(rough, 0.14, 0.99),
    metal: glClamp(metal, 0, 1),
    ao: glClamp(ao, 0.15, 1.0),
  };
}

// ---------------------------------------------------------------------------
// Capture grid: a fixed set of uv points per generator, plus one texel
// engineered to land `wave` exactly on `t = pi` for CORRUGATED (the
// `sign(0)` trap the port's module doc calls out). RIDGES = 12, so
// `t = uv.x * 12 * 2*pi == pi` at `uv.x = 1/24`.
// ---------------------------------------------------------------------------
const pts = [
  v2(0.0, 0.0),
  v2(0.13, 0.77),
  v2(0.42, 0.09),
  v2(0.91, 0.36),
  v2(1.0, 1.0),
  v2(1 / 24, 0.5), // corrugated sign(0) texel
];

const tintA = owSRGB(v3(0x4a / 255, 0x53 / 255, 0x40 / 255)); // LIBRARY metal_painted's 0x4a5340

const out = {
  metal_rust: { seed: 37.0, samples: pts.map((uv) => ({ uv, s: metalRust(uv, 37.0) })) },
  metal_painted: {
    seed: 61.0,
    tintA,
    paramZ: 0.0,
    samples: pts.map((uv) => ({ uv, s: metalPainted(uv, 61.0, tintA, 0.0) })),
  },
  metal_brushed: { seed: 83.0, samples: pts.map((uv) => ({ uv, s: metalBrushed(uv, 83.0) })) },
  corrugated: { seed: 29.0, samples: pts.map((uv) => ({ uv, s: corrugated(uv, 29.0) })) },
};

console.log(JSON.stringify(out));
