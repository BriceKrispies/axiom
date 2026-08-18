/**
 * Golden capture for the Claude-of-Duty ground-surface generators
 * (asphalt/sand/dirt/gravel).
 *
 * `src/materials/glsl/surfaces-ground.js` and the `src/materials/glsl/noise.js`
 * it builds on are both GLSL held in JavaScript template-string literals —
 * shader source that never ran anywhere but a browser GPU. There is no
 * JavaScript function to `import` and call as a genuine oracle (unlike
 * `tests/sky/capture.mjs`'s `celestial.js`/`atmosphere.js` CPU tail, which
 * IS real, importable JS). So, same discipline as that script's noise/LUT
 * half: every function below is a hand-transcription of the GLSL into plain
 * JS doubles, independent of (but line-referenced against, the same as) this
 * crate's Rust transcription in
 * `apps/claude-of-duty/src/materials/{noise,surfaces/ground}.rs`. Pinning the
 * Rust port against this catches drift between the Rust transcription and a
 * careful reading of the GLSL — it cannot catch a mistake both
 * transcriptions share, which is why this file states the caveat instead of
 * pretending to be an oracle.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 */

/* ------------------------------------------------------------------ */
/* GLSL scalar/vec2 primitives — noise.js has no owMix/owClamp/owStep    */
/* helpers of its own; these are the bare GLSL builtins every function   */
/* below uses.                                                           */
/* ------------------------------------------------------------------ */
const glFract = (x) => x - Math.floor(x);
const glMod = (x, y) => x - y * Math.floor(x / y);
const glMix = (a, b, t) => a + (b - a) * t;
const glClamp = (x, a, b) => Math.min(Math.max(x, a), b);
const glSmoothstep = (e0, e1, x) => {
  const t = glClamp((x - e0) / (e1 - e0), 0, 1);
  return t * t * (3 - 2 * t);
};
const glStep = (edge, x) => (x < edge ? 0 : 1);

/* vec2 helpers */
const v2 = (x, y) => [x, y];
const add2 = (a, b) => [a[0] + b[0], a[1] + b[1]];
const sub2 = (a, b) => [a[0] - b[0], a[1] - b[1]];
const mul2 = (a, b) => [a[0] * b[0], a[1] * b[1]];
const scale2 = (a, s) => [a[0] * s, a[1] * s];
const addS2 = (a, s) => [a[0] + s, a[1] + s];
const dot2 = (a, b) => a[0] * b[0] + a[1] * b[1];
const floor2 = (a) => [Math.floor(a[0]), Math.floor(a[1])];
const fract2 = (a) => [glFract(a[0]), glFract(a[1])];
const mod2 = (a, per) => [glMod(a[0], per[0]), glMod(a[1], per[1])];

/* vec3 helpers (just enough for owHash12/22, owSRGB) */
const v3 = (x, y, z) => [x, y, z];
const scale3 = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
const addS3 = (a, s) => [a[0] + s, a[1] + s, a[2] + s];
const mul3v = (a, b) => [a[0] * b[0], a[1] * b[1], a[2] * b[2]];
const dot3 = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const fract3 = (a) => [glFract(a[0]), glFract(a[1]), glFract(a[2])];
const mix3 = (a, b, t) => [glMix(a[0], b[0], t), glMix(a[1], b[1], t), glMix(a[2], b[2], t)];
const add3 = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];

/* ------------------------------------------------------------------ */
/* noise.js:15-218, transcribed function-for-function.                  */
/* ------------------------------------------------------------------ */

/** owHash12, noise.js:21-25. */
function owHash12(p) {
  let p3 = fract3(scale3(v3(p[0], p[1], p[0]), 0.1031));
  const yzx = addS3(v3(p3[1], p3[2], p3[0]), 33.33);
  const d = dot3(p3, yzx);
  p3 = addS3(p3, d);
  return glFract((p3[0] + p3[1]) * p3[2]);
}

/** owHash22, noise.js:26-30. */
function owHash22(p) {
  let p3 = fract3(mul3v(v3(p[0], p[1], p[0]), v3(0.1031, 0.1030, 0.0973)));
  const yzx = addS3(v3(p3[1], p3[2], p3[0]), 33.33);
  const d = dot3(p3, yzx);
  p3 = addS3(p3, d);
  // (p3.xx + p3.yz) * p3.zy
  const sum = [p3[0] + p3[1], p3[0] + p3[2]];
  const zy = [p3[2], p3[1]];
  return fract2(mul2(sum, zy));
}

/** owGrad2, noise.js:43-46. */
function owGrad2(i, per) {
  const a = owHash12(addS2(mod2(i, per), 0.317)) * 6.28318530718;
  return [Math.cos(a), Math.sin(a)];
}

/** owNoise, noise.js:49-57. */
function owNoise(p, per) {
  const i = floor2(p);
  const f = fract2(p);
  const fade = (v) => v * v * v * (v * (v * 6.0 - 15.0) + 10.0);
  const u = [fade(f[0]), fade(f[1])];
  const a = dot2(owGrad2(add2(i, [0, 0]), per), sub2(f, [0, 0]));
  const b = dot2(owGrad2(add2(i, [1, 0]), per), sub2(f, [1, 0]));
  const c = dot2(owGrad2(add2(i, [0, 1]), per), sub2(f, [0, 1]));
  const d = dot2(owGrad2(add2(i, [1, 1]), per), sub2(f, [1, 1]));
  return glMix(glMix(a, b, u[0]), glMix(c, d, u[0]), u[1]) * 1.4142;
}

/** owFbm, noise.js:72-81. */
function owFbm(p0, per0, oct, gain) {
  let p = p0, per = per0, s = 0, a = 0.5, n = 0;
  for (let i = 0; i < 10; i++) {
    if (i >= oct) break;
    s += a * owNoise(p, per);
    n += a;
    p = scale2(p, 2.0);
    per = scale2(per, 2.0);
    a *= gain;
  }
  return s / Math.max(n, 1e-4);
}
/** owFbm01, noise.js:82. */
function owFbm01(p, per, oct, gain) {
  return owFbm(p, per, oct, gain) * 0.5 + 0.5;
}

/** owBillow, noise.js:98-107. */
function owBillow(p0, per0, oct, gain) {
  let p = p0, per = per0, s = 0, a = 0.5, n = 0;
  for (let i = 0; i < 10; i++) {
    if (i >= oct) break;
    s += a * Math.abs(owNoise(p, per));
    n += a;
    p = scale2(p, 2.0);
    per = scale2(per, 2.0);
    a *= gain;
  }
  return s / Math.max(n, 1e-4);
}

/** owWarp, noise.js:110-114. */
function owWarp(p, per, amp, oct) {
  const q = [
    owFbm(add2(p, [1.7, 9.2]), per, oct, 0.5),
    owFbm(add2(p, [8.3, 2.8]), per, oct, 0.5),
  ];
  return add2(p, scale2(q, amp));
}

/** owWorley, noise.js:122-138. Returns {f1,f2,idX,idY}. */
function owWorley(p, per, jitter) {
  const ip = floor2(p);
  const fp = fract2(p);
  let f1 = 8.0, f2 = 8.0;
  let id = [0, 0];
  for (let y = -1; y <= 1; y++) {
    for (let x = -1; x <= 1; x++) {
      const g = [x, y];
      const cell = mod2(add2(ip, g), per);
      const o = addS2(scale2(owHash22(addS2(cell, 0.771)), jitter), (1.0 - jitter) * 0.5);
      const r = sub2(add2(g, o), fp);
      const d = dot2(r, r);
      if (d < f1) {
        f2 = f1;
        f1 = d;
        id = owHash22(addS2(cell, 3.117));
      } else if (d < f2) {
        f2 = d;
      }
    }
  }
  return { f1: Math.sqrt(f1), f2: Math.sqrt(f2), idX: id[0], idY: id[1] };
}

/** owVoronoiEdge, noise.js:144-170. */
function owVoronoiEdge(p, per, jitter) {
  const ip = floor2(p);
  const fp = fract2(p);
  let mr = [0, 0], mg = [0, 0], md = 8.0;
  for (let y = -1; y <= 1; y++) {
    for (let x = -1; x <= 1; x++) {
      const g = [x, y];
      const o = addS2(scale2(owHash22(addS2(mod2(add2(ip, g), per), 0.771)), jitter), (1.0 - jitter) * 0.5);
      const r = sub2(add2(g, o), fp);
      const d = dot2(r, r);
      if (d < md) {
        md = d;
        mr = r;
        mg = g;
      }
    }
  }
  md = 8.0;
  for (let y = -2; y <= 2; y++) {
    for (let x = -2; x <= 2; x++) {
      const g = add2(mg, [x, y]);
      const o = addS2(scale2(owHash22(addS2(mod2(add2(ip, g), per), 0.771)), jitter), (1.0 - jitter) * 0.5);
      const r = sub2(add2(g, o), fp);
      const diff = sub2(r, mr);
      const dd = dot2(diff, diff);
      if (dd > 1e-5) {
        const len = Math.sqrt(dd);
        const nrm = scale2(diff, 1 / len);
        md = Math.min(md, dot2(scale2(add2(mr, r), 0.5), nrm));
      }
    }
  }
  return md;
}

/** owCracks, noise.js:176-184. */
function owCracks(p, per, jitter, width, breakUp) {
  const wp = owWarp(p, per, 0.20, 3);
  const e = owVoronoiEdge(wp, per, jitter);
  let c = 1.0 - glSmoothstep(0.0, width, e);
  const mask = owFbm01(addS2(scale2(p, 1.7), 11.3), scale2(per, 1.7), 4, 0.55);
  c *= glSmoothstep(breakUp, breakUp + 0.28, mask);
  return glClamp(c, 0.0, 1.0);
}

/** owSRGB, noise.js:197-199. `mix(pow(...), c/12.92, step(c, 0.04045))`:
 * `step` is 1 when `c[i] < 0.04045`... actually GLSL `step(edge, x)` returns
 * 1 when `x >= edge`, so `step(c, vec3(0.04045))` here has `edge = c`,
 * `x = 0.04045` — i.e. it is 1 when `0.04045 >= c[i]`, matching the standard
 * sRGB decode's low-toe branch. Transcribed with that edge/x order exactly
 * as the source calls it (`step(c, vec3(0.04045))`, not `step(vec3(0.04045), c)`). */
function owSRGB(c) {
  const decode = (ci) => {
    const powPart = Math.pow((ci + 0.055) / 1.055, 2.4);
    const linPart = ci / 12.92;
    const s = glStep(ci, 0.04045); // step(edge=ci, x=0.04045)
    return glMix(powPart, linPart, s);
  };
  return [decode(c[0]), decode(c[1]), decode(c[2])];
}

/** owShear, noise.js:204-206. */
function owShear(p, k, stretch) {
  return [p[0] + p[1] * k, p[1] * stretch];
}
/** owShearPer, noise.js:207-209. */
function owShearPer(per, stretch) {
  return [per[0], per[1] * stretch];
}

/* ------------------------------------------------------------------ */
/* surfaces-ground.js — the four owSurface bodies, transcribed line-    */
/* for-line against `C:/dev/Claude-of-Duty/src/materials/glsl/          */
/* surfaces-ground.js`.                                                 */
/* ------------------------------------------------------------------ */

/** ASPHALT owSurface, surfaces-ground.js:19-118. */
function asphaltSurface(uv, uSeed) {
  const P = [8.0, 8.0];
  const p = addS2(mul2(uv, P), uSeed * 6.9);

  const macro = owFbm01(scale2(p, 0.55), scale2(P, 0.5), 4, 0.6);
  const mid = owFbm01(scale2(p, 3.0), scale2(P, 3.0), 5, 0.5);
  const fine = owFbm01(scale2(p, 16.0), scale2(P, 16.0), 4, 0.5);

  const cFresh = owSRGB([0.115, 0.115, 0.122]);
  const cWorn = owSRGB([0.300, 0.298, 0.295]);
  let c = mix3(cFresh, cWorn, glSmoothstep(0.25, 0.85, macro) * 0.85);
  c = scale3(c, 0.94 + 0.12 * fine);

  let h = 0.60 + (mid - 0.5) * 0.06;
  let rough = 0.78 + (mid - 0.5) * 0.10 + (fine - 0.5) * 0.14;
  const metal = 0.0;
  let ao = 1.0;

  const ap = owWarp(p, P, 0.10, 3);
  const big = owWorley(scale2(ap, 12.0), scale2(P, 12.0), 1.0);
  const bigM = glSmoothstep(0.40, 0.16, big.f1);
  const bigExposed = bigM * glSmoothstep(
    0.30, 0.62,
    owFbm01(addS2(scale2(p, 2.2), 3.0), scale2(P, 2.0), 4, 0.5) + big.idY * 0.5,
  );
  const small = owWorley(addS2(scale2(ap, 22.0), 7.0), scale2(P, 22.0), 1.0);
  const smallM = glSmoothstep(0.36, 0.10, small.f1);
  const smallExposed = smallM * glStep(0.30, small.idY);
  const grit = owWorley(addS2(scale2(ap, 28.0), 3.0), scale2(P, 28.0), 1.0);
  const gritM = glSmoothstep(0.32, 0.06, grit.f1) * glStep(0.45, grit.idX);

  const stoneA = owSRGB([0.400, 0.392, 0.378]);
  const stoneB = owSRGB([0.210, 0.200, 0.192]);
  const stoneC = owSRGB([0.560, 0.520, 0.470]);
  let stone = mix3(stoneA, stoneB, big.idX);
  stone = mix3(stone, stoneC, glStep(0.90, big.idY));

  c = mix3(c, stone, bigExposed * 0.52);
  c = mix3(c, mix3(stoneA, stoneC, small.idX), smallExposed * 0.22);
  c = mix3(c, mix3(stoneB, stoneA, grit.idX), gritM * 0.14);
  h += bigExposed * 0.15 * (0.6 + 0.6 * big.idX) + smallExposed * 0.065 + gritM * 0.022;
  rough += bigExposed * (0.10 - 0.22 * big.idX) + smallExposed * (0.06 - 0.14 * small.idX);

  const voidM = glSmoothstep(0.50, 0.85, big.f1) * glSmoothstep(0.28, 0.6, small.f1);
  h -= voidM * 0.10;
  ao -= voidM * 0.14;

  const lane = Math.abs(glFract(uv[0] * 1.0 + 0.25) - 0.5) * 2.0;
  const polish = (1.0 - glSmoothstep(0.10, 0.62, lane)) *
    glSmoothstep(0.25, 0.65, owFbm01([p[0] * 0.7, p[1] * 5.0], [P[0], P[1] * 5.0], 4, 0.5));
  rough -= polish * 0.16;
  h -= polish * 0.012;
  c = mix3(c, add3(scale3(c, 0.78), owSRGB([0.045, 0.045, 0.048])), polish * 0.45);

  const rep = owWorley(owWarp(addS2(scale2(p, 0.5), 13.0), scale2(P, 0.5), 1.6, 3), scale2(P, 0.5), 0.9);
  const inPatch = glStep(0.72, rep.idY);
  const patchEdge = (1.0 - glSmoothstep(0.0, 0.06, rep.f2 - rep.f1)) * inPatch;
  c = mix3(c, scale3(cFresh, 0.85 + 0.35 * fine), inPatch * 0.20);
  rough = glMix(rough, 0.84, inPatch * 0.22);
  h -= patchEdge * 0.07;
  ao -= patchEdge * 0.20;
  c = mix3(c, scale3(cFresh, 0.5), patchEdge * 0.35);
  const tar = patchEdge * glSmoothstep(0.4, 0.7, owFbm01(scale2(p, 6.0), scale2(P, 6.0), 3, 0.5));
  rough -= tar * 0.35;
  c = mix3(c, owSRGB([0.055, 0.055, 0.058]), tar * 0.7);

  const gator = owCracks(scale2(p, 3.4), scale2(P, 3.4), 0.9, 0.032, 0.56);
  const thermal = owCracks(addS2(scale2(p, 0.9), 41.0), scale2(P, 0.9), 0.75, 0.05, 0.70);
  const crack = glClamp(gator + thermal, 0.0, 1.0);
  h -= crack * 0.16;
  ao -= crack * 0.30;
  c = mix3(c, owSRGB([0.045, 0.043, 0.042]), crack * 0.85);
  rough += crack * 0.12;

  const oil = glSmoothstep(0.68, 0.90, owFbm01(owWarp(addS2(scale2(p, 1.8), 31.0), scale2(P, 1.8), 0.9, 3), scale2(P, 1.8), 4, 0.55));
  c = mix3(c, owSRGB([0.045, 0.043, 0.046]), oil * 0.6);
  rough -= oil * 0.16;

  const dust = glSmoothstep(0.55, 0.30, h) * glSmoothstep(0.35, 0.75, macro);
  c = mix3(c, owSRGB([0.420, 0.390, 0.340]), dust * 0.35);
  rough += dust * 0.10;

  return {
    alb: c.map((x) => glClamp(x, 0.02, 0.75)),
    h: glClamp(h, 0.0, 1.0),
    rough: glClamp(rough, 0.44, 0.99),
    metal,
    ao: glClamp(ao, 0.68, 1.0),
  };
}

/** SAND owSurface, surfaces-ground.js:122-178. */
function sandSurface(uv, uSeed) {
  const P = [8.0, 8.0];
  const p = addS2(mul2(uv, P), uSeed * 8.2);

  const rp = owShear(scale2(p, 1.0), 1.0, 1.0);
  const warp = owFbm(scale2(p, 0.9), scale2(P, 0.9), 3, 0.55);
  let ripple = Math.sin((rp[1] * 1.0 + warp * 0.55) * 6.28318);
  ripple = ripple * 0.5 + 0.5;
  ripple = Math.pow(ripple, 1.7) * 0.75 + ripple * 0.25;
  const rippleAmp = glSmoothstep(0.20, 0.70, owFbm01(scale2(p, 0.7), scale2(P, 0.7), 3, 0.6));
  const secondary = Math.sin((p[1] * 3.0 + p[0] * 1.0 + warp * 0.8) * 6.28318) * 0.5 + 0.5;

  const dune = owFbm01(scale2(p, 0.5), scale2(P, 0.5), 4, 0.6);
  const mid = owFbm01(scale2(p, 5.0), scale2(P, 5.0), 5, 0.5);
  const grain = owFbm01(scale2(p, 18.0), scale2(P, 18.0), 4, 0.55);
  const gcell = owWorley(scale2(p, 24.0), scale2(P, 24.0), 1.0);

  let h = 0.50 + (dune - 0.5) * 0.16 + (mid - 0.5) * 0.05
    + (ripple - 0.5) * 0.26 * rippleAmp + (secondary - 0.5) * 0.06 * rippleAmp
    + (grain - 0.5) * 0.018;

  const cLight = owSRGB([0.760, 0.660, 0.480]);
  const cMid = owSRGB([0.610, 0.510, 0.360]);
  const cDamp = owSRGB([0.360, 0.290, 0.205]);
  let c = mix3(cMid, cLight, glSmoothstep(0.3, 0.8, dune));
  c = mix3(c, cDamp, glSmoothstep(0.62, 0.28, h) * 0.55);
  c = mix3(c, scale3(cLight, 1.06), glSmoothstep(0.45, 0.85, ripple) * rippleAmp * 0.35);
  c = mix3(c, scale3(cMid, 0.88), glSmoothstep(0.45, 0.10, ripple) * rippleAmp * 0.30);
  c = scale3(c, 0.90 + 0.18 * grain);
  c = addS3(c, glSmoothstep(0.22, 0.0, gcell.f1) * glStep(0.86, gcell.idX) * 0.10);

  let rough = 0.90 + (grain - 0.5) * 0.10 - glSmoothstep(0.6, 0.3, h) * 0.12;
  const metal = 0.0;
  let ao = 1.0 - glSmoothstep(0.55, 0.25, h) * 0.10;

  const peb = owWorley(scale2(p, 18.0), scale2(P, 18.0), 1.0);
  const pebble = glSmoothstep(0.30, 0.10, peb.f1) * glStep(0.80, peb.idY);
  const pcol = mix3(owSRGB([0.400, 0.370, 0.330]), owSRGB([0.690, 0.660, 0.620]), peb.idX);
  c = mix3(c, pcol, pebble * 0.85);
  h += pebble * 0.05;
  rough = glMix(rough, 0.55 + 0.25 * peb.idX, pebble * 0.8);
  ao -= glSmoothstep(0.40, 0.30, peb.f1) * glStep(0.80, peb.idY) * 0.08;

  const streak = glSmoothstep(0.62, 0.88, owFbm01(owShear(scale2(p, 2.5), 2.0, 4.0), owShearPer(scale2(P, 2.5), 4.0), 4, 0.5));
  c = mix3(c, scale3(cDamp, 1.1), streak * 0.22);

  return {
    alb: c.map((x) => glClamp(x, 0.02, 0.82)),
    h: glClamp(h, 0.0, 1.0),
    rough: glClamp(rough, 0.35, 0.99),
    metal,
    ao: glClamp(ao, 0.80, 1.0),
  };
}

/** DIRT owSurface, surfaces-ground.js:182-247. */
function dirtSurface(uv, uSeed) {
  const P = [8.0, 8.0];
  const p = addS2(mul2(uv, P), uSeed * 3.4);

  const macro = owFbm01(scale2(p, 0.6), scale2(P, 0.6), 4, 0.62);
  const clump = owBillow(owWarp(scale2(p, 3.0), scale2(P, 3.0), 0.6, 3), scale2(P, 3.0), 5, 0.55);
  const fine = owFbm01(scale2(p, 14.0), scale2(P, 14.0), 4, 0.5);
  const micro = owFbm01(scale2(p, 22.0), scale2(P, 22.0), 3, 0.5);

  const cDry = owSRGB([0.430, 0.350, 0.255]);
  const cWet = owSRGB([0.185, 0.140, 0.100]);
  const cPale = owSRGB([0.560, 0.490, 0.385]);
  let c = mix3(cDry, cPale, glSmoothstep(0.45, 0.9, macro));
  c = mix3(c, cWet, glSmoothstep(0.55, 0.15, macro) * 0.8);
  c = scale3(c, 0.94 + 0.11 * fine);
  c = scale3(c, 0.975 + 0.05 * micro);

  let h = 0.55 + (macro - 0.5) * 0.14 + (clump - 0.5) * 0.16 + (fine - 0.5) * 0.075;
  let rough = 0.88 + (fine - 0.5) * 0.14 + (micro - 0.5) * 0.10;
  const metal = 0.0;
  let ao = 1.0;

  const pan = glSmoothstep(0.35, 0.65, macro);
  const mud = owCracks(scale2(p, 2.4), scale2(P, 2.4), 0.85, 0.045, 0.35) * pan;
  h -= mud * 0.16;
  ao -= mud * 0.32;
  c = mix3(c, scale3(cWet, 0.7), mud * 0.75);
  const plateLift = glSmoothstep(0.10, 0.0, mud) * pan;
  h += plateLift * 0.01;

  const st = owWorley(scale2(p, 11.0), scale2(P, 11.0), 1.0);
  const stone = glSmoothstep(0.30, 0.11, st.f1) * glStep(0.62, st.idY);
  const scol = mix3(owSRGB([0.330, 0.315, 0.295]), owSRGB([0.600, 0.575, 0.540]), st.idX);
  c = mix3(c, scol, stone * 0.6);
  h += stone * 0.085;
  rough = glMix(rough, 0.52 + 0.28 * st.idX, stone * 0.8);
  ao -= glSmoothstep(0.36, 0.28, st.f1) * glStep(0.62, st.idY) * 0.10;

  const grit = owWorley(scale2(p, 22.0), scale2(P, 22.0), 1.0);
  const gritM = glSmoothstep(0.26, 0.08, grit.f1) * glStep(0.55, grit.idY);
  c = mix3(c, mix3(scol, cPale, grit.idX), gritM * 0.4);
  h += gritM * 0.015;

  let litter = glSmoothstep(0.70, 0.86, owFbm01(owShear(scale2(p, 8.0), 1.0, 5.0), owShearPer(scale2(P, 8.0), 5.0), 4, 0.5));
  litter *= glSmoothstep(0.4, 0.8, macro);
  c = mix3(c, owSRGB([0.330, 0.290, 0.160]), litter * 0.5);
  h += litter * 0.012;
  rough += litter * 0.05;

  const moss = glSmoothstep(0.74, 0.92, owFbm01(addS2(scale2(p, 4.5), 19.0), scale2(P, 4.5), 5, 0.6)) * glSmoothstep(0.5, 0.1, macro);
  c = mix3(c, owSRGB([0.150, 0.185, 0.105]), moss * 0.65);

  const cavity = 1.0 - glSmoothstep(0.40, 0.70, h);
  ao -= cavity * 0.14;

  return {
    alb: c.map((x) => glClamp(x, 0.02, 0.72)),
    h: glClamp(h, 0.0, 1.0),
    rough: glClamp(rough, 0.45, 0.99),
    metal,
    ao: glClamp(ao, 0.72, 1.0),
  };
}

/** GRAVEL owSurface, surfaces-ground.js:251-365. */
function gravelSurface(uv, uSeed) {
  const P = [8.0, 8.0];
  const p = addS2(mul2(uv, P), uSeed * 2.7);

  const bed = owFbm01(scale2(p, 1.3), scale2(P, 1.3), 4, 0.55);

  const a = owWorley(scale2(p, 5.5), scale2(P, 5.5), 1.0);
  const b = owWorley(addS2(scale2(p, 10.0), 5.0), scale2(P, 10.0), 1.0);
  const cSm = owWorley(addS2(scale2(p, 21.0), 11.0), scale2(P, 21.0), 1.0);

  const sA = glSmoothstep(0.36, 0.10, a.f1) * glStep(0.44, a.idY);
  const sB = glSmoothstep(0.30, 0.08, b.f1) * glStep(0.62, b.idY);
  const sC = glSmoothstep(0.24, 0.06, cSm.f1) * glStep(0.74, cSm.idY);

  const ha = sA * 0.15 * (0.5 + a.idX);
  const hb = sB * 0.09 * (0.5 + b.idX);
  const hc = sC * 0.025;
  let h = 0.54 + (bed - 0.5) * 0.11 + Math.max(Math.max(ha, hb), hc) + 0.22 * (ha + hb);

  const s1 = owSRGB([0.372, 0.356, 0.332]);
  const s2 = owSRGB([0.232, 0.220, 0.208]);
  const s3 = owSRGB([0.462, 0.438, 0.400]);
  const s4 = owSRGB([0.352, 0.276, 0.220]);
  let top = mix3(s1, s2, a.idX);
  top = mix3(top, s3, glStep(0.78, a.idY));
  top = mix3(top, s4, glStep(0.90, b.idY) * 0.7);

  const cBed = owSRGB([0.362, 0.336, 0.294]);
  let c = mix3(cBed, top, glClamp(sA * 0.70 + sB * 0.42 + sC * 0.16, 0.0, 1.0));
  const grain = owFbm01(scale2(p, 13.0), scale2(P, 13.0), 4, 0.5);
  c = scale3(c, 0.965 + 0.07 * grain);

  let rough = 0.82 + 0.05 * grain + (1.0 - glClamp(sA + sB, 0.0, 1.0)) * 0.06
    - sA * (0.06 + 0.07 * a.idX) - sB * 0.05 * b.idX;
  const metal = 0.0;
  let ao = glMix(0.87, 1.0, glSmoothstep(0.42, 0.66, h));

  const dust = 1.0 - glSmoothstep(0.44, 0.62, h);
  c = mix3(c, scale3(cBed, 1.04), dust * 0.5);
  rough += dust * 0.08;
  ao = glMix(ao, 1.0, dust * 0.3);

  const drift = owFbm01(owWarp(addS2(scale2(p, 0.9), 17.0), scale2(P, 0.9), 0.8, 3), scale2(P, 0.9), 4, 0.6);
  h += (drift - 0.5) * 0.10;
  c = scale3(c, 0.86 + 0.28 * drift);
  rough += (drift - 0.5) * 0.10;
  c = mix3(c, scale3(cBed, 0.92 + 0.22 * drift), glSmoothstep(0.55, 0.88, drift) * 0.72);

  const scuff = owFbm01(owShear(scale2(p, 2.2), 0.0, 6.0), owShearPer(scale2(P, 2.2), 6.0), 4, 0.5);
  c = scale3(c, 1.0 - glSmoothstep(0.55, 0.92, scuff) * 0.10);
  rough -= glSmoothstep(0.6, 0.95, scuff) * 0.08;

  return {
    alb: c.map((x) => glClamp(x, 0.02, 0.78)),
    h: glClamp(h, 0.0, 1.0),
    rough: glClamp(rough, 0.62, 0.99),
    metal,
    ao: glClamp(ao, 0.72, 1.0),
  };
}

/* ------------------------------------------------------------------ */
/* Build the golden.                                                    */
/* ------------------------------------------------------------------ */

// Fixed uv grid — corners, mid-edges, and interior points, shared by every
// generator so the golden file stays small and readable.
const UVS = [
  [0.0, 0.0], [0.13, 0.77], [0.42, 0.09], [0.91, 0.36], [1.0, 1.0],
  [0.25, 0.5], [0.5, 0.25], [0.6, 0.8], [0.05, 0.95], [0.33, 0.33],
];

// Same seeds as the library entries in `src/materials/mod.rs::LIBRARY`
// (`apps/claude-of-duty/src/materials/mod.rs`) — asphalt=71, sand=91,
// dirt=13, gravel=57 — so the golden doubles as a real-world contract check,
// not just an arbitrary probe.
const out = {
  asphalt: { seed: 71.0, samples: UVS.map((uv) => ({ uv, out: asphaltSurface(uv, 71.0) })) },
  sand: { seed: 91.0, samples: UVS.map((uv) => ({ uv, out: sandSurface(uv, 91.0) })) },
  dirt: { seed: 13.0, samples: UVS.map((uv) => ({ uv, out: dirtSurface(uv, 13.0) })) },
  gravel: { seed: 57.0, samples: UVS.map((uv) => ({ uv, out: gravelSurface(uv, 57.0) })) },
};

// A dense grid for gravel alone, to pin the documented 0.87..1.0 AO band
// against the real transcription rather than only the Rust port's own
// re-derivation of that range.
{
  const dense = [];
  for (let iy = 0; iy <= 16; iy++) {
    for (let ix = 0; ix <= 16; ix++) {
      dense.push([ix / 16, iy / 16]);
    }
  }
  out.gravelDenseAo = dense.map((uv) => ({ uv, ao: gravelSurface(uv, 57.0).ao }));
}

process.stdout.write(JSON.stringify(out, null, 1));
