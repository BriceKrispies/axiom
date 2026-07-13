/*
 * mat4.ts — minimal column-major 4×4 matrix math powering the WebGL2
 * renderer. Pure functions over `Float32Array` (16 floats, column-major, i.e.
 * `m[col * 4 + row]`), no DOM, no imports beyond the engine contract's vector
 * and quaternion types. Conventions: right-handed world/view space, the camera
 * looks down -Z in view space, and `perspective` produces WebGL clip depth
 * (-1..1 NDC). `fromTrs` composes the standard T·R·S model matrix (scale first,
 * then rotate, then translate), matching the renderer's `Transform` semantics.
 *
 * The math is branchless (an engine-spine invariant): degenerate-input guards
 * that were ternaries are arithmetic blends, and the matrix products are
 * combinator folds over index ranges rather than nested loops.
 */

import type { EngineQuat, EngineVec3 } from "./api.ts";

/** A column-major 4×4 matrix: `m[col * 4 + row]`. */
export type Mat4 = Float32Array;

const EPS = 1e-9;
const HALF = 0.5;
const TWO = 2;
const THREE = 3;
/** The matrix is `ORDER`×`ORDER`, `ENTRIES` floats in column-major order. */
const ORDER = 4;
const ENTRIES = ORDER * ORDER;

/** `[0, 1, …, count - 1]`, a branchless counting-index list that replaces a loop. */
const range = (count: number): number[] => Array.from({ length: count }, (value, index) => index);

/** The `[0, 1, 2, 3]` lane indices reused by the matrix folds. */
const LANES = range(ORDER);

/** Read a matrix entry as a definite `number` (indices are always in range;
 * `Number` coerces the `noUncheckedIndexedAccess` `| undefined` away without a
 * non-null assertion). */
const entry = (mat: Mat4, idx: number): number => Number(mat[idx]);

// ── small internal vector helpers (self-contained on purpose) ─────────────────

const v3 = (x: number, y: number, z: number): EngineVec3 => ({ x, y, z });

const sub3 = (lhs: EngineVec3, rhs: EngineVec3): EngineVec3 => v3(lhs.x - rhs.x, lhs.y - rhs.y, lhs.z - rhs.z);

const dot3 = (lhs: EngineVec3, rhs: EngineVec3): number => lhs.x * rhs.x + lhs.y * rhs.y + lhs.z * rhs.z;

const cross3 = (lhs: EngineVec3, rhs: EngineVec3): EngineVec3 =>
  v3(lhs.y * rhs.z - lhs.z * rhs.y, lhs.z * rhs.x - lhs.x * rhs.z, lhs.x * rhs.y - lhs.y * rhs.x);

/**
 * Normalize with a divide-by-zero guard; degenerate input (length below `EPS`)
 * falls back to `fallback`. Branchless: `degenerate` is a 0/1 selector, and the
 * denominator gains `degenerate` so the fallback arm never divides by zero (no
 * `0 · Infinity` NaN), then the two arms are blended.
 */
const normalize3 = (vec: EngineVec3, fallback: EngineVec3): EngineVec3 => {
  const len = Math.sqrt(dot3(vec, vec));
  const degenerate = Number(len < EPS);
  const keep = 1 - degenerate;
  const safeLen = len + degenerate;
  return v3(
    degenerate * fallback.x + keep * (vec.x / safeLen),
    degenerate * fallback.y + keep * (vec.y / safeLen),
    degenerate * fallback.z + keep * (vec.z / safeLen),
  );
};

/** Replace `value` with `EPS`-signed `safe` when its magnitude is below `EPS`,
 * otherwise keep it — a branchless magnitude guard that replaces the ternary,
 * blending the safe value and the real one by a 0/1 selector. */
const guardMagnitude = (value: number, safe: number): number => {
  const degenerate = Number(Math.abs(value) < EPS);
  return degenerate * safe + (1 - degenerate) * value;
};

// ── constructors ──────────────────────────────────────────────────────────────

/** The 4×4 identity matrix (1 on the diagonal, where `idx % (ORDER + 1) === 0`). */
export const identity = (): Mat4 => new Float32Array(range(ENTRIES).map((idx) => Number(idx % (ORDER + 1) === 0)));

/**
 * A right-handed perspective projection with WebGL clip conventions (NDC depth
 * -1 at `near`, +1 at `far`). `fovY` is the vertical field of view in radians.
 * Degenerate inputs (zero aspect, fovY, or near == far) are guarded so the
 * result is finite rather than NaN.
 */
export const perspective = (fovY: number, aspect: number, near: number, far: number): Mat4 => {
  const focal = 1 / guardMagnitude(Math.tan(fovY * HALF), EPS);
  const aspectSafe = guardMagnitude(aspect, EPS);
  const depthDenom = guardMagnitude(near - far, -EPS);
  return new Float32Array([
    focal / aspectSafe, 0, 0, 0,
    0, focal, 0, 0,
    0, 0, (far + near) / depthDenom, -1,
    0, 0, (TWO * far * near) / depthDenom, 0,
  ]);
};

/**
 * A right-handed view matrix: the camera sits at `eye` looking at `target`,
 * with `up` steadying the roll. In the resulting view space the camera looks
 * down -Z (so `target` maps onto the negative Z axis).
 */
export const lookAt = (eye: EngineVec3, target: EngineVec3, up: EngineVec3): Mat4 => {
  const fwd = normalize3(sub3(target, eye), v3(0, 0, -1));
  const side = normalize3(cross3(fwd, up), v3(1, 0, 0));
  const upOrtho = cross3(side, fwd);
  return new Float32Array([
    side.x, upOrtho.x, -fwd.x, 0,
    side.y, upOrtho.y, -fwd.y, 0,
    side.z, upOrtho.z, -fwd.z, 0,
    -dot3(side, eye), -dot3(upOrtho, eye), dot3(fwd, eye), 1,
  ]);
};

/** Column-major matrix product `a · b` (apply `b` first, then `a`): each of the
 * `ENTRIES` outputs folds the dot product of a row of `a` with a column of `b`. */
export const multiply = (lhs: Mat4, rhs: Mat4): Mat4 =>
  new Float32Array(
    range(ENTRIES).map((idx) => {
      const col = Math.floor(idx / ORDER);
      const row = idx % ORDER;
      return LANES.reduce((sum, lane) => sum + entry(lhs, lane * ORDER + row) * entry(rhs, col * ORDER + lane), 0);
    }),
  );

/**
 * The standard model matrix T·R·S: scale in local space, then rotate by the
 * unit quaternion `[x, y, z, w]`, then translate — so the origin always lands
 * exactly on `position`.
 */
export const fromTrs = (position: EngineVec3, rotation: EngineQuat, scale: EngineVec3): Mat4 => {
  const [x, y, z, qw] = rotation;
  const r00 = 1 - TWO * (y * y + z * z);
  const r01 = TWO * (x * y - z * qw);
  const r02 = TWO * (x * z + y * qw);
  const r10 = TWO * (x * y + z * qw);
  const r11 = 1 - TWO * (x * x + z * z);
  const r12 = TWO * (y * z - x * qw);
  const r20 = TWO * (x * z - y * qw);
  const r21 = TWO * (y * z + x * qw);
  const r22 = 1 - TWO * (x * x + y * y);
  return new Float32Array([
    r00 * scale.x, r10 * scale.x, r20 * scale.x, 0,
    r01 * scale.y, r11 * scale.y, r21 * scale.y, 0,
    r02 * scale.z, r12 * scale.z, r22 * scale.z, 0,
    position.x, position.y, position.z, 1,
  ]);
};

/**
 * Transform a point (w = 1) by `mat` with perspective divide, guarding a
 * near-zero clip-space w. Used by tests and any CPU-side projection math. Each
 * output component folds a row of `mat` against the homogeneous point.
 */
export const transformPoint = (mat: Mat4, point: EngineVec3): EngineVec3 => {
  const coords = [point.x, point.y, point.z, 1];
  const component = (row: number): number =>
    coords.reduce((sum, coord, col) => sum + entry(mat, col * ORDER + row) * coord, 0);
  const x = component(0);
  const y = component(1);
  const z = component(TWO);
  const wClip = component(THREE);
  // Guard a near-zero w: fall back to inv = 1 (branchless blend, safe divisor).
  const degenerate = Number(Math.abs(wClip) < EPS);
  const keep = 1 - degenerate;
  const wSafe = degenerate + keep * wClip;
  const inv = degenerate + keep * (1 / wSafe);
  return v3(x * inv, y * inv, z * inv);
};
