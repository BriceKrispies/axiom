/*
 * engine/mat4.ts — minimal column-major 4×4 matrix math for the in-app WebGL2
 * renderer. Pure functions over `Float32Array` (16 floats, column-major, i.e.
 * `m[col * 4 + row]`), no DOM, no imports beyond the engine contract's vector
 * and quaternion types. Conventions: right-handed world/view space, the camera
 * looks down -Z in view space, and `perspective` produces WebGL clip depth
 * (-1..1 NDC). `fromTrs` composes the standard T·R·S model matrix (scale first,
 * then rotate, then translate), matching the renderer's `Transform` semantics.
 */

import type { EngineQuat, EngineVec3 } from "./api.ts";

/** A column-major 4×4 matrix: `m[col * 4 + row]`. */
export type Mat4 = Float32Array;

const EPS = 1e-9;

// ── small internal vector helpers (self-contained on purpose) ─────────────────

const v3 = (x: number, y: number, z: number): EngineVec3 => ({ x, y, z });

const sub3 = (a: EngineVec3, b: EngineVec3): EngineVec3 => v3(a.x - b.x, a.y - b.y, a.z - b.z);

const dot3 = (a: EngineVec3, b: EngineVec3): number => a.x * b.x + a.y * b.y + a.z * b.z;

const cross3 = (a: EngineVec3, b: EngineVec3): EngineVec3 =>
  v3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);

/** Normalize with a divide-by-zero guard; degenerate input falls back to `fallback`. */
const normalize3 = (a: EngineVec3, fallback: EngineVec3): EngineVec3 => {
  const len = Math.sqrt(dot3(a, a));
  return len < EPS ? fallback : v3(a.x / len, a.y / len, a.z / len);
};

// ── constructors ──────────────────────────────────────────────────────────────

/** The 4×4 identity matrix. */
export const identity = (): Mat4 => {
  const m = new Float32Array(16);
  m[0] = 1;
  m[5] = 1;
  m[10] = 1;
  m[15] = 1;
  return m;
};

/**
 * A right-handed perspective projection with WebGL clip conventions (NDC depth
 * -1 at `near`, +1 at `far`). `fovY` is the vertical field of view in radians.
 * Degenerate inputs (zero aspect, fovY, or near == far) are guarded so the
 * result is finite rather than NaN.
 */
export const perspective = (fovY: number, aspect: number, near: number, far: number): Mat4 => {
  const t = Math.tan(fovY * 0.5);
  const f = 1 / (Math.abs(t) < EPS ? EPS : t);
  const a = Math.abs(aspect) < EPS ? EPS : aspect;
  const nf = Math.abs(near - far) < EPS ? -EPS : near - far;
  const m = new Float32Array(16);
  m[0] = f / a;
  m[5] = f;
  m[10] = (far + near) / nf;
  m[11] = -1;
  m[14] = (2 * far * near) / nf;
  return m;
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
  const m = new Float32Array(16);
  m[0] = side.x;
  m[1] = upOrtho.x;
  m[2] = -fwd.x;
  m[4] = side.y;
  m[5] = upOrtho.y;
  m[6] = -fwd.y;
  m[8] = side.z;
  m[9] = upOrtho.z;
  m[10] = -fwd.z;
  m[12] = -dot3(side, eye);
  m[13] = -dot3(upOrtho, eye);
  m[14] = dot3(fwd, eye);
  m[15] = 1;
  return m;
};

/** Column-major matrix product `a · b` (apply `b` first, then `a`). */
export const multiply = (a: Mat4, b: Mat4): Mat4 => {
  const out = new Float32Array(16);
  for (let col = 0; col < 4; col += 1) {
    for (let row = 0; row < 4; row += 1) {
      let sum = 0;
      for (let k = 0; k < 4; k += 1) {
        sum += a[k * 4 + row]! * b[col * 4 + k]!;
      }
      out[col * 4 + row] = sum;
    }
  }
  return out;
};

/**
 * The standard model matrix T·R·S: scale in local space, then rotate by the
 * unit quaternion `[x, y, z, w]`, then translate — so the origin always lands
 * exactly on `position`.
 */
export const fromTrs = (position: EngineVec3, rotation: EngineQuat, scale: EngineVec3): Mat4 => {
  const [x, y, z, w] = rotation;
  const r00 = 1 - 2 * (y * y + z * z);
  const r01 = 2 * (x * y - z * w);
  const r02 = 2 * (x * z + y * w);
  const r10 = 2 * (x * y + z * w);
  const r11 = 1 - 2 * (x * x + z * z);
  const r12 = 2 * (y * z - x * w);
  const r20 = 2 * (x * z - y * w);
  const r21 = 2 * (y * z + x * w);
  const r22 = 1 - 2 * (x * x + y * y);
  const m = new Float32Array(16);
  m[0] = r00 * scale.x;
  m[1] = r10 * scale.x;
  m[2] = r20 * scale.x;
  m[4] = r01 * scale.y;
  m[5] = r11 * scale.y;
  m[6] = r21 * scale.y;
  m[8] = r02 * scale.z;
  m[9] = r12 * scale.z;
  m[10] = r22 * scale.z;
  m[12] = position.x;
  m[13] = position.y;
  m[14] = position.z;
  m[15] = 1;
  return m;
};

/**
 * Transform a point (w = 1) by `m` with perspective divide, guarding a
 * near-zero clip-space w. Used by tests and any CPU-side projection math.
 */
export const transformPoint = (m: Mat4, p: EngineVec3): EngineVec3 => {
  const x = m[0]! * p.x + m[4]! * p.y + m[8]! * p.z + m[12]!;
  const y = m[1]! * p.x + m[5]! * p.y + m[9]! * p.z + m[13]!;
  const z = m[2]! * p.x + m[6]! * p.y + m[10]! * p.z + m[14]!;
  const w = m[3]! * p.x + m[7]! * p.y + m[11]! * p.z + m[15]!;
  const inv = Math.abs(w) < EPS ? 1 : 1 / w;
  return v3(x * inv, y * inv, z * inv);
};
