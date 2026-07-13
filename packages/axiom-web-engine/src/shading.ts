/*
 * shading.ts — the pure Lambert shading term shared by both drawing backends
 * (backend-webgl2.ts and backend-canvas2d.ts). It evaluates the SAME lighting a
 * fragment shader would: an ambient floor plus every directional light (N·L,
 * clamped at zero) plus every point light (N·L with a soft 1/(1 + k·d²) distance
 * falloff). Extracted here as a branchless, fully-tested spine unit so the
 * hardware and software paths provably agree — the software rasterizer lights a
 * triangle once at its centroid with exactly this function, and the two backends
 * are held to matching output by shading.test.ts.
 */

import { AMBIENT, type SceneFrame } from "./backend.ts";

/** A plain 3-vector: a normal, a direction, a position, or a per-channel color. */
type Vec3 = readonly [number, number, number];

/** A linear RGB triple; each channel is an unbounded (0..∞) accumulated value. */
type Rgb = readonly [number, number, number];

/** Point-light falloff coefficient: intensity scales by 1/(1 + FALLOFF·d²). */
const FALLOFF = 0.08;

/** Distance floor so a light coincident with the surface never divides by zero. */
const MIN_DISTANCE = 1e-5;

/** Dot product of two 3-vectors. */
const dot = ([ax, ay, az]: Vec3, [bx, by, bz]: Vec3): number => ax * bx + ay * by + az * bz;

/** Component-wise difference `lhs − rhs`. */
const sub = ([ax, ay, az]: Vec3, [bx, by, bz]: Vec3): Vec3 => [ax - bx, ay - by, az - bz];

/** Squared length. Kept as a helper so `Math.sqrt` sees only a scalar: the exact
 * same value as an inline sum of squares (Math.hypot would round differently). */
const lengthSquared = (vec: Vec3): number => dot(vec, vec);

/** A point light's Lambert term at unit normal `normal` for the surface→light
 * offset `offset`: N·L clamped at zero, with the soft 1/(1 + 0.08·d²) falloff. */
const pointTerm = (normal: Vec3, offset: Vec3): number => {
  const dist = Math.sqrt(lengthSquared(offset));
  const inv = 1 / Math.max(dist, MIN_DISTANCE);
  return Math.max(0, dot(normal, offset) * inv) / (1 + FALLOFF * dist * dist);
};

// The neutral return of a `.map` used purely to iterate: the branchless spine
// has no `for`/`forEach`, so each light-list walk is a `.map` whose numeric
// result is discarded (array-callback-return still wants a return value).
const ITERATE = 0;

/**
 * The shared Lambert term at a surface point: ambient + Σ directional + Σ point
 * (soft 1/(1 + 0.08·d²) falloff), the exact per-fragment math evaluated once.
 * `(nx, ny, nz)` is the unit surface normal, `(px, py, pz)` the world position.
 * Each light-list `.map` accumulates in array order into the same running
 * per-channel sums, so the result is byte-identical to the shader's.
 */
export const lambertLight = (
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
  let red = AMBIENT;
  let green = AMBIENT;
  let blue = AMBIENT;
  frame.dirLights.map((light): number => {
    const [cr, cg, cb] = light.color;
    const lambert = Math.max(0, -dot(normal, light.direction));
    red += lambert * cr;
    green += lambert * cg;
    blue += lambert * cb;
    return ITERATE;
  });
  frame.pointLights.map((light): number => {
    const [cr, cg, cb] = light.color;
    const lambert = pointTerm(normal, sub(light.position, surface));
    red += lambert * cr;
    green += lambert * cg;
    blue += lambert * cb;
    return ITERATE;
  });
  return [red, green, blue];
};
