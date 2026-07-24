/*
 * shading.ts — the pure per-fragment shading term shared by both drawing backends
 * (backend-webgl2.ts and backend-canvas2d.ts). It evaluates the SAME lighting a
 * fragment shader would and is the SOFTWARE TRUTH the GLSL twin is byte-matched
 * against (backend-webgl2.ts, enforced by shading.test.ts).
 *
 * `shadeSurface` returns TWO neutral buckets for one lit point:
 *   - `diffuse` = ambient floor + Σ directional (N·L, clamped) + Σ point (N·L with
 *     the soft 1/(1 + 0.08·d²) falloff). This bucket is tinted by the material
 *     albedo and attenuated by ambient occlusion in the backend.
 *   - `specular` = a WHITE (albedo-independent) Blinn-Phong lobe per light, driven
 *     by the material roughness (glossiness = 1 − roughness) and the eye vector,
 *     plus a subtle Schlick Fresnel rim that brightens grazing edges. Both scale
 *     by glossiness, so a fully-rough (default) material contributes ZERO specular
 *     and the shading collapses to the historical Lambert term.
 *
 * `tonemap` is the highlight-rolloff curve applied to the final composited color:
 * exact identity below the knee, a smooth Reinhard shoulder above it, bounded so
 * over-driven lights compress into range instead of hard-clipping to white.
 *
 * The whole file is branchless (an engine-spine invariant): light-list walks are
 * `.reduce` folds, and every selection is arithmetic (min/max/sign), never an
 * `if`/`?:`.
 */

import { AMBIENT, type FrameDirLight, type FramePointLight, type SceneFrame } from "./backend.ts";

/** A plain 3-vector: a normal, a direction, a position, or a per-channel color. */
type Vec3 = readonly [number, number, number];

/** A linear RGB triple; each channel is an unbounded (0..∞) accumulated value. */
type Rgb = readonly [number, number, number];

/** The two neutral shading buckets for one surface point. `diffuse` is
 * albedo-tinted + AO-attenuated downstream; `specular` is added neutrally. */
export interface SurfaceShading {
  readonly diffuse: Rgb;
  readonly specular: Rgb;
}

/** Point-light falloff coefficient: intensity scales by 1/(1 + FALLOFF·d²). */
const FALLOFF = 0.08;

/** Distance floor so a light (or a degenerate half-vector) never divides by zero. */
const MIN_DISTANCE = 1e-5;

/** Blinn-Phong exponent range, interpolated by glossiness (1 − roughness): a
 * broad lobe near matte, a tight mirror-like highlight at gloss 1. */
const SHINE_MIN = 8;
const SHINE_MAX = 128;

/** Schlick Fresnel: dielectric base reflectance, rim strength, and the classic
 * 5th-power falloff. The rim is scaled by glossiness so it too vanishes at the
 * fully-rough default (existing matte materials get no rim). */
const FRESNEL_F0 = 0.04;
const FRESNEL_GAIN = 0.5;
const FRESNEL_POWER = 5;

/** Highlight-rolloff knee: the curve is EXACT identity on [0, KNEE] and applies a
 * Reinhard shoulder above it, mapping (KNEE, ∞) into (KNEE, 1). Chosen high so
 * content that stays in range is visually unchanged; see the module note. */
const TONEMAP_KNEE = 0.9;

/** Dot product of two 3-vectors. */
const dot = ([ax, ay, az]: Vec3, [bx, by, bz]: Vec3): number => ax * bx + ay * by + az * bz;

/** Component-wise difference `lhs − rhs`. */
const sub = ([ax, ay, az]: Vec3, [bx, by, bz]: Vec3): Vec3 => [ax - bx, ay - by, az - bz];

/** Component-wise sum. */
const add = ([ax, ay, az]: Vec3, [bx, by, bz]: Vec3): Vec3 => [ax + bx, ay + by, az + bz];

/** Component-wise scale. */
const scale = ([ax, ay, az]: Vec3, factor: number): Vec3 => [ax * factor, ay * factor, az * factor];

/** Squared length. Kept as a helper so `Math.sqrt` sees only a scalar: the exact
 * same value as an inline sum of squares (Math.hypot would round differently). */
const lengthSquared = (vec: Vec3): number => dot(vec, vec);

/** Unit vector, with the same MIN_DISTANCE floor the GLSL twin uses, so a zero
 * or near-zero input never yields a NaN. */
const normalize = (vec: Vec3): Vec3 => scale(vec, 1 / Math.max(Math.sqrt(lengthSquared(vec)), MIN_DISTANCE));

/** 1 when `value > 0`, else 0 — the branchless `step`-like facing gate. Matches
 * the GLSL `max(sign(x), 0.0)` exactly (both give 0 at x = 0). */
const positiveGate = (value: number): number => Math.max(Math.sign(value), 0);

/** Blinn-Phong lobe: max(N·H, 0)^shininess for the half-vector H of the unit
 * light and eye directions. Gated to the lit hemisphere by the caller. */
const blinnLobe = (normal: Vec3, toLight: Vec3, toEye: Vec3, shininess: number): number =>
  Math.max(dot(normal, normalize(add(toLight, toEye))), 0) ** shininess;

/** A directional light's neutral diffuse + specular contribution. */
const dirContribution = (light: FrameDirLight, normal: Vec3, toEye: Vec3, gloss: number, shininess: number): SurfaceShading => {
  const toLight = sub([0, 0, 0], light.direction);
  const ndl = dot(normal, toLight);
  const specular = blinnLobe(normal, toLight, toEye, shininess) * gloss * positiveGate(ndl);
  return { diffuse: scale(light.color, Math.max(0, ndl)), specular: scale(light.color, specular) };
};

/** A point light's neutral diffuse + specular contribution: N·L with the soft
 * 1/(1 + 0.08·d²) falloff on both, and the specular gated to the lit hemisphere. */
const pointContribution = (
  light: FramePointLight,
  normal: Vec3,
  surface: Vec3,
  toEye: Vec3,
  gloss: number,
  shininess: number,
): SurfaceShading => {
  const offset = sub(light.position, surface);
  const dist = Math.sqrt(lengthSquared(offset));
  const attenuation = 1 / (1 + FALLOFF * dist * dist);
  const ndl = dot(normal, offset) / Math.max(dist, MIN_DISTANCE);
  const specular = blinnLobe(normal, normalize(offset), toEye, shininess) * gloss * positiveGate(ndl) * attenuation;
  return { diffuse: scale(light.color, Math.max(0, ndl) * attenuation), specular: scale(light.color, specular) };
};

// The neutral return of a `.map` used purely to iterate: the branchless spine
// has no `for`/`forEach`/`reduce`, so each light-list walk is a `.map` whose
// numeric result is discarded (array-callback-return still wants a return value).
const ITERATE = 0;

/**
 * Evaluate the shared per-fragment shading at a surface point, returning the
 * albedo-tinted `diffuse` bucket and the neutral `specular` bucket (Blinn-Phong
 * lobes per light + a Schlick Fresnel rim). `(nx, ny, nz)` is the unit surface
 * normal, `(px, py, pz)` the world position, `(ex, ey, ez)` the eye/camera world
 * position, and `roughness ∈ [0, 1]` the material roughness (1 = matte, specular
 * off). Both light lists fold in array order into the same running sums, so the
 * result is byte-identical to the shader's.
 */
export const shadeSurface = (
  nx: number,
  ny: number,
  nz: number,
  px: number,
  py: number,
  pz: number,
  ex: number,
  ey: number,
  ez: number,
  roughness: number,
  frame: Pick<SceneFrame, "dirLights" | "pointLights">,
): SurfaceShading => {
  const normal: Vec3 = [nx, ny, nz];
  const surface: Vec3 = [px, py, pz];
  const gloss = Math.min(Math.max(1 - roughness, 0), 1);
  const shininess = SHINE_MIN + gloss * (SHINE_MAX - SHINE_MIN);
  const toEye = normalize(sub([ex, ey, ez], surface));
  const ndv = Math.max(dot(normal, toEye), 0);
  // Schlick Fresnel rim (neutral): brightens grazing edges, scaled by gloss.
  const rim = (1 - FRESNEL_F0) * (1 - ndv) ** FRESNEL_POWER * gloss * FRESNEL_GAIN;
  let diffR = AMBIENT;
  let diffG = AMBIENT;
  let diffB = AMBIENT;
  let specR = rim;
  let specG = rim;
  let specB = rim;
  const accumulate = (part: SurfaceShading): number => {
    const [pdr, pdg, pdb] = part.diffuse;
    const [psr, psg, psb] = part.specular;
    diffR += pdr;
    diffG += pdg;
    diffB += pdb;
    specR += psr;
    specG += psg;
    specB += psb;
    return ITERATE;
  };
  frame.dirLights.map((light): number => accumulate(dirContribution(light, normal, toEye, gloss, shininess)));
  frame.pointLights.map((light): number =>
    accumulate(pointContribution(light, normal, surface, toEye, gloss, shininess)),
  );
  return { diffuse: [diffR, diffG, diffB], specular: [specR, specG, specB] };
};

/**
 * The DIFFUSE-only shade (ambient floor + Σ directional N·L + Σ point N·L with
 * the soft falloff) — the COMPLETE shade for a matte material, whose specular and
 * Fresnel are identically zero. Byte-identical to `shadeSurface(...).diffuse` (the
 * diffuse bucket doesn't depend on roughness). The software backend calls this for
 * matte materials so a per-triangle flat shade skips the wasted specular math
 * (no eye vector, no Blinn-Phong lobe, no Fresnel rim). Branchless like the rest.
 */
export const diffuseOnly = (
  nx: number,
  ny: number,
  nz: number,
  px: number,
  py: number,
  pz: number,
  frame: Pick<SceneFrame, "dirLights" | "pointLights">,
): Rgb => {
  const normal: Vec3 = [nx, ny, nz];
  const surface: Vec3 = [px, py, pz];
  let diffR = AMBIENT;
  let diffG = AMBIENT;
  let diffB = AMBIENT;
  const accumulate = ([pr, pg, pb]: Rgb): number => {
    diffR += pr;
    diffG += pg;
    diffB += pb;
    return ITERATE;
  };
  frame.dirLights.map((light): number =>
    accumulate(scale(light.color, Math.max(0, dot(normal, sub([0, 0, 0], light.direction))))),
  );
  frame.pointLights.map((light): number => {
    const offset = sub(light.position, surface);
    const dist = Math.sqrt(lengthSquared(offset));
    const attenuation = 1 / (1 + FALLOFF * dist * dist);
    const ndl = dot(normal, offset) / Math.max(dist, MIN_DISTANCE);
    return accumulate(scale(light.color, Math.max(0, ndl) * attenuation));
  });
  return [diffR, diffG, diffB];
};

/**
 * Highlight-rolloff tone curve for one linear channel. Exact identity on
 * [0, KNEE]; above the knee a Reinhard shoulder `x/(1+x)` maps the excess into the
 * remaining (KNEE, 1) headroom, C¹-continuous at the knee and bounded below 1 so
 * over-driven values compress smoothly instead of clipping flat to white.
 */
export const tonemap = (linear: number): number => {
  const low = Math.min(linear, TONEMAP_KNEE);
  const excess = Math.max(linear - TONEMAP_KNEE, 0) / (1 - TONEMAP_KNEE);
  return low + (1 - TONEMAP_KNEE) * (excess / (1 + excess));
};
