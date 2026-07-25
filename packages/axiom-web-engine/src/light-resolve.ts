/*
 * Light resolution: turning an AUTHORED light spec into the flat per-frame light
 * a backend consumes.
 *
 * `store.ts` retains lights as the specs the app wrote (so they stay re-posable
 * via `setLight`); this module owns the conversion applied on the way into a
 * frame — folding intensity into the color, normalizing a direction, flattening
 * a position vector. Pure and singleton-free, so each rule is testable without a
 * backend or a store.
 */

import type { FrameDirLight, FramePointLight } from "./backend.ts";
import type { Light } from "./api.ts";
import { select } from "./branchless.ts";

/** Color · intensity, resolved to a plain RGB triple for the frame. */
export type Rgb = readonly [number, number, number];

/** Below this length a direction vector carries no usable orientation. */
const DIRECTION_EPSILON = 1e-9;

export const isDirectional = (light: Light): light is Extract<Light, { kind: "directional" }> =>
  light.kind === "directional";

export const isPoint = (light: Light): light is Extract<Light, { kind: "point" }> => light.kind === "point";

/** Color · intensity, resolved to the plain RGB triple a frame light carries. */
export const litColor = (light: Light): Rgb => {
  const [cr, cg, cb] = light.color;
  return [cr * light.intensity, cg * light.intensity, cb * light.intensity];
};

/** Normalize the direction and fold in intensity. A degenerate (zero-length)
 * direction resolves to straight down rather than NaN, so a light authored with
 * no direction still renders as a sane overhead key. */
export const resolveDirLight = (light: Extract<Light, { kind: "directional" }>): FrameDirLight => {
  const dir = light.direction;
  const len = Math.hypot(dir.x, dir.y, dir.z);
  const tiny = len < DIRECTION_EPSILON;
  const inv = select(tiny, 0, 1 / len);
  return { color: litColor(light), direction: [dir.x * inv, select(tiny, -1, dir.y * inv), dir.z * inv] };
};

export const resolvePointLight = (light: Extract<Light, { kind: "point" }>): FramePointLight => {
  const pos = light.position;
  return { color: litColor(light), position: [pos.x, pos.y, pos.z] };
};
