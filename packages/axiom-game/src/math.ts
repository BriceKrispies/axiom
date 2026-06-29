/*
 * The math and spatial-query free functions (SPEC-03 §4.2). Anything the native
 * core owns is projected through the installed `HostBridge` (`host-binding.ts`):
 * `clamp` and `normalizeAngle` come from the native `MathApi`, and
 * `overlapCircle` is a scene query over the committed transforms for the current
 * tick — none of these are re-implemented in TS where a native source exists.
 *
 * `lerp` is the one local helper: a single bit-trivial affine blend
 * (`start + (end - start) * t`) with no native state to consult, so it stays in
 * the TS layer rather than paying a bridge crossing.
 */

import type { Entity, Vec2 } from "./vocabulary.ts";
import { boundHost } from "./host-binding.ts";

/** Constrain `value` to `[low, high]` (native `MathApi`, SPEC-03 §4.2). */
export const clamp = (value: number, low: number, high: number): number =>
  boundHost().clamp(value, low, high);

/** Linear blend from `start` to `end` by `fraction` — local, bit-trivial (SPEC-03 §4.2). */
export const lerp = (start: number, end: number, fraction: number): number =>
  start + (end - start) * fraction;

/** Wrap `angle` to `(-π, π]` (native `MathApi`, SPEC-03 §4.2). */
export const normalizeAngle = (angle: number): number => boundHost().normalizeAngle(angle);

/** Entities whose committed transform overlaps the circle, in stable order (SPEC-03 §4.2). */
export const overlapCircle = (center: Vec2, radius: number): readonly Entity[] =>
  boundHost().overlapCircle(center.x, center.y, radius);
