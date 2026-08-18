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
 * (`apps/shmup/src/sky/{atmosphere,luts,noise}.rs`) also transcribes.
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
// Genuine oracle for `skCloudMacro`'s CPU twin — clouds.js exports these two
// plain functions directly (see `cloudMacro`/`cloudSunOcclusion`,
// clouds.js:351-374), unlike everything else in clouds.js/dome.js/stars.js/
// volumetrics.js, which is WebGL2 shader source only.
import { cloudMacro, cloudSunOcclusion } from 'file:///C:/dev/Claude-of-Duty/src/sky/clouds.js';
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
 *
 * NOTE: these declarations are intentionally NOT block-scoped (no long
 * longer wrapped in `{ ... }`) — `dome`/`clouds`/`stars`/`volumetrics`'s
 * transcriptions further down this file call skVal2/skVal3/skFbm2/skRidge2/
 * skFbm3/skRot directly, the same way the real GLSL concatenates NOISE_GLSL
 * once and every later shader body calls into it.
 */
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

/* ====================================================================== */
/* dome.js / clouds.js / stars.js / volumetrics.js — no oracle, hand-       */
/* transcribed (WebGL2 fragment-shader source only, same situation as the   */
/* LUT bakes and noise.js above). Each function below is tagged with the    */
/* exact source line range it transcribes; pinned against the independent   */
/* Rust transcription in `src/sky/{dome,clouds,stars,volumetrics}.rs`.      */
/* ====================================================================== */

/* -- shared GLSL-builtin helpers not needed until this point --------------- */
const smoothstep = (e0, e1, x) => {
  const t = Math.max(0, Math.min(1, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
};
const mixv3 = (a, b, t) => add(a, scale(sub(b, a), t));
const floor3 = (a) => [Math.floor(a[0]), Math.floor(a[1]), Math.floor(a[2])];
const addS3 = (a, s) => [a[0] + s, a[1] + s, a[2] + s];
// 2D (vec2) helpers — clouds.js works in kilometres on the deck plane.
const add2 = (a, b) => [a[0] + b[0], a[1] + b[1]];
const sub2 = (a, b) => [a[0] - b[0], a[1] - b[1]];
const scale2 = (a, s) => [a[0] * s, a[1] * s];
const addS2 = (a, s) => [a[0] + s, a[1] + s];
const dot2 = (a, b) => a[0] * b[0] + a[1] * b[1];
const length2 = (a) => Math.sqrt(dot2(a, a));
const normalize2 = (a) => scale2(a, 1 / length2(a));

/* -- clouds.js: cloudMacro/cloudSunOcclusion have a REAL oracle ------------- */
/* imported directly from the original source (genuine oracle, like          */
/* celestial.js/transmittanceToSpace above) — no hand-transcription needed.   */

/* -- clouds.js: CLOUDS_GLSL, no oracle -------------------------------------- */
const CUMULUS_KM = 1.5; // SK_CUMULUS_KM, clouds.js:49
const CIRRUS_KM = 7.8; // SK_CIRRUS_KM, clouds.js:50

/** skSmoothRidge2, clouds.js:74-84. */
function skSmoothRidge2(p0, oct) {
  let p = p0, a = 0.62, s = 0, n = 0;
  for (let i = 0; i < oct; i++) {
    const v = skVal2(p) * 2 - 1;
    s += a * (1 - v * v);
    n += a;
    const r = skRot(p);
    p = [r[0] * 2.17 + 3.71, r[1] * 2.17 + 3.71];
    a *= 0.45;
  }
  return s / Math.max(n, 1e-4);
}

/** skCirrusBand, clouds.js:126-149. */
function skCirrusBand(p, cov, seed, base, rotKmInv, lenKM, aniso, oct) {
  const w = sub2(
    [skVal2(addS2(scale2(p, 0.30), seed)), skVal2(addS2(addS2(scale2(p, 0.30), seed), 11.7))],
    [0.5, 0.5],
  );
  const n = skFbm2(add2(scale2(p, 0.78), scale2(w, 1.3)), oct + 1);
  let d = smoothstep(1 - cov * 1.65, 1 - cov * 0.60, n);
  if (d <= 0.001) return 0;
  d *= smoothstep(0.36, 0.66, skVal2(addS2(scale2(p, 0.12), seed * 0.5)));
  if (d <= 0.001) return 0;
  const ang = base + (skVal2(addS2(scale2(p, rotKmInv), seed)) - 0.5) * 1.1;
  const ca = Math.cos(ang), sa = Math.sin(ang);
  const pr = [p[0] * ca - p[1] * sa, p[0] * sa + p[1] * ca];
  const fa = 1 / Math.max(0.4, lenKM);
  const q = [pr[0] * fa, pr[1] * fa * aniso];
  const f = skSmoothRidge2(addS2(q, seed), oct);
  return d * (0.35 + 1.05 * f);
}

/** skCumulusDensity, clouds.js:152-172. */
function skCumulusDensity(p, oct, coverage) {
  const macro = cloudMacro(p[0] * 0.22, p[1] * 0.22);
  const cov = Math.max(0, Math.min(1, coverage * (0.34 + 1.30 * macro)));
  const w = sub2([skVal2(scale2(p, 0.42)), skVal2(addS2(scale2(p, 0.42), 19.7))], [0.5, 0.5]);
  const n = skFbm2(add2(scale2(p, 1.25), scale2(w, 1.6)), oct);
  let d = smoothstep(1 - cov, 1 - cov * 0.34 + 0.05, n);
  if (d > 0 && d < 0.94 && oct > 3) {
    const e = skRidge2(add2(scale2(p, 5.3), scale2(w, 2.0)), 3);
    d = Math.max(0, Math.min(1, d - (1 - d) * (0.50 - 0.50 * e)));
  }
  return d;
}

/** skCumulusLight, clouds.js:179-186. */
function skCumulusLight(p, lightDir, oct, coverage, density) {
  const step2 = scale2(
    normalize2(addS2([lightDir[0], lightDir[2]], 1e-4)),
    0.20 / Math.max(0.12, Math.abs(lightDir[1])),
  );
  let tau = 0;
  tau += skCumulusDensity(add2(p, scale2(step2, 1.0)), oct, coverage) * 1.0;
  tau += skCumulusDensity(add2(p, scale2(step2, 2.4)), oct, coverage) * 0.7;
  tau += skCumulusDensity(add2(p, scale2(step2, 4.6)), oct, coverage) * 0.4;
  return Math.exp(-tau * density * 2.1);
}

/** skClouds, clouds.js:195-327. Returns { rgb, a }. */
function skClouds(rayDir, sunDir, sunLow, sunHigh, moonDir, moonLow, moonHigh, ambient, quality, params, viewPos) {
  if (rayDir[1] < -0.008) return { rgb: [0, 0, 0], a: 0 };

  const octD = quality > 0 ? 6 : 3;
  const octL = quality > 0 ? 4 : 2;
  const octC = 2;
  const t = params.time;
  const wind = scale2([params.windX, params.windZ], t);

  const cosSun = dot(rayDir, sunDir);
  const cosMoon = dot(rayDir, moonDir);

  let cirrusRgb = [0, 0, 0], cirrusA = 0;
  const tc = raySphere(viewPos, rayDir, ATMO.groundRadiusMM + CIRRUS_KM * 0.001);
  if (tc > 0) {
    const distKM = tc * 1000;
    let fade = 1 - smoothstep(22.0, 90.0, distKM);
    fade *= 1 - 0.66 * smoothstep(0.55, 0.85, rayDir[1]);
    if (fade > 0.004) {
      const hit = add(viewPos, scale(rayDir, tc));
      const p = add2(scale2([hit[0], hit[2]], 1000.0), scale2(wind, 2.4));
      const cov = Math.max(0, Math.min(1, params.cirrusCoverage));
      const d1 = skCirrusBand(p, cov, 0.0, 0.24, 0.135, 1.5, 4.0, octC);
      const d2 = skCirrusBand(addS2(p, 137.4), cov * 0.92, 4.7, 1.56, 0.098, 2.0, 3.4, octC);
      const d = 1 - (1 - d1) * (1 - d2 * 0.85);
      const a = Math.max(0, Math.min(0.70, d * params.cirrusOpacity * fade));
      const fwd = hgPhase(cosSun, 0.74) * 3.2 + 0.60;
      const col = add(
        scale(add(scale(sunHigh, fwd), scale(moonHigh, hgPhase(cosMoon, 0.68) * 2.8 + 0.55)), 1 / Math.PI),
        scale(ambient, 0.85),
      );
      cirrusRgb = col;
      cirrusA = a;
    }
  }

  let cumulusRgb = [0, 0, 0], cumulusA = 0;
  const tk = raySphere(viewPos, rayDir, ATMO.groundRadiusMM + CUMULUS_KM * 0.001);
  if (tk > 0) {
    const distKM = tk * 1000;
    const fade = 1 - smoothstep(14.0, 130.0, distKM);
    if (fade > 0.004) {
      const hit0 = add(viewPos, scale(rayDir, tk));
      const p0 = add2(scale2([hit0[0], hit0[2]], 1000.0), wind);
      const dBase = skCumulusDensity(p0, octD, params.coverage);
      const shear = scale2([rayDir[0], rayDir[2]], (0.85 * dBase) / Math.max(0.10, rayDir[1]));
      const d = Math.max(skCumulusDensity(add2(p0, shear), octD, params.coverage), dBase * 0.55);
      if (d > 0.003) {
        const p = add2(p0, shear);
        const lit = skCumulusLight(p, sunDir, octL, params.coverage, params.density);
        const litM = skCumulusLight(p, moonDir, octL, params.coverage, params.density);
        const graze = Math.max(0, Math.min(1, 0.09 / (Math.abs(rayDir[1]) + 0.09)));
        const thick = d * params.density * glMix(1.0, 1.7, graze);
        const a = Math.max(0, Math.min(1, 1 - Math.exp(-thick * 3.4))) * fade;
        const powder = 1 - Math.exp(-thick * 5.5);
        const rim = Math.pow(Math.max(0, Math.min(1, 1 - d)), 2.0);
        const fwdS = hgPhase(cosSun, 0.62) * 4.0 + 0.62;
        const fwdM = hgPhase(cosMoon, 0.60) * 3.4 + 0.55;
        let direct = scale(sunLow, lit * (0.55 + 0.45 * powder) * fwdS + rim * lit * 0.9);
        direct = add(direct, scale(moonLow, litM * (0.55 + 0.45 * powder) * fwdM + rim * litM * 0.9));
        const fill = scale(ambient, glMix(0.50, 1.5, Math.max(0, Math.min(1, d * 1.6))) * (0.32 + 0.68 * lit));
        cumulusRgb = add(scale(direct, 1 / Math.PI), fill);
        cumulusA = a;
      }
    }
  }

  const outA = cirrusA + cumulusA * (1 - cirrusA);
  let outC = add(scale(cirrusRgb, cirrusA), scale(cumulusRgb, cumulusA * (1 - cirrusA)));
  if (outA > 1e-5) outC = scale(outC, 1 / outA);
  return { rgb: outC, a: outA };
}

/** skCloudShadow, clouds.js:334-341. */
function skCloudShadow(worldXZ, sunDir, params) {
  const p = add2(
    add2(scale2(worldXZ, 0.001), scale2([sunDir[0], sunDir[2]], CUMULUS_KM / Math.max(0.10, sunDir[1]))),
    scale2([params.windX, params.windZ], params.time),
  );
  const d = skCumulusDensity(p, 4, params.coverage);
  return Math.exp(-d * params.density * 2.4);
}

/* -- stars.js: STARS_GLSL, no oracle ----------------------------------------- */
const SK_GAL_POLE = [-0.4288, 0.7146, 0.5522]; // stars.js:99
const SK_GAL_CORE = [0.7549, -0.2154, -0.6194]; // stars.js:100
const SK_STAR_TINT = 0.11; // stars.js:40

/** skBlackbody, stars.js:43-55. */
function skBlackbody(kelvin) {
  const t = Math.max(1200, Math.min(40000, kelvin)) / 100;
  let r, g, b;
  if (t <= 66) r = 1;
  else r = Math.max(0, Math.min(1, 1.29293619 * Math.pow(t - 60, -0.13320476)));
  if (t <= 66) g = Math.max(0, Math.min(1, 0.39008158 * Math.log(t) - 0.63184144));
  else g = Math.max(0, Math.min(1, 1.12989086 * Math.pow(t - 60, -0.07551485)));
  if (t >= 66) b = 1;
  else if (t <= 19) b = 0;
  else b = Math.max(0, Math.min(1, 0.54320679 * Math.log(t - 10) - 1.19625409));
  const c = [Math.pow(r, 2.2), Math.pow(g, 2.2), Math.pow(b, 2.2)];
  return scale(c, 1 / Math.max(1e-4, dot(c, [0.2126, 0.7152, 0.0722])));
}

/** skAirmass, stars.js:58-61. */
function skAirmass(cosZenith) {
  const z = (Math.acos(Math.max(-1, Math.min(1, cosZenith))) * 180) / Math.PI;
  return 1 / (Math.max(cosZenith, 0) + 0.50572 * Math.pow(Math.max(0, 96.07995 - z), -1.6364));
}

/** skStarLayer, stars.js:68-96. */
function skStarLayer(dir, N, keep, gain, seed, sigma, twinkle, band, time) {
  const cellFloor = floor3(scale(dir, N));
  const cell = addS3(cellFloor, seed);
  const h = skHash33(cell);
  if (h[0] < 1 - keep) return [0, 0, 0];

  const h2 = skHash33(addS3(cell, 91.7));
  const starDir = normalize(add(addS3(cellFloor, 0.5), scale(sub(h2, [0.5, 0.5, 0.5]), 0.94)));

  const d = length(cross(dir, starDir));

  const mag = Math.pow(h[1], 5.5);
  const flux = gain * (mag + 0.0016) * (1 + band * 1.4);

  const core = Math.exp(-(d * d) / (sigma * sigma));
  const skirt = 0.055 * Math.exp(-d / (sigma * 3.4));

  const tw =
    1 + twinkle * (Math.sin(time * (7 + 19 * h[2]) + h2[0] * 43) + 0.6 * Math.sin(time * (23 + 31 * h2[1])));
  const kelvin = glMix(2600, 22000, Math.pow(h2[2], 1.9));
  const tint = mixv3([1, 1, 1], skBlackbody(kelvin), SK_STAR_TINT);
  return scale(tint, flux * (core + skirt) * Math.max(0, tw));
}

/** skMilkyWay, stars.js:102-127. */
function skMilkyWay(eq, gain, oct) {
  const lat = dot(eq, SK_GAL_POLE);
  const spine = Math.exp(-Math.pow(Math.abs(lat) / 0.048, 1.55));
  const halo = Math.exp(-Math.pow(Math.abs(lat) / 0.165, 1.30));
  const band = Math.max(0, Math.min(1.4, 0.78 * spine + 0.48 * halo));
  if (band < 0.002) return [0, 0, 0];

  const toCore = dot(eq, SK_GAL_CORE);
  const bulge = Math.exp(-Math.pow(Math.max(0, 1 - toCore) / 0.22, 1.1));

  const q = scale(eq, 9.0);
  const clumps = skFbm3(q, oct);
  const dust = skFbm3(addS3(scale(eq, 21.0), 3.7), Math.max(2, oct - 1));
  const lane = smoothstep(0.36, 0.68, dust) * spine;

  let density = band * (0.20 + 1.35 * clumps * clumps) * (1 - 0.80 * lane);
  density *= 1 + 2.6 * bulge;

  const tint = mixv3([0.72, 0.80, 1.06], [1.10, 0.86, 0.62], bulge * 0.85);
  return scale(tint, density * gain);
}

/** mat3 * vec3 (row-major rows, as celestialMatrixRows are built elsewhere in this file). */
function mulMat3Vec3(rows, v) {
  return [dot(rows[0], v), dot(rows[1], v), dot(rows[2], v)];
}

/** skNightSky, stars.js:133-161. */
function skNightSky(dir, mwOctaves, points, celestialRows, star) {
  const eq = mulMat3Vec3(celestialRows, dir);
  const am = skAirmass(dir[1]);
  const ext = Math.exp(-0.145 * am) * smoothstep(-0.03, 0.10, dir[1]);

  let col = skMilkyWay(eq, star.milkywayGain, mwOctaves);

  if (points) {
    const tw = star.twinkle * Math.max(0, Math.min(0.85, (am - 1) * 0.16));
    const mw = Math.max(-1, Math.min(1, dot(eq, SK_GAL_POLE)));
    const band = Math.exp(-Math.pow(Math.abs(mw) / 0.16, 1.4));
    col = add(col, skStarLayer(eq, 21.0, 0.30, 1.00, 0.0, 0.00165, tw, band, star.time));
    col = add(col, skStarLayer(eq, 43.0, 0.20, 0.34, 13.0, 0.00145, tw, band, star.time));
    col = add(col, skStarLayer(eq, 87.0, 0.10, 0.11, 47.0, 0.00125, tw * 0.5, band * 2.2, star.time));
  }

  col = add(col, scale([0.55, 1.0, 0.78], 0.0003));

  return scale(col, star.brightness * ext);
}

/* -- dome.js: SKY_BODY, no oracle --------------------------------------------- */
const AUREOLE_CUT = 0.9135; // CUT, dome.js:100

/** skAureole, dome.js:99-117. `lightDir` is an unused GLSL parameter — see
 *  `src/sky/dome.rs`'s doc comment on `aureole` for why it is dropped here too. */
function skAureole(rayDirY, irradiance, transAlongRay, cosTheta, mieScale) {
  if (cosTheta <= AUREOLE_CUT) return [0, 0, 0];
  const mieOd = (ATMO.mieScattering * mieScale * 0.0012) / Math.max(0.055, rayDirY + 0.055);
  const excess = Math.max(0, miePhase(cosTheta) - miePhase(AUREOLE_CUT));
  return scale(mul(irradiance, transAlongRay), excess * mieOd * 4.2);
}

/** skRolloff, dome.js:140-154. */
function skRolloffFn(col, knee, exponent) {
  if (knee <= 0) return col;
  const l = Math.max(luminance(col), 1e-6);
  if (l <= knee) return col;
  return scale(col, (Math.pow(l / knee, exponent) * knee) / l);
}

/** skSunDisc, dome.js:70-82. */
function skSunDisc(theta, fwidthTheta, angRadius, drawScale, discRadiance, transToSun) {
  const rEdge = angRadius * drawScale;
  const aa = Math.max(1e-6, fwidthTheta);
  const cover = smoothstep(rEdge + aa, rEdge - aa, theta);
  if (cover <= 0) return [0, 0, 0];
  const r = Math.max(0, Math.min(1, theta / rEdge));
  const mu = Math.sqrt(Math.max(0, 1 - r * r));
  const limb = [Math.pow(mu, 0.32), Math.pow(mu, 0.44), Math.pow(mu, 0.58)];
  let v = mul(discRadiance, limb);
  v = scale(v, cover);
  v = mul(v, transToSun);
  v = scale(v, 1 / (drawScale * drawScale));
  return v;
}

/** skMoonDisc, dome.js:156-188. */
function skMoonDisc(rayDir, theta, oct, moonDir, sunDir, angRadius, drawScale, fwidthR2, discRadiance) {
  const rEdge = angRadius * drawScale;
  if (theta > rEdge * 1.6) return [0, 0, 0];
  const reference = Math.abs(moonDir[1]) > 0.97 ? [0, 0, 1] : [0, 1, 0];
  const mr = normalize(cross(reference, moonDir));
  const mu3 = cross(moonDir, mr);
  const px = dot(rayDir, mr) / rEdge;
  const py = dot(rayDir, mu3) / rEdge;
  const r2 = px * px + py * py;
  const aa = Math.max(1e-4, 1.9 * fwidthR2);
  const cover = smoothstep(1 + aa, 1 - aa, r2);
  if (cover <= 0) return [0, 0, 0];
  const n = normalize(
    sub(add(scale(mr, px), scale(mu3, py)), scale(moonDir, Math.sqrt(Math.max(0, 1 - Math.min(r2, 1))))),
  );
  const highlands = skFbm3(scale(n, 6.5), oct);
  const maria = smoothstep(0.44, 0.63, skFbm3(addS3(scale(n, 2.1), 5.0), Math.max(2, oct - 1)));
  const albedo = glMix(0.105, 0.155, highlands) * glMix(1.0, 0.52, maria);
  const nDl = Math.max(0, dot(n, sunDir));
  const shade = Math.pow(nDl, 0.42);
  const earthshine = 0.014;
  return scale(discRadiance, (albedo / 0.13) * (shade + earthshine) * cover);
}

/** skSample, dome.js:194-273. `u` bundles the uniforms `src/sky/dome.rs`'s
 *  `DomeUniforms` names; `skyViewLut`/`transmittanceLut` are `Lut2D`-shaped
 *  `{width,height,wrapS,data}` objects from `bakeSkyView`/`bakeTransmittance`;
 *  `ambient` is `bakeAmbient`'s two-texel `[texel0, texel1]`. */
function skSample(rayDir, quality, u, skyViewLut, transmittanceLut, ambient) {
  const ambSky = ambient[0], ambHor = ambient[1];

  let col = skyViewLookup(skyViewLut, rayDir, u.sunDir, u.viewPos);

  const cosS = dot(rayDir, u.sunDir);
  const cosM = dot(rayDir, u.moonDir);
  const thetaS = safeAcos(cosS);
  const thetaM = safeAcos(cosM);

  const transmittanceAt = (p, dir) => {
    const [uu, vv] = lutUv(p, dir);
    return lutSample(transmittanceLut, uu, vv);
  };

  const transAlongRay = transmittanceAt(u.viewPos, rayDir);
  col = add(col, skAureole(rayDir[1], u.sunIrradiance, transAlongRay, cosS, u.mieScale));
  col = add(col, skAureole(rayDir[1], u.moonIrradiance, transAlongRay, cosM, u.mieScale));

  const pLow = [0, ATMO.groundRadiusMM + 0.0015, 0];
  const pHigh = [0, ATMO.groundRadiusMM + 0.0078, 0];
  const sunLow = mul(u.sunIrradiance, transmittanceAt(pLow, u.sunDir));
  const sunHigh = mul(u.sunIrradiance, transmittanceAt(pHigh, u.sunDir));
  const moonLow = mul(u.moonIrradiance, transmittanceAt(pLow, u.moonDir));
  const moonHigh = mul(u.moonIrradiance, transmittanceAt(pHigh, u.moonDir));
  const cl = skClouds(rayDir, u.sunDir, sunLow, sunHigh, u.moonDir, moonLow, moonHigh, ambSky, quality, u.cloud, u.viewPos);

  const night = skNightSky(rayDir, quality > 0 ? 5 : 3, quality > 0, u.celestialRows, u.star);
  col = add(col, scale(night, 1 - Math.max(0, Math.min(1, cl.a * 1.9))));

  if (cl.a > 1e-4) {
    const bleed = 1 - smoothstep(0.0, 0.22, rayDir[1]);
    col = mixv3(col, mixv3(cl.rgb, col, bleed * 0.82), cl.a);
  }

  if (rayDir[1] < 0) {
    const ground = mul(
      u.groundAlbedo,
      add(add(ambHor, scale(u.sunIrradiance, Math.max(0, u.sunDir[1]) / Math.PI)), scale(u.moonIrradiance, Math.max(0, u.moonDir[1]) / Math.PI)),
    );
    col = mixv3(col, ground, smoothstep(0.0, -0.22, rayDir[1]));
  }

  const murk = u.horizonMurk * Math.exp(-Math.abs(rayDir[1]) * 26.0);
  col = mixv3(col, scale(ambHor, 1.15), Math.max(0, Math.min(0.85, murk)));

  col = skRolloffFn(col, u.rolloffKnee, u.rolloffExponent);

  if (quality > 0) {
    const transToSun = transmittanceAt(u.viewPos, u.sunDir);
    col = add(col, skSunDisc(thetaS, u.fwidthSunTheta, u.sunAngRadius, u.sunDrawScale, u.sunDiscRadiance, transToSun));
  }
  col = add(
    col,
    skMoonDisc(rayDir, thetaM, quality > 0 ? 4 : 2, u.moonDir, u.sunDir, u.moonAngRadius, u.moonDrawScale, u.fwidthMoonR2, u.moonDiscRadiance),
  );

  return vmax(col, [0, 0, 0]);
}

/* -- volumetrics.js: SHARED/MARCH_FRAG/COMPOSITE_FRAG, no oracle -------------- */

/** skFogAmbient, volumetrics.js:70-79. */
function skFogAmbient(cosKey, ambient, keyIrr) {
  const cool = ambient[0], hor = ambient[1];
  const maxC = Math.max(1e-4, Math.max(keyIrr[0], Math.max(keyIrr[1], keyIrr[2])));
  const keyHue = scale(keyIrr, 1 / maxC);
  const f = 0.5 + 0.5 * Math.max(-1, Math.min(1, cosKey));
  const warm = scale(mul(hor, mixv3([1, 1, 1], keyHue, 0.55)), 1.3);
  return mixv3(cool, warm, f * f);
}

/** skFogPhase, volumetrics.js:83-85. */
function skFogPhase(cosTheta, gFwd, gBack, backWeight) {
  return glMix(hgPhase(cosTheta, gFwd), hgPhase(cosTheta, gBack), backWeight);
}

/** skFogInscatterPhase, volumetrics.js:103-107. */
function skFogInscatterPhase(cosTheta, gFwd, gBack, backWeight, shaftGain) {
  const iso = 1 / (4 * Math.PI);
  const p = skFogPhase(cosTheta, gFwd, gBack, backWeight);
  return p + Math.max(0, p - iso) * (shaftGain - 1);
}

/** skFogNearRamp, volumetrics.js:118-120. */
function skFogNearRamp(t) {
  return smoothstep(0.0, 12.0, t);
}

/** skFogDensity, volumetrics.js:123-129. */
function skFogDensity(p, baseY, invHeightScale, noiseScale, drift, noiseAmount) {
  const h = Math.exp(-(p[1] - baseY) * invHeightScale);
  if (noiseAmount <= 0.001) return h;
  const q = add(scale(p, noiseScale), drift);
  const n = skVal3(q) * 0.63 + skVal3(add(scale(q, 2.71), [5.1, 5.1, 5.1])) * 0.37;
  return h * glMix(1.0, 0.30 + 1.55 * n, noiseAmount);
}

/** skHeightIntegral, volumetrics.js:136-141. */
function skHeightIntegral(y0, dy, t, baseY, invHeightScale) {
  const d0 = Math.exp(-(y0 - baseY) * invHeightScale);
  const x = dy * invHeightScale * t;
  if (Math.abs(x) < 1e-4) return d0 * t;
  return (d0 * (1 - Math.exp(-x))) / (dy * invHeightScale);
}

/** skVogel, volumetrics.js:163-167. */
function skVogel(i, n, phi) {
  const r = Math.sqrt((i + 0.5) / n);
  const theta = i * 2.39996323 + phi;
  return [Math.cos(theta) * r, Math.sin(theta) * r];
}

/** MARCH_FRAG's main(), minus screen-space ray reconstruction and the
 *  shadow-map texture read (see `src/sky/volumetrics.rs`'s module doc);
 *  `sunVisibility(worldPos, viewDepth, rot) -> [0,1]` stands in for
 *  `skSunVisibility`. Returns { L, T }. volumetrics.js:211-274. */
function raymarchFog(dir, rayLen, maxT, dith, camPos, u, cloud, steps, sunVisibility) {
  const cosKey = dot(dir, u.keyDir);
  const phase = skFogInscatterPhase(cosKey, u.gFwd, u.gBack, u.backWeight, u.shaftGain);
  const ambient = scale(skFogAmbient(cosKey, u.ambient, u.keyIrr), u.ambientBoost);

  const cloudNear = skCloudShadow([camPos[0], camPos[2]], u.keyDir, cloud);
  const farPos = add(camPos, scale(dir, maxT));
  const cloudFar = skCloudShadow([farPos[0], farPos[2]], u.keyDir, cloud);

  let l = [0, 0, 0];
  let tTrans = 1.0;
  let prev = 0.0;

  for (let i = 0; i < steps; i++) {
    const f = (i + dith) / steps;
    const t = maxT * f * f * (3 - 2 * f) * 0.35 + maxT * f * f * f * 0.65;
    const dt = t - prev;
    prev = t;
    if (dt <= 1e-5) continue;

    const wp = add(camPos, scale(dir, t));
    const dens = skFogDensity(wp, u.baseY, u.invHeightScale, u.noiseScale, u.fogDrift, u.noiseAmount);
    if (dens <= 1e-4) continue;

    const sigmaS = u.sigmaS * dens * skFogNearRamp(t);
    const sigmaE = Math.max(1e-7, u.sigmaE * dens);

    let vis = sunVisibility(wp, t / rayLen, dith);
    vis *= glMix(cloudNear, cloudFar, f);

    const ambOcc = 0.42 + 0.58 * vis;
    const j = add(scale(u.keyIrr, vis * phase), scale(ambient, ambOcc));

    const aT = Math.exp(-sigmaE * dt);
    const contrib = scale(scale(scale(scale(j, tTrans), sigmaS), 1 - aT), 1 / sigmaE);
    l = add(l, contrib);
    tTrans *= aT;
    if (tTrans < 0.004) break;
  }

  return { L: l, T: tTrans };
}

/** COMPOSITE_FRAG's VOL_ANALYTIC branch, volumetrics.js:367-388. */
function compositeAnalytic(color, dir, dist, camPosY, fogExt, u) {
  const od = skHeightIntegral(camPosY, dir[1], dist, u.baseY, u.invHeightScale);
  const trans = vexp(scale(fogExt, -od));

  const odNear = skHeightIntegral(camPosY, dir[1], Math.min(dist, 12.0), u.baseY, u.invHeightScale);
  const odS = Math.max(0, od - odNear * 0.5);
  const mono = 1 - Math.exp(-u.sigmaE * odS);
  const cosKey = dot(dir, u.keyDir);
  const inscatter = scale(
    add(
      scale(u.keyIrr, skFogInscatterPhase(cosKey, u.gFwd, u.gBack, u.backWeight, u.shaftGain) * 0.55),
      scale(skFogAmbient(cosKey, u.ambient, u.keyIrr), u.ambientBoost),
    ),
    (u.sigmaS / Math.max(1e-6, u.sigmaE)) * mono,
  );

  return add(mul(color, trans), inscatter);
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
