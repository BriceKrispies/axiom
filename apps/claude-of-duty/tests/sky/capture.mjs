/**
 * Golden capture for the Claude-of-Duty sky port.
 *
 * `src/sky/celestial.js` and the CPU tail of `src/sky/atmosphere.js`
 * (`transmittanceToSpace`, `luminance`, the `ATMO`/`SCENE_LUX` constants) are
 * genuine JavaScript with a real Node oracle, so this script imports and
 * calls the ORIGINAL `C:/dev/Claude-of-Duty/src/sky/{atmosphere,celestial}.js`
 * directly — no transcription, exactly `tests/audio/capture.mjs`'s method.
 *
 * The rest of `src/sky/` (the `*_GLSL` template strings in atmosphere.js, and
 * every `*_FRAG` shader body in luts.js) is WebGL2 fragment-shader source: it
 * has no JavaScript form to import, because it never runs anywhere but a
 * browser GPU. There is therefore no oracle for those pieces beyond the GLSL
 * text itself. Below, each such shader body is hand-transcribed into a plain
 * JS function, one function per named GLSL function, each tagged with the
 * exact `atmosphere.js`/`luts.js` line range it transcribes — a *second*,
 * independently-reviewable translation of the same source the Rust port
 * (`apps/claude-of-duty/src/sky/{atmosphere,luts,noise}.rs`) also transcribes.
 * Auditing correctness therefore means reading three things side by side: the
 * GLSL, this file, and the Rust — not trusting either transcription as a
 * silent oracle for the other.
 *
 * Regenerate (from this directory):
 *
 *   node capture.mjs > golden.json
 */

import {
  ATMO,
  SCENE_LUX,
  SUN_ILLUMINANCE_TOP,
  MOON_ILLUMINANCE_NIGHT,
  transmittanceToSpace,
  luminance,
} from 'file:///C:/dev/Claude-of-Duty/src/sky/atmosphere.js';
import { SITE, solarDeclination, altAz, dirFromAltAz, Celestial } from 'file:///C:/dev/Claude-of-Duty/src/sky/celestial.js';
// `celestial.js` imports 'three' as a bare specifier, resolved relative to
// its OWN location — that works because Node walks up from
// C:/dev/Claude-of-Duty/src/sky/ to find node_modules/three. This script
// lives elsewhere, so the same bare specifier would not resolve here; import
// three's real module entry by absolute path instead, so `dirFromAltAz`'s
// `out: THREE.Vector3` argument and `Celestial.celestialMatrix`'s
// `out: THREE.Matrix3` argument get the real classes the source expects.
import * as THREE from 'file:///C:/dev/Claude-of-Duty/node_modules/three/build/three.module.js';

const ISO_PHASE = 0.07957747154594767; // SK_ISO_PHASE, atmosphere.js:118

/* ------------------------------------------------------------------ */
/* vec3 helpers                                                        */
/* ------------------------------------------------------------------ */
const v3 = (x, y, z) => [x, y, z];
const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const mul = (a, b) => [a[0] * b[0], a[1] * b[1], a[2] * b[2]];
const div = (a, b) => [a[0] / b[0], a[1] / b[1], a[2] / b[2]];
const scale = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
const splat = (s) => [s, s, s];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const length = (a) => Math.sqrt(dot(a, a));
const normalize = (a) => scale(a, 1 / length(a));
const vmax = (a, b) => [Math.max(a[0], b[0]), Math.max(a[1], b[1]), Math.max(a[2], b[2])];
const vexp = (a) => [Math.exp(a[0]), Math.exp(a[1]), Math.exp(a[2])];
const glSign = (x) => (x > 0 ? 1 : x < 0 ? -1 : 0);
const safeAcos = (x) => Math.acos(Math.max(-1, Math.min(1, x)));

/* ------------------------------------------------------------------ */
/* atmosphere.js:106-269 GLSL, transcribed                             */
/* ------------------------------------------------------------------ */

/** `skRaySphere`, atmosphere.js:128-136. */
function raySphere(ro, rd, rad) {
  const b = dot(ro, rd);
  const c = dot(ro, ro) - rad * rad;
  if (c > 0 && b > 0) return -1;
  const d = b * b - c;
  if (d < 0) return -1;
  if (d > b * b) return -b + Math.sqrt(d);
  return -b - Math.sqrt(d);
}

/** `skMedium`, atmosphere.js:138-148. */
function medium(pos, mieScale) {
  const altKM = (length(pos) - ATMO.groundRadiusMM) * 1000;
  const rDen = Math.exp(-altKM / ATMO.rayleighScaleHeightKM);
  const mDen = Math.exp(-altKM / ATMO.mieScaleHeightKM);
  const rayleighS = scale(ATMO.rayleigh, rDen);
  const mieS = ATMO.mieScattering * mieScale * mDen;
  const mieA = ATMO.mieAbsorption * mieScale * mDen;
  const ozone = scale(ATMO.ozone, Math.max(0, 1 - Math.abs(altKM - ATMO.ozoneCentreKM) / ATMO.ozoneWidthKM));
  const extinction = add(add(rayleighS, splat(mieS + mieA)), ozone);
  return { rayleighS, mieS, extinction };
}

/** `skMiePhase`, atmosphere.js:151-155. */
function miePhase(cosTheta) {
  const g = 0.8;
  const k = (3.0 / (8.0 * Math.PI)) * (1 - g * g) / (2 + g * g);
  return (k * (1 + cosTheta * cosTheta)) / Math.pow(1 + g * g - 2 * g * cosTheta, 1.5);
}

/** `skRayleighPhase`, atmosphere.js:157-159. */
function rayleighPhase(cosTheta) {
  return (3.0 / (16.0 * Math.PI)) * (1 + cosTheta * cosTheta);
}

/** `skHG`, atmosphere.js:162-166. */
function hgPhase(cosTheta, g) {
  const g2 = g * g;
  const d = Math.max(1e-4, 1 + g2 - 2 * g * cosTheta);
  return (1 - g2) / (4 * Math.PI * d * Math.sqrt(d));
}

/** `skLutUv`, TRANSMITTANCE_LOOKUP_GLSL, atmosphere.js:177-183. */
function lutUv(pos, dir) {
  const h = length(pos);
  const mu = dot(dir, scale(pos, 1 / h));
  return [
    Math.max(0, Math.min(1, 0.5 + 0.5 * mu)),
    Math.max(0, Math.min(1, (h - ATMO.groundRadiusMM) / (ATMO.atmosphereRadiusMM - ATMO.groundRadiusMM))),
  ];
}

/** `skRaymarchSky`, SCATTER_GLSL, atmosphere.js:212-269. */
function raymarchSky(pos, rayDir, sunDir, sunIrr, moonDir, moonIrr, steps, mieScale, sampleT, sampleM) {
  const topT = raySphere(pos, rayDir, ATMO.atmosphereRadiusMM);
  const groundT = raySphere(pos, rayDir, ATMO.groundRadiusMM);
  const tMax = groundT < 0 ? topT : groundT;
  if (tMax <= 0) return [0, 0, 0];

  const cS = dot(rayDir, sunDir);
  const cM = dot(rayDir, moonDir);
  const mieSPhase = miePhase(cS);
  const raySPhase = rayleighPhase(cS);
  const mieMPhase = miePhase(cM);
  const rayMPhase = rayleighPhase(cM);

  let lum = [0, 0, 0];
  let trans = [1, 1, 1];
  let t = 0;
  for (let i = 0; i < steps; i++) {
    const nt = ((i + 0.3) / steps) * tMax;
    const dt = nt - t;
    t = nt;
    const p = add(pos, scale(rayDir, t));

    const m = medium(p, mieScale);
    const sampleTt = vexp(scale(m.extinction, -dt));

    const tSun = sampleT(p, sunDir);
    const psiSun = sampleM(p, sunDir);
    let inScatter = mul(
      add(mul(m.rayleighS, add(scale(tSun, raySPhase), psiSun)), mul(splat(m.mieS), add(scale(tSun, mieSPhase), psiSun))),
      sunIrr,
    );

    const tMoon = sampleT(p, moonDir);
    const psiMoon = sampleM(p, moonDir);
    inScatter = add(
      inScatter,
      mul(
        add(mul(m.rayleighS, add(scale(tMoon, rayMPhase), psiMoon)), mul(splat(m.mieS), add(scale(tMoon, mieMPhase), psiMoon))),
        moonIrr,
      ),
    );

    lum = add(lum, div(mul(trans, sub(inScatter, mul(inScatter, sampleTt))), vmax(m.extinction, splat(1e-8))));
    trans = mul(trans, sampleTt);
  }
  return lum;
}

/* ------------------------------------------------------------------ */
/* luts.js — Lut2D + the four bakes                                    */
/* ------------------------------------------------------------------ */

function makeLut(width, height, wrapS) {
  return { width, height, wrapS, data: new Array(width * height).fill(0).map(() => [0, 0, 0]) };
}
function lutSet(lut, x, y, v) {
  lut.data[y * lut.width + x] = v;
}
function lutTexel(lut, x, y) {
  const xi = lut.wrapS ? (((x % lut.width) + lut.width) % lut.width) : Math.max(0, Math.min(lut.width - 1, x));
  const yi = Math.max(0, Math.min(lut.height - 1, y));
  return lut.data[yi * lut.width + xi];
}
/** GLSL `texture(sampler, vec2(u,v))` — bilinear, texel-centred. */
function lutSample(lut, u, v) {
  const tx = u * lut.width - 0.5;
  const ty = v * lut.height - 0.5;
  const x0 = Math.floor(tx);
  const y0 = Math.floor(ty);
  const fx = tx - x0;
  const fy = ty - y0;
  const c00 = lutTexel(lut, x0, y0);
  const c10 = lutTexel(lut, x0 + 1, y0);
  const c01 = lutTexel(lut, x0, y0 + 1);
  const c11 = lutTexel(lut, x0 + 1, y0 + 1);
  const top = add(scale(c00, 1 - fx), scale(c10, fx));
  const bot = add(scale(c01, 1 - fx), scale(c11, fx));
  return add(scale(top, 1 - fy), scale(bot, fy));
}

/** TRANSMITTANCE_FRAG, luts.js:61-87. */
function bakeTransmittance(width, height, steps, mieScale) {
  const lut = makeLut(width, height, false);
  for (let j = 0; j < height; j++) {
    for (let i = 0; i < width; i++) {
      const vu = (i + 0.5) / width;
      const vv = (j + 0.5) / height;
      const mu = vu * 2 - 1;
      const h = ATMO.groundRadiusMM + (ATMO.atmosphereRadiusMM - ATMO.groundRadiusMM) * vv;
      const pos = v3(0, h, 0);
      const dir = v3(Math.sqrt(Math.max(0, 1 - mu * mu)), mu, 0);
      const t = raySphere(pos, dir, ATMO.atmosphereRadiusMM);
      if (t <= 0) {
        lutSet(lut, i, j, [0, 0, 0]);
        continue;
      }
      const dt = t / steps;
      let od = [0, 0, 0];
      for (let s = 0; s < steps; s++) {
        const p = add(pos, scale(dir, (s + 0.5) * dt));
        const m = medium(p, mieScale);
        od = add(od, scale(m.extinction, dt));
      }
      lutSet(lut, i, j, vexp(scale(od, -1)));
    }
  }
  return lut;
}

/** MULTISCATTER_FRAG, luts.js:89-161. */
function bakeMultiscatter(size, steps, sqrtSamples, mieScale, transmittance) {
  const lut = makeLut(size, size, false);
  const sampleT = (p, dir) => {
    const [u, v] = lutUv(p, dir);
    return lutSample(transmittance, u, v);
  };
  for (let j = 0; j < size; j++) {
    for (let i = 0; i < size; i++) {
      const vu = (i + 0.5) / size;
      const vv = (j + 0.5) / size;
      const mu = vu * 2 - 1;
      const hMin = ATMO.groundRadiusMM + 1e-5;
      const h = hMin + (ATMO.atmosphereRadiusMM - hMin) * vv;
      const pos = v3(0, h, 0);
      const sunDir = normalize(v3(Math.sqrt(Math.max(0, 1 - mu * mu)), mu, 0));

      let lumTotal = [0, 0, 0];
      let fmsTotal = [0, 0, 0];
      const invSamples = 1 / (sqrtSamples * sqrtSamples);

      for (let si = 0; si < sqrtSamples; si++) {
        for (let sj = 0; sj < sqrtSamples; sj++) {
          const theta = (Math.PI * (si + 0.5)) / sqrtSamples;
          const phi = safeAcos(1 - (2 * (sj + 0.5)) / sqrtSamples);
          const cp = Math.cos(phi);
          const sp = Math.sin(phi);
          const rayDir = v3(sp * Math.sin(theta), cp, sp * Math.cos(theta));

          const topT = raySphere(pos, rayDir, ATMO.atmosphereRadiusMM);
          const grnT = raySphere(pos, rayDir, ATMO.groundRadiusMM);
          const tMax = grnT < 0 ? topT : grnT;
          if (tMax <= 0) continue;

          let lum = [0, 0, 0];
          let fms = [0, 0, 0];
          let trans = [1, 1, 1];
          let t = 0;
          for (let s = 0; s < steps; s++) {
            const nt = ((s + 0.5) / steps) * tMax;
            const dt = nt - t;
            t = nt;
            const p = add(pos, scale(rayDir, t));
            const m = medium(p, mieScale);
            const sampleTt = vexp(scale(m.extinction, -dt));

            const sNoPhase = add(m.rayleighS, splat(m.mieS));
            fms = add(fms, div(mul(trans, sub(sNoPhase, mul(sNoPhase, sampleTt))), vmax(m.extinction, splat(1e-8))));

            const tSun = sampleT(p, sunDir);
            const inS = mul(scale(sNoPhase, ISO_PHASE), tSun);
            lum = add(lum, div(mul(trans, sub(inS, mul(inS, sampleTt))), vmax(m.extinction, splat(1e-8))));
            trans = mul(trans, sampleTt);
          }

          if (grnT > 0) {
            const hit = scale(normalize(add(pos, scale(rayDir, grnT))), ATMO.groundRadiusMM);
            if (dot(hit, sunDir) > 0) {
              lum = add(lum, mul(scale(trans, ATMO.groundAlbedo), sampleT(hit, sunDir)));
            }
          }

          lumTotal = add(lumTotal, scale(lum, invSamples));
          fmsTotal = add(fmsTotal, scale(fms, invSamples));
        }
      }

      const psi = div(lumTotal, vmax(sub([1, 1, 1], fmsTotal), splat(1e-4)));
      lutSet(lut, i, j, psi);
    }
  }
  return lut;
}

/** SKYVIEW_FRAG, luts.js:163-200. */
function bakeSkyView(width, height, steps, params, transmittance, multiscatter) {
  const lut = makeLut(width, height, true);
  const sampleT = (p, dir) => {
    const [u, v] = lutUv(p, dir);
    return lutSample(transmittance, u, v);
  };
  const sampleM = (p, dir) => {
    const [u, v] = lutUv(p, dir);
    return lutSample(multiscatter, u, v);
  };
  const sunDir = v3(0, Math.sin(params.sunAltitude), -Math.cos(params.sunAltitude));
  const cm = Math.cos(params.moonAltitude);
  const moonDir = v3(cm * Math.sin(params.moonRelAz), Math.sin(params.moonAltitude), -cm * Math.cos(params.moonRelAz));

  for (let j = 0; j < height; j++) {
    for (let i = 0; i < width; i++) {
      const vu = (i + 0.5) / width;
      const vv = (j + 0.5) / height;
      const azimuth = (vu - 0.5) * 2 * Math.PI;
      const adjV = vv < 0.5 ? -(1 - 2 * vv) * (1 - 2 * vv) : (2 * vv - 1) * (2 * vv - 1);
      const h = length(params.viewPos);
      const horizon = safeAcos(Math.sqrt(h * h - ATMO.groundRadiusMM * ATMO.groundRadiusMM) / h) - 0.5 * Math.PI;
      const altitude = adjV * 0.5 * Math.PI - horizon;
      const ca = Math.cos(altitude);
      const rayDir = v3(ca * Math.sin(azimuth), Math.sin(altitude), -ca * Math.cos(azimuth));

      const lum = raymarchSky(params.viewPos, rayDir, sunDir, params.sunIrradiance, moonDir, params.moonIrradiance, steps, params.mieScale, sampleT, sampleM);
      lutSet(lut, i, j, lum);
    }
  }
  return lut;
}

/** `skSkyView`, SKYVIEW_LOOKUP_GLSL, luts.js:37-53. */
function skyViewLookup(lut, rayDir, sunDir, viewPos) {
  const h = length(viewPos);
  const up = scale(viewPos, 1 / h);
  const horizon = safeAcos(Math.sqrt(h * h - ATMO.groundRadiusMM * ATMO.groundRadiusMM) / h);
  const altitude = horizon - safeAcos(dot(rayDir, up));

  let azimuth = 0;
  if (Math.abs(altitude) < 0.5 * Math.PI - 1e-4) {
    const right = cross(sunDir, up);
    const fwd = cross(up, right);
    const proj = normalize(sub(rayDir, scale(up, dot(rayDir, up))));
    azimuth = Math.atan2(dot(proj, right), dot(proj, fwd)) + Math.PI;
  }

  const v = 0.5 + 0.5 * glSign(altitude) * Math.sqrt(Math.abs(altitude) * 2 / Math.PI);
  return lutSample(lut, azimuth / (2 * Math.PI), v);
}

/** AMBIENT_FRAG, luts.js:208-235. */
function ambientTexel(skyView, sunDir, viewPos, horizonBand) {
  let sum = [0, 0, 0];
  let wsum = 0;
  const N = 64;
  for (let i = 0; i < N; i++) {
    const fi = (i + 0.5) / N;
    const phi = i * 2.39996323;
    const ct = horizonBand ? -0.12 + (0.35 - -0.12) * fi : Math.sqrt(1 - fi);
    const st = Math.sqrt(Math.max(0, 1 - ct * ct));
    const d = v3(st * Math.cos(phi), ct, st * Math.sin(phi));
    const w = horizonBand ? 1 : Math.max(0, ct);
    sum = add(sum, scale(skyViewLookup(skyView, d, sunDir, viewPos), w));
    wsum += w;
  }
  return scale(sum, 1 / Math.max(wsum, 1e-4));
}
function bakeAmbient(skyView, sunAltitude, viewPos) {
  const sunDir = v3(0, Math.sin(sunAltitude), -Math.cos(sunAltitude));
  return [ambientTexel(skyView, sunDir, viewPos, false), ambientTexel(skyView, sunDir, viewPos, true)];
}

/* ------------------------------------------------------------------ */
/* Build the golden                                                    */
/* ------------------------------------------------------------------ */

const out = {};

out.constants = {
  sceneLux: SCENE_LUX,
  sunIlluminanceTop: SUN_ILLUMINANCE_TOP,
  moonIlluminanceNight: MOON_ILLUMINANCE_NIGHT,
  isoPhase: ISO_PHASE,
  atmo: ATMO,
};

/* -- phase functions, fixed cosTheta samples -------------------------- */
out.phaseFunctions = [-1, -0.75, -0.5, -0.2, 0, 0.2, 0.5, 0.75, 0.9, 1].map((c) => ({
  cosTheta: c,
  mie: miePhase(c),
  rayleigh: rayleighPhase(c),
  hg_g0_76: hgPhase(c, 0.76),
}));

/* -- raySphere, fixed cases -------------------------------------------- */
out.raySphere = [
  { ro: [0, 6.36, 0], rd: [0, 1, 0], rad: 6.46, t: raySphere([0, 6.36, 0], [0, 1, 0], 6.46) },
  { ro: [0, 6.36, 0], rd: [0, -1, 0], rad: 6.46, t: raySphere([0, 6.36, 0], [0, -1, 0], 6.46) },
  { ro: [0, 6.36002, 0], rd: [1, 0, 0], rad: 6.46, t: raySphere([0, 6.36002, 0], [1, 0, 0], 6.46) },
  { ro: [0, 6.36002, 0], rd: [0, 1, 0], rad: 6.36, t: raySphere([0, 6.36002, 0], [0, 1, 0], 6.36) },
  { ro: [0, 10, 0], rd: [0, 1, 0], rad: 6.46, t: raySphere([0, 10, 0], [0, 1, 0], 6.46) },
];

/* -- medium, fixed altitude/position samples ---------------------------- */
out.medium = [0, 1, 5, 10, 25, 50, 100].map((altKm) => {
  const pos = [0, ATMO.groundRadiusMM + altKm / 1000, 0];
  const m = medium(pos, 1.35);
  return { altKm, mieScale: 1.35, rayleighS: m.rayleighS, mieS: m.mieS, extinction: m.extinction };
});

/* -- lutUv -------------------------------------------------------------- */
out.lutUv = [
  { pos: [0, 6.36 + 0.0002, 0], dir: [0, 1, 0] },
  { pos: [0, 6.36 + 0.0002, 0], dir: [1, 0, 0] },
  { pos: [0, 6.41, 0], dir: normalize([1, 1, 0]) },
].map(({ pos, dir }) => ({ pos, dir, uv: lutUv(pos, dir) }));

/* -- transmittanceToSpace / luminance (real oracle) ---------------------- */
out.transmittanceToSpace = [-0.9, -0.3, 0, 0.3, 0.6, 1.0].map((mu) => ({
  mu,
  mieScale: 1.35,
  rgb: transmittanceToSpace(mu, 1.35),
}));
out.luminance = [
  [1, 1, 1],
  [1, 0, 0],
  [0.5, 0.6, 0.7],
].map((rgb) => ({ rgb, value: luminance(rgb) }));

/* -- raymarchSky segment integral, bake-independent (constant stub LUTs) - */
{
  const stubT = () => [1, 1, 1];
  const stubM = () => [0.01, 0.02, 0.03];
  const viewPos = [0, ATMO.groundRadiusMM + ATMO.viewAltitudeMM, 0];
  const cases = [
    { rayDir: [0, 1, 0], sunDir: normalize([0, 1, -0.2]), sunIrr: [4.7, 4.4, 3.9], moonDir: [0, -1, 0], moonIrr: [0, 0, 0], steps: 8 },
    { rayDir: normalize([0.6, 0.3, -0.7]), sunDir: normalize([0.2, 0.1, -1]), sunIrr: [4.7, 4.4, 3.9], moonDir: normalize([-0.3, 0.4, 0.8]), moonIrr: [0.02, 0.02, 0.03], steps: 12 },
  ];
  out.raymarchSkySegment = cases.map((c) => ({
    ...c,
    mieScale: 1.35,
    lum: raymarchSky(viewPos, c.rayDir, c.sunDir, c.sunIrr, c.moonDir, c.moonIrr, c.steps, 1.35, stubT, stubM),
  }));
}

/* -- transmittance LUT, full 256x64 dump --------------------------------- */
{
  const W = 256, H = 64, STEPS = 40, MIE = 1.35;
  const lut = bakeTransmittance(W, H, STEPS, MIE);
  out.transmittanceLut = { width: W, height: H, steps: STEPS, mieScale: MIE, data: lut.data };
}

/* -- multiscatter LUT, full 32x32 dump ------------------------------------ */
let multiscatterLutObj;
{
  const SIZE = 32, STEPS = 20, SQ = 8, MIE = 1.35;
  const transmittance = bakeTransmittance(256, 64, 40, MIE);
  multiscatterLutObj = bakeMultiscatter(SIZE, STEPS, SQ, MIE, transmittance);
  out.multiscatterLut = { size: SIZE, steps: STEPS, sqrtSamples: SQ, mieScale: MIE, data: multiscatterLutObj.data };
}

/* -- sky-view LUT, reduced 64x32 dump (same bake fn, cheaper grid) -------- */
let skyViewLutObj;
const skyViewParams = {
  sunIrradiance: [4.774063735416551, 4.38636408750061, 3.8342290587459558],
  moonIrradiance: [0.006, 0.0065, 0.008],
  sunAltitude: (68.44 * Math.PI) / 180,
  moonRelAz: (140 * Math.PI) / 180,
  moonAltitude: (-8 * Math.PI) / 180,
  viewPos: [0, ATMO.groundRadiusMM + ATMO.viewAltitudeMM, 0],
  mieScale: 1.35,
};
{
  const transmittance = bakeTransmittance(256, 64, 40, skyViewParams.mieScale);
  const multiscatter = multiscatterLutObj;
  const W = 64, H = 32, STEPS = 40;
  skyViewLutObj = bakeSkyView(W, H, STEPS, skyViewParams, transmittance, multiscatter);
  out.skyViewLut = { width: W, height: H, steps: STEPS, params: skyViewParams, data: skyViewLutObj.data };
}

/* -- skyViewLookup, fixed ray directions against the baked LUT above -----
 * NOTE: exact zenith ([0,1,0], parallel to `up`) is a genuine singularity in
 * `skSkyView` (SKYVIEW_LOOKUP_GLSL, luts.js:37-53): `proj = normalize(rayDir
 * - up*dot(rayDir,up))` divides a zero vector by its own zero length there,
 * same in GLSL and in the Rust/JS ports alike (0/0 -> NaN in every one of
 * them). The source never special-cases it, so this is ported behaviour, not
 * a defect to silently fix — but it also makes a bad *test input* (NaN
 * compares unequal to itself), so every direction below is near-zenith
 * rather than exactly at it. */
out.skyViewLookup = [
  normalize([0.05, 1, 0.02]),
  normalize([0, 0.3, -1]),
  normalize([1, 0.05, 0]),
  normalize([0, 0.02, 1]),
].map((rayDir) => ({
  rayDir,
  rgb: skyViewLookup(skyViewLutObj, rayDir, [0, Math.sin(skyViewParams.sunAltitude), -Math.cos(skyViewParams.sunAltitude)], skyViewParams.viewPos),
}));

/* -- ambient probe --------------------------------------------------------- */
{
  const [texel0, texel1] = bakeAmbient(skyViewLutObj, skyViewParams.sunAltitude, skyViewParams.viewPos);
  out.ambientProbe = { sunAltitude: skyViewParams.sunAltitude, viewPos: skyViewParams.viewPos, texel0, texel1 };
}

/* -- derived photometric-contract constants (order-of-magnitude checks) -- */
{
  const noonMu = Math.sin((68.4397829394163 * Math.PI) / 180);
  const noonSunRgb = transmittanceToSpace(noonMu, 1.35).map((c) => c * SUN_ILLUMINANCE_TOP);
  out.derivedConstants = {
    sunIlluminanceTop: SUN_ILLUMINANCE_TOP,
    noonSunRgb,
    noonSunLuminance: luminance(noonSunRgb),
    zenithSkyRgb: (() => {
      const transmittance = bakeTransmittance(256, 64, 40, 1.35);
      const multiscatter = bakeMultiscatter(32, 20, 8, 1.35, transmittance);
      const sampleT = (p, dir) => { const [u, v] = lutUv(p, dir); return lutSample(transmittance, u, v); };
      const sampleM = (p, dir) => { const [u, v] = lutUv(p, dir); return lutSample(multiscatter, u, v); };
      const viewPos = [0, ATMO.groundRadiusMM + ATMO.viewAltitudeMM, 0];
      const sunAlt = (68.4397829394163 * Math.PI) / 180;
      const sunDir = [0, Math.sin(sunAlt), -Math.cos(sunAlt)];
      return raymarchSky(viewPos, [0, 1, 0], sunDir, noonSunRgb, [0, -1, 0], [0, 0, 0], 40, 1.35, sampleT, sampleM);
    })(),
  };
  out.derivedConstants.zenithSkyLuminance = luminance(out.derivedConstants.zenithSkyRgb);
}

/* -- noise.js (no oracle — NOISE_GLSL is shader source only) -------------
 * Transcribed from `src/sky/noise.js:13-89`, the same "no oracle, hand-
 * transcribe and line-reference" situation as the LUT bakes above.
 */
{
  const glFract = (x) => x - Math.floor(x);
  const glMix = (a, b, t) => a + (b - a) * t;
  const smooth = (v) => v * v * (3 - 2 * v);

  const skHash12 = (p) => {
    let p3 = [glFract(p[0] * 0.1031), glFract(p[1] * 0.1031), glFract(p[0] * 0.1031)];
    const yzx = [p3[1] + 33.33, p3[2] + 33.33, p3[0] + 33.33];
    const d = dot(p3, yzx);
    p3 = [p3[0] + d, p3[1] + d, p3[2] + d];
    return glFract((p3[0] + p3[1]) * p3[2]);
  };
  const skHash13 = (p) => {
    let pp = [glFract(p[0] * 0.1031), glFract(p[1] * 0.1031), glFract(p[2] * 0.1031)];
    const yzx = [pp[1] + 33.33, pp[2] + 33.33, pp[0] + 33.33];
    const d = dot(pp, yzx);
    pp = [pp[0] + d, pp[1] + d, pp[2] + d];
    return glFract((pp[0] + pp[1]) * pp[2]);
  };
  const skHash33 = (p) => {
    let pp = [glFract(p[0] * 0.1031), glFract(p[1] * 0.11369), glFract(p[2] * 0.13787)];
    const yxz = [pp[1] + 19.19, pp[0] + 19.19, pp[2] + 19.19];
    const d = dot(pp, yxz);
    pp = [pp[0] + d, pp[1] + d, pp[2] + d];
    return [glFract((pp[0] + pp[1]) * pp[2]), glFract((pp[0] + pp[2]) * pp[1]), glFract((pp[1] + pp[2]) * pp[0])];
  };
  const skIGN = (p) => glFract(52.9829189 * glFract(p[0] * 0.06711056 + p[1] * 0.00583715));
  const skVal2 = (p) => {
    const i = [Math.floor(p[0]), Math.floor(p[1])];
    let f = [glFract(p[0]), glFract(p[1])];
    f = [smooth(f[0]), smooth(f[1])];
    const a = skHash12(i);
    const b = skHash12([i[0] + 1, i[1]]);
    const c = skHash12([i[0], i[1] + 1]);
    const d = skHash12([i[0] + 1, i[1] + 1]);
    return glMix(glMix(a, b, f[0]), glMix(c, d, f[0]), f[1]);
  };
  const skVal3 = (p) => {
    const i = [Math.floor(p[0]), Math.floor(p[1]), Math.floor(p[2])];
    const fr = [glFract(p[0]), glFract(p[1]), glFract(p[2])];
    const f = [smooth(fr[0]), smooth(fr[1]), smooth(fr[2])];
    const h = (dx, dy, dz) => skHash13([i[0] + dx, i[1] + dy, i[2] + dz]);
    const x00 = glMix(h(0, 0, 0), h(1, 0, 0), f[0]);
    const x10 = glMix(h(0, 1, 0), h(1, 1, 0), f[0]);
    const y0 = glMix(x00, x10, f[1]);
    const x01 = glMix(h(0, 0, 1), h(1, 0, 1), f[0]);
    const x11 = glMix(h(0, 1, 1), h(1, 1, 1), f[0]);
    const y1 = glMix(x01, x11, f[1]);
    return glMix(y0, y1, f[2]);
  };
  const skRot = (p) => [0.8 * p[0] - 0.6 * p[1], 0.6 * p[0] + 0.8 * p[1]];
  const skFbm2 = (p0, oct) => {
    let p = p0, a = 0.5, s = 0, n = 0;
    for (let i = 0; i < oct; i++) {
      s += a * skVal2(p);
      n += a;
      const r = skRot(p);
      p = [r[0] * 2.04 + 7.13, r[1] * 2.04 + 7.13];
      a *= 0.5;
    }
    return s / Math.max(n, 1e-4);
  };
  const skRidge2 = (p0, oct) => {
    let p = p0, a = 0.5, s = 0, n = 0;
    for (let i = 0; i < oct; i++) {
      s += a * (1 - Math.abs(skVal2(p) * 2 - 1));
      n += a;
      const r = skRot(p);
      p = [r[0] * 2.11 + 3.71, r[1] * 2.11 + 3.71];
      a *= 0.52;
    }
    return s / Math.max(n, 1e-4);
  };
  const skFbm3 = (p0, oct) => {
    let p = p0, a = 0.5, s = 0, n = 0;
    for (let i = 0; i < oct; i++) {
      s += a * skVal3(p);
      n += a;
      p = add(scale(p, 2.07), [11.3, 5.1, 7.7]);
      a *= 0.5;
    }
    return s / Math.max(n, 1e-4);
  };

  const p2s = [[0.3, 1.7], [4.2, -2.1], [-3.5, 9.9], [0, 0], [10.25, -10.25]];
  const p3s = [[0.3, 1.7, -2.2], [4.2, -2.1, 5.5], [-3.5, 9.9, 0.1], [0, 0, 0]];

  out.noise = {
    hash12: p2s.map((p) => ({ p, v: skHash12(p) })),
    hash13: p3s.map((p) => ({ p, v: skHash13(p) })),
    hash33: p3s.map((p) => ({ p, v: skHash33(p) })),
    ign: p2s.map((p) => ({ p, v: skIGN(p) })),
    val2: p2s.map((p) => ({ p, v: skVal2(p) })),
    val3: p3s.map((p) => ({ p, v: skVal3(p) })),
    fbm2: p2s.flatMap((p) => [1, 3, 6].map((oct) => ({ p, oct, v: skFbm2(p, oct) }))),
    ridge2: p2s.flatMap((p) => [1, 3, 6].map((oct) => ({ p, oct, v: skRidge2(p, oct) }))),
    fbm3: p3s.flatMap((p) => [1, 3, 6].map((oct) => ({ p, oct, v: skFbm3(p, oct) }))),
  };
}

/* -- celestial.js (real oracle) -------------------------------------------- */
out.celestial = {};
out.celestial.solarDeclination = [1, 80, 172, 266, 355].map((d) => ({ dayOfYear: d, decl: solarDeclination(d) }));
out.celestial.altAz = [
  { hourAngle: 0, decl: 0.3, lat: 45 },
  { hourAngle: 0.4, decl: 0.3, lat: 45 },
  { hourAngle: -0.4, decl: -0.1, lat: 45 },
  { hourAngle: 2.5, decl: 0.3, lat: 45 },
].map(({ hourAngle, decl, lat }) => ({ hourAngle, decl, lat, ...altAz(hourAngle, decl, lat) }));
out.celestial.dirFromAltAz = [
  { alt: 0.5, az: 1.2, north: 0 },
  { alt: -0.2, az: 4.0, north: 0.3 },
].map(({ alt, az, north }) => {
  const v = dirFromAltAz(alt, az, north, new THREE.Vector3());
  return { alt, az, north, dir: [v.x, v.y, v.z] };
});
out.celestial.setHour = [10, 12, 16.5, 19.2, 1.5].map((hour) => {
  const c = new Celestial(SITE);
  c.setHour(hour);
  // `Mat3(row-major)` on the Rust side vs. three's `Matrix3.elements`
  // (COLUMN-major): elements[col*3+row] = row-major[row][col]. The Rust
  // test transposes this golden's `celestialMatrixRows` accordingly — see
  // `tests/sky_port.rs`'s `celestial_matrix` comparison.
  const m3 = c.celestialMatrix(new THREE.Matrix3());
  const e = m3.elements; // column-major, length 9
  const rows = [
    [e[0], e[3], e[6]],
    [e[1], e[4], e[7]],
    [e[2], e[5], e[8]],
  ];
  return {
    hour,
    sun: [c.sun.x, c.sun.y, c.sun.z],
    moon: [c.moon.x, c.moon.y, c.moon.z],
    sunAlt: c.sunAlt,
    sunAz: c.sunAz,
    moonAlt: c.moonAlt,
    moonAz: c.moonAz,
    moonPhase: c.moonPhase,
    moonElongation: c.moonElongation,
    celestialMatrixRows: rows,
  };
});

process.stdout.write(JSON.stringify(out, null, 1));
