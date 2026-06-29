/*
 * The 3D math namespaces (SPEC-11 §4.2): `v3` (vector), `mat4` (matrix), `quat`
 * (quaternion). CRITICAL: every operation routes to the native `MathApi` through
 * the `HostBridge` — there is exactly ONE deterministic source of truth for engine
 * math (SPEC-03 §3.2), so NONE of this is re-implemented in TS. Each namespace is a
 * frozen record of thin forwarders; a cross-check test asserts the projected
 * surface and the native `MathApi` agree on a vector of sample inputs (no second
 * implementation to drift).
 *
 * The `Vec3`/`Mat4`/`Quat` value shapes are the neutral records the bridge carries
 * (SPEC-11 §5): `Vec3` is `{ x, y, z }`, `Mat4` a 16-number row-major array, `Quat`
 * an `[x, y, z, w]` tuple — all plain data that marshals 1:1 across the wasm seam.
 */

import type { Mat4, Quat, Vec3 } from "./vocabulary.ts";
import type { PerspectiveSpec } from "./host-descriptors.ts";
import { boundHost } from "./host-binding.ts";

/** Vector algebra projected from the native `MathApi` (SPEC-11 §4.2). */
export const v3 = {
  /** `lhs + rhs`. */
  add: (lhs: Vec3, rhs: Vec3): Vec3 => boundHost().v3Add(lhs, rhs),
  /** `lhs × rhs` (cross product). */
  cross: (lhs: Vec3, rhs: Vec3): Vec3 => boundHost().v3Cross(lhs, rhs),
  /** The distance between `lhs` and `rhs`. */
  dist: (lhs: Vec3, rhs: Vec3): number => boundHost().v3Dist(lhs, rhs),
  /** `lhs · rhs` (dot product). */
  dot: (lhs: Vec3, rhs: Vec3): number => boundHost().v3Dot(lhs, rhs),
  /** The Euclidean length of `vector`. */
  len: (vector: Vec3): number => boundHost().v3Len(vector),
  /** The linear blend from `lhs` to `rhs` by `fraction`. */
  lerp: (lhs: Vec3, rhs: Vec3, fraction: number): Vec3 => boundHost().v3Lerp(lhs, rhs, fraction),
  /** The unit vector in the direction of `vector`. */
  normalize: (vector: Vec3): Vec3 => boundHost().v3Normalize(vector),
  /** `vector * scalar`. */
  scale: (vector: Vec3, scalar: number): Vec3 => boundHost().v3Scale(vector, scalar),
  /** `lhs - rhs`. */
  sub: (lhs: Vec3, rhs: Vec3): Vec3 => boundHost().v3Sub(lhs, rhs),
} as const;

/** Matrix algebra projected from the native `MathApi` (SPEC-11 §4.2). */
export const mat4 = {
  /** A TRS (translate · rotate · scale) composition. */
  fromTRS: (translation: Vec3, rotation: Quat, scale: Vec3): Mat4 =>
    boundHost().mat4FromTRS(translation, rotation, scale),
  /** The 4×4 identity matrix. */
  identity: (): Mat4 => boundHost().mat4Identity(),
  /** The inverse of `matrix`. */
  invert: (matrix: Mat4): Mat4 => boundHost().mat4Invert(matrix),
  /** A right-handed look-at view matrix. */
  lookAt: (eye: Vec3, target: Vec3, up: Vec3): Mat4 => boundHost().mat4LookAt(eye, target, up),
  /** The matrix product `lhs · rhs`. */
  multiply: (lhs: Mat4, rhs: Mat4): Mat4 => boundHost().mat4Multiply(lhs, rhs),
  /** A right-handed perspective projection matrix from its spec. */
  perspective: (spec: PerspectiveSpec): Mat4 => boundHost().mat4Perspective(spec),
} as const;

/** Quaternion algebra projected from the native `MathApi` (SPEC-11 §4.2). */
export const quat = {
  /** A quaternion from intrinsic Euler angles (radians). */
  fromEuler: (pitch: number, yaw: number, roll: number): Quat =>
    boundHost().quatFromEuler(pitch, yaw, roll),
  /** The identity quaternion. */
  identity: (): Quat => boundHost().quatIdentity(),
  /** The composition `lhs · rhs`. */
  multiply: (lhs: Quat, rhs: Quat): Quat => boundHost().quatMultiply(lhs, rhs),
  /** The unit quaternion in the direction of `quaternion`. */
  normalize: (quaternion: Quat): Quat => boundHost().quatNormalize(quaternion),
  /** The rotation matrix of `quaternion`. */
  toMat4: (quaternion: Quat): Mat4 => boundHost().quatToMat4(quaternion),
} as const;
