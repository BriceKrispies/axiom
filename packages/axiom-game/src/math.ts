/*
 * The pure scalar-math free functions (SPEC-03 §4.2 / §5). `clamp` and
 * `normalizeAngle` come from the native `MathApi` through the installed
 * `HostBridge` (`host-binding.ts`) — the one deterministic source of truth, never
 * re-implemented in TS. The stateful scene queries (`overlapCircle`/`overlapBox`/
 * `raycast`) live in `query.ts`, beside this scalar surface.
 *
 * `lerp` is the one local helper: a single bit-trivial affine blend
 * (`start + (end - start) * t`) with no native state to consult, so it stays in
 * the TS layer rather than paying a bridge crossing. (The native `MathApi`
 * exposes no standalone scalar `lerp` export today — only `clamp` /
 * `normalizeAngle` — so routing it native would need a new Wave-2 export; see the
 * agent report's gap note.)
 */

import { boundHost } from "./host-binding.ts";

/** Constrain `value` to `[low, high]` (native `MathApi`, SPEC-03 §4.2). */
export const clamp = (value: number, low: number, high: number): number =>
  boundHost().clamp(value, low, high);

/** Linear blend from `start` to `end` by `fraction` — local, bit-trivial (SPEC-03 §4.2). */
export const lerp = (start: number, end: number, fraction: number): number =>
  start + (end - start) * fraction;

/** Wrap `angle` to `(-π, π]` (native `MathApi`, SPEC-03 §4.2). */
export const normalizeAngle = (angle: number): number => boundHost().normalizeAngle(angle);
