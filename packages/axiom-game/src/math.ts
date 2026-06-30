/*
 * The pure scalar-math + 2D-vector + predicate free functions (SPEC-03 §4.2 /
 * §5). EVERY operation routes to the native `MathApi` through the installed
 * `HostBridge` (`host-binding.ts`) — the one deterministic source of truth
 * (SPEC-03 §3.2/§3.4), never re-implemented in TS. The stateful scene queries
 * (`overlapCircle`/`overlapBox`/`raycast`) live in `query.ts`, beside this scalar
 * surface.
 *
 * `lerp` is native too (not a local TS blend): a sim-class value must come from
 * the authoritative `MathApi::lerp`, the same crossing `clamp` / `normalizeAngle`
 * already pay, so a replay re-derives byte-identical results regardless of where
 * the blend is computed.
 *
 * The `v2` vector namespace mirrors `math3d.ts`'s `v3` (a frozen record of thin
 * forwarders), and the three pure predicates back onto the native `Aabb` /
 * `Sphere`. `circleOverlap` takes two `Circle` records rather than the contract's
 * flat `(aCenter, aR, bCenter, bR)` so the call stays within the SDK's
 * ≤3-parameter law — the same record-bundling `mat4Perspective(spec)` uses; the
 * geometry is unchanged.
 */

import type { Circle, Rect, Vec2 } from "./vocabulary.ts";
import { boundHost } from "./host-binding.ts";

/** Constrain `value` to `[low, high]` (native `MathApi`, SPEC-03 §4.2). */
export const clamp = (value: number, low: number, high: number): number =>
  boundHost().clamp(value, low, high);

/** Linear blend from `start` to `end` by `fraction` (native `MathApi`, SPEC-03 §4.2). */
export const lerp = (start: number, end: number, fraction: number): number =>
  boundHost().lerp(start, end, fraction);

/** Wrap `angle` to `(-π, π]` (native `MathApi`, SPEC-03 §4.2). */
export const normalizeAngle = (angle: number): number => boundHost().normalizeAngle(angle);

/** 2D vector algebra projected from the native `MathApi` (SPEC-03 §4.2). */
export const v2 = {
  /** `lhs + rhs`. */
  add: (lhs: Vec2, rhs: Vec2): Vec2 => boundHost().v2Add(lhs, rhs),
  /** The distance between `lhs` and `rhs`. */
  dist: (lhs: Vec2, rhs: Vec2): number => boundHost().v2Dist(lhs, rhs),
  /** `lhs · rhs` (dot product). */
  dot: (lhs: Vec2, rhs: Vec2): number => boundHost().v2Dot(lhs, rhs),
  /** The Euclidean length of `vector`. */
  len: (vector: Vec2): number => boundHost().v2Len(vector),
  /** The linear blend from `lhs` to `rhs` by `fraction`. */
  lerp: (lhs: Vec2, rhs: Vec2, fraction: number): Vec2 => boundHost().v2Lerp(lhs, rhs, fraction),
  /** The unit vector in the direction of `vector`. */
  normalize: (vector: Vec2): Vec2 => boundHost().v2Normalize(vector),
  /** `vector * scalar`. */
  scale: (vector: Vec2, scalar: number): Vec2 => boundHost().v2Scale(vector, scalar),
  /** `lhs - rhs`. */
  sub: (lhs: Vec2, rhs: Vec2): Vec2 => boundHost().v2Sub(lhs, rhs),
} as const;

/** Whether rects `lhs` and `rhs` share any point (native `Aabb`, SPEC-03 §4.2). */
export const aabbOverlap = (lhs: Rect, rhs: Rect): boolean => boundHost().aabbOverlap(lhs, rhs);

/** Whether `point` lies inside `rect` (native `Aabb`, SPEC-03 §4.2). */
export const pointInRect = (point: Vec2, rect: Rect): boolean => boundHost().pointInRect(point, rect);

/** Whether circles `lhs` and `rhs` share any point (native `Sphere`, SPEC-03 §4.2). */
export const circleOverlap = (lhs: Circle, rhs: Circle): boolean => boundHost().circleOverlap(lhs, rhs);
