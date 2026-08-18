/**
 * Golden capture for the Claude-of-Duty organic-surface generators
 * (wood/fabric/burlap/foliage/rubber/glass).
 *
 * `src/materials/glsl/surfaces-organic.js` and the `src/materials/glsl/noise.js`
 * it builds on are both GLSL held in JavaScript template-string literals —
 * shader source that never ran anywhere but a browser GPU. There is no
 * JavaScript function to `import` and call as a genuine oracle. So, same
 * discipline as `tests/materials_surfaces_ground/capture.mjs`: every function
 * below is a hand-transcription of the GLSL into plain JS doubles,
 * independent of (but line-referenced against, the same as) this crate's Rust
 * transcription in `apps/claude-of-duty/src/materials/surfaces/organic.rs`.
 * Pinning the Rust port against this catches drift between the Rust
 * transcription and a careful reading of the GLSL — it cannot catch a
 * mistake both transcriptions share, which is why this file states the
 * caveat instead of pretending to be an oracle.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 */

/* ------------------------------------------------------------------ */
/* GLSL scalar/vec2/vec3/vec4 primitives — bare GLSL builtins every      */
/* function below uses; noise.js has no owMix/owClamp/owStep of its own. */
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
const add2 = (a, b) => [a[0] + b[0], a[1] + b[1]];
const sub2 = (a, b) => [a[0] - b[0], a[1] - b[1]];
const mul2 = (a, b) => [a[0] * b[0], a[1] * b[1]];
const scale2 = (a, s) => [a[0] * s, a[1] * s];
const addS2 = (a, s) => [a[0] + s, a[1] + s];
const dot2 = (a, b) => a[0] * b[0] + a[1] * b[1];
const floor2 = (a) => [Math.floor(a[0]), Math.floor(a[1])];
const fract2 = (a) => [glFract(a[0]), glFract(a[1])];
const mod2 = (a, per) => [glMod(a[0], per[0]), glMod(a[1], per[1])];
const length2 = (a) => Math.sqrt(dot2(a, a));

/* vec3 helpers */
const scale3 = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
const addS3 = (a, s) => [a[0] + s, a[1] + s, a[2] + s];
const mul3v = (a, b) => [a[0] * b[0], a[1] * b[1], a[2] * b[2]];
const dot3 = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const fract3 = (a) => [glFract(a[0]), glFract(a[1]), glFract(a[2])];
const mix3 = (a, b, t) => [glMix(a[0], b[0], t), glMix(a[1], b[1], t), glMix(a[2], b[2], t)];
const add3 = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];

/* vec4 helpers — just enough for owHash42 */
const v4 = (x, y, z, w) => [x, y, z, w];
const mul4 = (a, b) => [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]];
const addS4 = (a, s) => [a[0] + s, a[1] + s, a[2] + s, a[3] + s];
const dot4 = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
const fract4 = (a) => a.map(glFract);

/* ------------------------------------------------------------------ */
/* noise.js, transcribed function-for-function.                         */
/* ------------------------------------------------------------------ */

/** owHash11, noise.js:15-20. */
function owHash11(p) {
  let x = glFract(p * 0.1031);
  x *= x + 33.33;
  x *= x + x;
  return glFract(x);
}

/** owHash12, noise.js:21-25. */
function owHash12(p) {
  let p3 = fract3(scale3([p[0], p[1], p[0]], 0.1031));
  const yzx = addS3([p3[1], p3[2], p3[0]], 33.33);
  const d = dot3(p3, yzx);
  p3 = addS3(p3, d);
  return glFract((p3[0] + p3[1]) * p3[2]);
}

/** owHash22, noise.js:26-30. */
function owHash22(p) {
  let p3 = fract3(mul3v([p[0], p[1], p[0]], [0.1031, 0.1030, 0.0973]));
  const yzx = addS3([p3[1], p3[2], p3[0]], 33.33);
  const d = dot3(p3, yzx);
  p3 = addS3(p3, d);
  const sum = [p3[0] + p3[1], p3[0] + p3[2]];
  const zy = [p3[2], p3[1]];
  return fract2(mul2(sum, zy));
}

/** owHash42, noise.js:36-40. Returns {x,y,z,w}. */
function owHash42(p) {
  let p4 = fract4(mul4(v4(p[0], p[1], p[0], p[1]), v4(0.1031, 0.103, 0.0973, 0.1099)));
  const wzxy = addS4(v4(p4[3], p4[2], p4[0], p4[1]), 33.33);
  const d = dot4(p4, wzxy);
  p4 = addS4(p4, d);
  const sum = v4(p4[0] + p4[1], p4[0] + p4[2], p4[1] + p4[2], p4[2] + p4[3]);
  const zywx = v4(p4[2], p4[1], p4[3], p4[0]);
  const r = fract4(mul4(sum, zywx));
  return { x: r[0], y: r[1], z: r[2], w: r[3] };
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

/** owSRGB, noise.js:197-199. See `tests/materials_surfaces_ground/capture.mjs`
 * for why this is `step(edge=ci, x=0.04045)`, not the other operand order. */
function owSRGB(c) {
  const decode = (ci) => {
    const powPart = Math.pow((ci + 0.055) / 1.055, 2.4);
    const linPart = ci / 12.92;
    const s = glStep(ci, 0.04045);
    return glMix(powPart, linPart, s);
  };
  return [decode(c[0]), decode(c[1]), decode(c[2])];
}

/** owRot, noise.js:192-195. Column-major `mat2(c,-s,s,c) * p`: a clockwise
 * rotation for positive `a`, preserved exactly (see the Rust port's doc). */
function owRot(p, a) {
  const s = Math.sin(a), c = Math.cos(a);
  return [c * p[0] + s * p[1], c * p[1] - s * p[0]];
}

/** owShear, noise.js:204-206. */
function owShear(p, k, stretch) {
  return [p[0] + p[1] * k, p[1] * stretch];
}
/** owShearPer, noise.js:207-209. */
function owShearPer(per, stretch) {
  return [per[0], per[1] * stretch];
}

/** owScratches, noise.js:212-217. */
function owScratches(p, per, stretch, k, thin) {
  const q = owShear(p, k, stretch);
  const qper = owShearPer(per, stretch);
  const n = owFbm01(q, qper, 4, 0.5);
  return glSmoothstep(thin, thin + 0.06, n) * (1.0 - glSmoothstep(thin + 0.06, thin + 0.2, n));
}

/** `new THREE.Color(hex)` under default r180 color management: the hex is
 * decoded as sRGB into the linear working color space — the same transform
 * `owSRGB` performs on every other hard-coded albedo constant in this file.
 * Used only for `fabric`'s `uTintA`/`uTintB` uniforms. */
function hexToLinear(hex) {
  return owSRGB([((hex >> 16) & 255) / 255, ((hex >> 8) & 255) / 255, (hex & 255) / 255]);
}

/* ------------------------------------------------------------------ */
/* surfaces-organic.js — the six owSurface bodies, transcribed line-for- */
/* line against `C:/dev/Claude-of-Duty/src/materials/glsl/               */
/* surfaces-organic.js`.                                                 */
/* ------------------------------------------------------------------ */

/** WOOD owSurface, surfaces-organic.js:9-119. */
function woodSurface(uv, uSeed) {
  const P = [8.0, 8.0];
  const PLANKS = 5.0;
  const p = addS2(mul2(uv, P), uSeed * 12.9);

  const rowF = uv[1] * PLANKS;
  const row = Math.floor(rowF);
  const rf = glFract(rowF);
  const stagger = owHash11(row + uSeed * 2.0);
  const lenF = uv[0] * 2.0 + stagger;
  const board = Math.floor(lenF);
  const lf = glFract(lenF);
  const rnd = owHash42(addS2([board, row], uSeed));

  const GY = 0.035, GX = 0.010;
  const ey = Math.min(glSmoothstep(0.0, GY, rf), glSmoothstep(0.0, GY, 1.0 - rf));
  const ex = Math.min(glSmoothstep(0.0, GX, lf), glSmoothstep(0.0, GX, 1.0 - lf));
  const face = Math.min(ex, ey);

  const gp = [lf * 2.0 + rnd.x * 13.0, rf + rnd.y * 7.0];
  const GP = [16.0, 8.0];
  const warp = owFbm([gp[0] * 3.0, gp[1] * 12.0], [GP[0] * 3.0, GP[1] * 12.0], 4, 0.55);
  let ringCoord = gp[1] * (14.0 + rnd.z * 12.0) + warp * 2.2 + rnd.w * 5.0;

  const knotP = [0.25 + rnd.x * 0.5, 0.35 + rnd.y * 0.3];
  const kd = length2(mul2(sub2([lf, rf], knotP), [2.2, 1.0]));
  const hasKnot = glStep(0.68, rnd.z);
  const knotPull = hasKnot * Math.exp(-kd * 9.0);
  ringCoord = glMix(ringCoord, kd * 42.0, glClamp(knotPull * 1.6, 0.0, 1.0));

  const rings = glFract(ringCoord);
  const ringDark = glSmoothstep(0.42, 0.5, rings) * (1.0 - glSmoothstep(0.5, 0.62, rings));
  const latewood = glSmoothstep(0.30, 0.52, rings);

  const fibre = owFbm01(owShear(scale2(p, 6.0), 0.0, 40.0), owShearPer(scale2(P, 6.0), 40.0), 4, 0.5);
  const micro = owFbm01(scale2(p, 22.0), scale2(P, 22.0), 3, 0.5);

  const wLight = owSRGB([0.505, 0.408, 0.290]);
  const wMid = owSRGB([0.362, 0.272, 0.180]);
  const wDark = owSRGB([0.205, 0.142, 0.092]);
  const wGrey = owSRGB([0.372, 0.355, 0.328]);
  let c = mix3(wLight, wMid, rnd.w * 0.8 + latewood * 0.5);
  c = mix3(c, wDark, ringDark * 0.65);
  c = scale3(c, 0.90 + 0.18 * fibre);
  c = mix3(c, scale3(wDark, 0.7), glClamp(knotPull * 2.2, 0.0, 1.0) * 0.8);

  const weather = glSmoothstep(0.20, 0.85, owFbm01(scale2(p, 0.8), scale2(P, 0.8), 3, 0.6)) * (0.4 + 0.6 * rnd.x);
  c = mix3(c, wGrey, weather * 0.68);

  let faceH = 0.74 - ringDark * 0.02 - latewood * 0.012 + (fibre - 0.5) * 0.03 + (micro - 0.5) * 0.008;
  faceH += (rnd.y - 0.5) * 0.035;
  faceH -= glClamp(knotPull * 1.5, 0.0, 1.0) * 0.03;

  const split = owScratches(scale2(p, 2.0), scale2(P, 2.0), 30.0, 0.0, 0.66) * weather;
  faceH -= split * 0.10;
  c = mix3(c, scale3(wDark, 0.45), split * 0.7);

  const saw = owFbm01(mul2(owShear(scale2(p, 3.0), 0.0, 1.0), [30.0, 1.0]), [P[0] * 90.0, P[1] * 3.0], 3, 0.5);
  faceH += (saw - 0.5) * 0.012;

  const edgeD = Math.min(Math.min(rf, 1.0 - rf) / GY, Math.min(lf, 1.0 - lf) / GX);
  const bevel = 1.0 - glSmoothstep(0.0, 2.4, edgeD);
  faceH -= bevel * 0.035;
  c = scale3(c, 1.0 - bevel * 0.10);
  c = mix3(c, scale3(wLight, 1.15), bevel * glSmoothstep(0.5, 0.9, owFbm01(scale2(p, 20.0), scale2(P, 20.0), 3, 0.5)) * 0.35);

  const m = glSmoothstep(0.05, 0.7, face);
  let h = glMix(0.44, faceH, m);
  c = mix3(scale3(wDark, 0.25), c, m);
  let rough = glMix(0.95, 0.62 + 0.22 * fibre + weather * 0.20 + split * 0.15, m);
  let ao = glMix(0.25, 1.0, glSmoothstep(0.0, 0.5, face)) - bevel * 0.12 * m;
  let metal = 0.0;

  // Source quirk, omitted (dead `nf`/first `nd`) — see the Rust port's doc.
  const nd = length2(mul2([glFract(lf * 3.0 + 0.5) - 0.5, rf - 0.22], [1.4, 1.0]));
  const nail = glSmoothstep(0.055, 0.030, nd) * m * glStep(0.3, rnd.w);
  h -= nail * 0.02;
  c = mix3(c, owSRGB([0.230, 0.200, 0.170]), nail * 0.85);
  rough = glMix(rough, 0.55, nail);
  metal = glMix(metal, 0.85, nail * 0.7);
  ao -= nail * 0.25;
  const weep = glSmoothstep(0.11, 0.05, nd) * glStep(0.3, rnd.w) * glSmoothstep(0.0, 0.6, rf - 0.22) * m;
  c = mix3(c, owSRGB([0.330, 0.185, 0.095]), glClamp(weep, 0.0, 1.0) * 0.4);

  const cavity = 1.0 - glSmoothstep(0.55, 0.78, h);
  c = mix3(c, owSRGB([0.120, 0.106, 0.088]), cavity * 0.45);
  const soil = glSmoothstep(0.40, 0.88, owFbm01(owWarp(addS2(scale2(p, 2.2), 5.0), scale2(P, 2.2), 0.9, 3), scale2(P, 2.2), 5, 0.6));
  c = mix3(c, owSRGB([0.185, 0.160, 0.128]), soil * 0.40);
  rough += soil * 0.08;

  return {
    alb: c.map((x) => glClamp(x, 0.02, 0.80)),
    h: glClamp(h, 0.0, 1.0),
    rough: glClamp(rough, 0.25, 0.99),
    metal,
    ao: glClamp(ao, 0.12, 1.0),
  };
}

/** FABRIC owSurface, surfaces-organic.js:123-198. */
function fabricSurface(uv, uSeed, tintA, tintB) {
  const P = [8.0, 8.0];
  const THREADS = 96.0;
  const p = addS2(mul2(uv, P), uSeed * 3.9);

  const t = scale2(uv, THREADS);
  const cell = floor2(t);
  const f = addS2(fract2(t), -0.5);
  const over = glMod(cell[0] + cell[1], 2.0);

  const warpProfile = Math.cos(f[0] * 3.14159);
  const weftProfile = Math.cos(f[1] * 3.14159);
  const top = glMix(warpProfile, weftProfile, over);
  const bot = glMix(weftProfile, warpProfile, over) * 0.45;
  const weave = Math.max(top, bot);
  const threadId = owHash12(addS2(cell, uSeed));

  const fuzz = owFbm01(scale2(p, 12.0), scale2(P, 12.0), 3, 0.55);
  const slub = owFbm01(scale2(p, 14.0), scale2(P, 14.0), 4, 0.5);
  const macro = owFbm01(scale2(p, 1.2), scale2(P, 1.2), 4, 0.6);

  let c = mix3(tintA, tintB, threadId * 0.6 + slub * 0.4);
  c = scale3(c, 0.865 + 0.215 * (weave * 0.5 + 0.5));
  c = scale3(c, 0.960 + 0.075 * fuzz);
  c = scale3(c, 0.90 + 0.20 * macro);

  let h = 0.55 + weave * 0.30 + (fuzz - 0.5) * 0.03 + (slub - 0.5) * 0.05;
  let rough = 0.86 + (1.0 - weave) * 0.08 + (fuzz - 0.5) * 0.06;
  const metal = 0.0;
  let ao = glMix(0.82, 1.0, glSmoothstep(-0.4, 0.9, weave));

  const foldC = uv[1] * 2.6 + uv[0] * 0.55 + owFbm01(scale2(p, 0.9), scale2(P, 0.9), 3, 0.62) * 2.2;
  const foldT = Math.abs(glFract(foldC) - 0.5) * 2.0;
  const crest = 1.0 - foldT;
  const foldR = owHash11(Math.floor(foldC) * 2.13 + uSeed);
  const fold = crest * crest * (0.55 + 0.75 * foldR);
  h += (fold - 0.30) * 0.115;
  c = scale3(c, 0.895 + 0.21 * fold);
  ao -= (1.0 - crest) * 0.14;
  const creaseLine = 1.0 - glSmoothstep(0.0, 0.10, foldT);
  rough -= creaseLine * 0.06;
  c = scale3(c, 1.0 + creaseLine * 0.05);

  const wearField = glSmoothstep(0.58, 0.82, owFbm01(owWarp(scale2(p, 2.0), scale2(P, 2.0), 0.8, 3), scale2(P, 2.0), 4, 0.55));
  c = mix3(c, addS3(scale3(c, 1.35), 0.02), wearField * 0.5);
  rough += wearField * 0.06;
  h -= wearField * 0.05;

  const pulled = owScratches(scale2(p, 3.0), scale2(P, 3.0), 18.0, 1.0, 0.68);
  h += pulled * 0.05;
  c = scale3(c, 1.0 - pulled * 0.10);

  const stain = glSmoothstep(0.55, 0.9, owFbm01(owWarp(addS2(scale2(p, 1.5), 7.0), scale2(P, 1.5), 1.0, 3), scale2(P, 1.5), 5, 0.6));
  c = mix3(c, add3(scale3(c, 0.42), owSRGB([0.09, 0.08, 0.06])), stain * 0.55);
  rough += stain * 0.05;

  const dust = glSmoothstep(0.4, 0.85, owFbm01(scale2(p, 6.0), scale2(P, 6.0), 4, 0.5));
  c = mix3(c, owSRGB([0.400, 0.375, 0.335]), dust * 0.14);

  return {
    alb: c.map((x) => glClamp(x, 0.02, 0.85)),
    h: glClamp(h, 0.0, 1.0),
    rough: glClamp(rough, 0.5, 0.99),
    metal,
    ao: glClamp(ao, 0.25, 1.0),
  };
}

/** BURLAP owSurface, surfaces-organic.js:202-256. */
function burlapSurface(uv, uSeed) {
  const P = [8.0, 8.0];
  const THREADS = 34.0;
  const p = addS2(mul2(uv, P), uSeed * 4.7);

  const t = scale2(uv, THREADS);
  const cell = floor2(t);
  const f = addS2(fract2(t), -0.5);
  const over = glMod(cell[0] + cell[1], 2.0);

  const twx = 0.62 + 0.30 * owHash12(addS2([cell[0], 0.0], uSeed));
  const twy = 0.62 + 0.30 * owHash12(addS2([0.0, cell[1]], uSeed * 1.7));
  const warpP = Math.cos(glClamp(f[0] / twx, -0.5, 0.5) * 3.14159);
  const weftP = Math.cos(glClamp(f[1] / twy, -0.5, 0.5) * 3.14159);
  const top = glMix(warpP, weftP, over);
  const bot = glMix(weftP, warpP, over) * 0.40;
  const weave = Math.max(top, bot);

  const fibre = owFbm01(owShear(scale2(p, 12.0), 0.0, 8.0), owShearPer(scale2(P, 12.0), 8.0), 3, 0.5);
  const macro = owFbm01(scale2(p, 1.0), scale2(P, 1.0), 4, 0.62);
  const dirt = owFbm01(owWarp(scale2(p, 2.5), scale2(P, 2.5), 0.8, 3), scale2(P, 2.5), 5, 0.55);

  const cJute = owSRGB([0.520, 0.430, 0.275]);
  const cPale = owSRGB([0.640, 0.560, 0.400]);
  const cSoil = owSRGB([0.230, 0.180, 0.120]);
  let c = mix3(cJute, cPale, owHash12(addS2(cell, 3.0)) * 0.5 + fibre * 0.15);
  c = scale3(c, 0.855 + 0.235 * (weave * 0.5 + 0.5));
  c = scale3(c, 0.90 + 0.18 * macro);
  c = mix3(c, cSoil, glSmoothstep(0.42, 0.85, dirt) * 0.60);

  let h = 0.50 + weave * 0.38 + (fibre - 0.5) * 0.05;
  let rough = 0.90 + (1.0 - weave) * 0.06;
  const metal = 0.0;
  let ao = glMix(0.74, 1.0, glSmoothstep(-0.4, 0.9, weave));

  const rot = glSmoothstep(0.55, 0.9, owFbm01(addS2(scale2(p, 0.7), 11.0), scale2(P, 0.7), 3, 0.6));
  c = mix3(c, scale3(cPale, 1.15), rot * 0.4);
  rough += rot * 0.05;

  const loose = owScratches(scale2(p, 4.0), scale2(P, 4.0), 10.0, 2.0, 0.70);
  h += loose * 0.06;
  c = mix3(c, cPale, loose * 0.3);

  const sand = glSmoothstep(0.5, 0.85, owFbm01(scale2(p, 12.0), scale2(P, 12.0), 4, 0.5)) * (1.0 - glSmoothstep(0.2, 0.7, weave));
  c = mix3(c, owSRGB([0.640, 0.545, 0.390]), sand * 0.45);

  return {
    alb: c.map((x) => glClamp(x, 0.02, 0.80)),
    h: glClamp(h, 0.0, 1.0),
    rough: glClamp(rough, 0.6, 0.99),
    metal,
    ao: glClamp(ao, 0.2, 1.0),
  };
}

/** FOLIAGE owSurface, surfaces-organic.js:260-327. `h` is the alpha-test
 * cutout mask (`bestCover`), not a height — see the Rust port's module doc. */
function foliageSurface(uv, uSeed) {
  const P = [8.0, 8.0];
  const CELLS = 5.0;
  const p = addS2(mul2(uv, P), uSeed * 5.9);

  const lp = scale2(uv, CELLS);
  const ip = floor2(lp);
  const fp = fract2(lp);

  let bestCover = 0.0;
  let bestDepth = -1.0;
  let bestCol = [0.0, 0.0, 0.0];
  let bestVein = 0.0;

  for (let y = -1; y <= 1; y++) {
    for (let x = -1; x <= 1; x++) {
      const g = [x, y];
      const cell = mod2(add2(ip, g), [CELLS, CELLS]);
      const r = owHash42(addS2(cell, uSeed * 2.0));
      const r2 = owHash42(addS2(addS2(scale2(cell, 1.7), 9.0), uSeed));
      const centre = sub2(add2(addS2(g, 0.15), scale2([r.x, r.y], 0.7)), fp);
      const ang = r.z * 6.28318;
      const q = owRot(centre, ang);
      const s = [0.30 + r.w * 0.16, 0.13 + r2.x * 0.07];
      const e = [q[0] / s[0], q[1] / s[1]];
      const d = length2(e);
      const pinch = 1.0 - 0.55 * Math.abs(e[0]) * 0.5;
      // Source quirk, omitted (dead un-serrated `cover`) — see the Rust port's doc.
      const serr = Math.sin(Math.atan2(e[1], e[0]) * 26.0) * 0.03;
      const cover = glSmoothstep(1.02 + serr, 0.88 + serr, d / Math.max(pinch, 0.3));
      if (cover > 0.01) {
        const depth = r2.y;
        if (depth > bestDepth) {
          let vein = 1.0 - glSmoothstep(0.0, 0.05, Math.abs(e[1] * s[1]));
          const sideV = glSmoothstep(0.75, 1.0, Math.abs(glFract(e[0] * 5.0 + e[1] * 2.0) * 2.0 - 1.0));
          vein = glClamp(vein + sideV * 0.45 * cover, 0.0, 1.0);
          const cYoung = owSRGB([0.180, 0.330, 0.090]);
          const cOld = owSRGB([0.095, 0.185, 0.060]);
          const cDry = owSRGB([0.390, 0.320, 0.110]);
          let lc = mix3(cYoung, cOld, r2.z);
          lc = mix3(lc, cDry, glSmoothstep(0.55, 1.0, r2.w) * 0.8);
          const spots = owFbm01(scale2(p, 22.0), scale2(P, 22.0), 3, 0.5);
          lc = scale3(lc, 0.85 + 0.30 * spots);
          lc = mix3(lc, scale3(cDry, 0.7), glSmoothstep(0.78, 0.95, spots) * 0.5);
          lc = mix3(lc, scale3(lc, 1.35), vein * 0.5);
          bestDepth = depth;
          bestCover = cover;
          bestCol = lc;
          bestVein = vein;
          // `bestH` is computed in the source here but never read — dead,
          // see the Rust port's doc. Omitted.
        }
      }
    }
  }

  const fine = owFbm01(scale2(p, 12.0), scale2(P, 12.0), 3, 0.5);
  return {
    alb: scale3(bestCol, 0.955 + 0.085 * fine).map((x) => glClamp(x, 0.02, 0.7)),
    h: bestCover,
    rough: glClamp(0.62 + (1.0 - bestVein) * 0.14 + (fine - 0.5) * 0.10, 0.35, 0.95),
    metal: 0.0,
    ao: glClamp(0.55 + bestDepth * 0.45, 0.3, 1.0),
  };
}

/** RUBBER owSurface, surfaces-organic.js:331-379. */
function rubberSurface(uv, uSeed) {
  const P = [8.0, 8.0];
  const p = addS2(mul2(uv, P), uSeed * 9.6);

  const pb = owWorley(scale2(p, 12.0), scale2(P, 12.0), 1.0);
  const pebble = glSmoothstep(0.42, 0.10, pb.f1);
  const fine = owFbm01(scale2(p, 12.0), scale2(P, 12.0), 3, 0.5);
  const macro = owFbm01(scale2(p, 1.5), scale2(P, 1.5), 4, 0.6);

  let h = 0.60 + pebble * 0.10 + (fine - 0.5) * 0.02 + (macro - 0.5) * 0.03;
  let c = owSRGB([0.200, 0.200, 0.206]);
  c = scale3(c, 0.85 + 0.25 * (pebble * 0.5 + 0.5));
  c = scale3(c, 0.94 + 0.10 * fine);

  let rough = 0.88 - pebble * 0.06 + (fine - 0.5) * 0.08;
  let ao = glMix(0.6, 1.0, pebble * 0.5 + 0.5);

  const seam = 1.0 - glSmoothstep(0.0, 0.012, Math.abs(glFract(uv[1] * 2.0 + 0.5) - 0.5));
  h += seam * 0.03;
  c = scale3(c, 1.0 + seam * 0.35);
  rough -= seam * 0.10;

  const scuff = glSmoothstep(0.55, 0.88, owFbm01(owWarp(scale2(p, 3.0), scale2(P, 3.0), 0.8, 3), scale2(P, 3.0), 4, 0.55));
  c = mix3(c, owSRGB([0.220, 0.218, 0.212]), scuff * 0.45);
  rough += scuff * 0.06;
  h -= scuff * 0.015;

  const crack = owCracks(scale2(p, 7.0), scale2(P, 7.0), 0.9, 0.028, 0.62);
  h -= crack * 0.06;
  c = scale3(c, 1.0 - crack * 0.35);
  ao -= crack * 0.35;

  const dust = glSmoothstep(0.5, 0.9, owFbm01(scale2(p, 8.0), scale2(P, 8.0), 4, 0.5));
  c = mix3(c, owSRGB([0.290, 0.275, 0.250]), dust * 0.16);

  return {
    alb: c.map((x) => glClamp(x, 0.02, 0.35)),
    h: glClamp(h, 0.0, 1.0),
    rough: glClamp(rough, 0.55, 0.99),
    metal: 0.0,
    ao: glClamp(ao, 0.3, 1.0),
  };
}

/** GLASS owSurface, surfaces-organic.js:383-414. */
function glassSurface(uv, uSeed) {
  const P = [8.0, 8.0];
  const p = addS2(mul2(uv, P), uSeed * 2.2);

  const smear = owFbm01(owShear(scale2(p, 3.0), 1.0, 6.0), owShearPer(scale2(P, 3.0), 6.0), 4, 0.5);
  const dustF = owFbm01(scale2(p, 5.0), scale2(P, 5.0), 5, 0.55);
  const spots = owWorley(scale2(p, 24.0), scale2(P, 24.0), 1.0).f1;
  const fine = owFbm01(scale2(p, 12.0), scale2(P, 12.0), 3, 0.5);

  let c = owSRGB([0.045, 0.050, 0.052]);

  const dirty = glSmoothstep(0.45, 0.85, dustF);
  c = mix3(c, owSRGB([0.300, 0.290, 0.265]), dirty * 0.35);

  let rough = 0.045 + smear * 0.10 * glSmoothstep(0.3, 0.9, dustF) + dirty * 0.22;
  rough += glSmoothstep(0.30, 0.05, spots) * 0.25;
  rough += (fine - 0.5) * 0.02;

  const scr = owScratches(scale2(p, 2.0), scale2(P, 2.0), 24.0, 1.0, 0.70);
  rough += scr * 0.25;
  c = addS3(c, scr * 0.02);

  const h = 0.5 + (smear - 0.5) * 0.004;
  const ao = 1.0 - dirty * 0.1;

  return {
    alb: c.map((x) => glClamp(x, 0.02, 0.5)),
    h: glClamp(h, 0.0, 1.0),
    rough: glClamp(rough, 0.02, 0.7),
    metal: 0.0,
    ao,
  };
}

/* ------------------------------------------------------------------ */
/* Build the golden.                                                    */
/* ------------------------------------------------------------------ */

// Fixed uv grid — corners, mid-edges, and interior points, shared by every
// generator so the golden file stays small and readable. Same grid
// `tests/materials_surfaces_ground/capture.mjs` uses.
const UVS = [
  [0.0, 0.0], [0.13, 0.77], [0.42, 0.09], [0.91, 0.36], [1.0, 1.0],
  [0.25, 0.5], [0.5, 0.25], [0.6, 0.8], [0.05, 0.95], [0.33, 0.33],
];

// Same seeds (and, for fabric, tints) as the library entries in
// `src/materials/mod.rs::LIBRARY` (`apps/claude-of-duty/src/materials/mod.rs`):
// wood=19, fabric=43 (tintA=0x5a5445, tintB=0x3a3830), burlap=67, foliage=79,
// rubber=97, glass=3 — so the golden doubles as a real-world contract check.
const fabricTintA = hexToLinear(0x5a5445);
const fabricTintB = hexToLinear(0x3a3830);

const out = {
  wood: { seed: 19.0, samples: UVS.map((uv) => ({ uv, out: woodSurface(uv, 19.0) })) },
  fabric: {
    seed: 43.0,
    tintA: fabricTintA,
    tintB: fabricTintB,
    samples: UVS.map((uv) => ({ uv, out: fabricSurface(uv, 43.0, fabricTintA, fabricTintB) })),
  },
  burlap: { seed: 67.0, samples: UVS.map((uv) => ({ uv, out: burlapSurface(uv, 67.0) })) },
  foliage: { seed: 79.0, samples: UVS.map((uv) => ({ uv, out: foliageSurface(uv, 79.0) })) },
  rubber: { seed: 97.0, samples: UVS.map((uv) => ({ uv, out: rubberSurface(uv, 97.0) })) },
  glass: { seed: 3.0, samples: UVS.map((uv) => ({ uv, out: glassSurface(uv, 3.0) })) },
};

// A dense grid for foliage alone, to pin the "h is a binary-ish cutout mask,
// not a smooth height" claim against the real transcription, not just the
// Rust port's own re-derivation of the same shape.
{
  const dense = [];
  for (let iy = 0; iy <= 24; iy++) {
    for (let ix = 0; ix <= 24; ix++) {
      dense.push([ix / 24, iy / 24]);
    }
  }
  out.foliageDenseH = dense.map((uv) => ({ uv, h: foliageSurface(uv, 79.0).h }));
}

process.stdout.write(JSON.stringify(out, null, 1));
