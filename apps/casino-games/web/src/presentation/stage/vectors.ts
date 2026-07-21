/*
 * vectors.ts — thin constructors and quaternion helpers over the engine's OWN
 * value shapes (`EngineVec3`, `EngineQuat`, `Transform`). This is arrangement
 * vocabulary, not a math library: the engine owns matrices, projection, and
 * rendering; games only need to build transforms and axis rotations.
 */

import type { EngineQuat, EngineVec3, Transform } from "@axiom/web-engine";

export const v3 = (x: number, y: number, z: number): EngineVec3 => ({ x, y, z });

export const addV3 = (a: EngineVec3, b: EngineVec3): EngineVec3 => v3(a.x + b.x, a.y + b.y, a.z + b.z);

export const subV3 = (a: EngineVec3, b: EngineVec3): EngineVec3 => v3(a.x - b.x, a.y - b.y, a.z - b.z);

export const scaleV3 = (a: EngineVec3, s: number): EngineVec3 => v3(a.x * s, a.y * s, a.z * s);

export const dotV3 = (a: EngineVec3, b: EngineVec3): number => a.x * b.x + a.y * b.y + a.z * b.z;

export const crossV3 = (a: EngineVec3, b: EngineVec3): EngineVec3 =>
  v3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);

/** Unit vector along `a`; a zero-length vector normalizes to +Z rather than to
 * NaN, so a degenerate camera basis degrades to a usable one. */
export const normalizeV3 = (a: EngineVec3): EngineVec3 => {
  const length = Math.hypot(a.x, a.y, a.z);
  return length > 1e-9 ? scaleV3(a, 1 / length) : v3(0, 0, 1);
};

export const lerpV3 = (a: EngineVec3, b: EngineVec3, t: number): EngineVec3 =>
  v3(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t);

export const QUAT_IDENTITY: EngineQuat = [0, 0, 0, 1];

/** Axis-angle quaternion (axis must be normalized). */
export const quatAxisAngle = (axis: EngineVec3, radians: number): EngineQuat => {
  const half = radians / 2;
  const s = Math.sin(half);
  return [axis.x * s, axis.y * s, axis.z * s, Math.cos(half)];
};

export const quatYaw = (radians: number): EngineQuat => quatAxisAngle(v3(0, 1, 0), radians);
export const quatPitch = (radians: number): EngineQuat => quatAxisAngle(v3(1, 0, 0), radians);
export const quatRoll = (radians: number): EngineQuat => quatAxisAngle(v3(0, 0, 1), radians);

/** Hamilton product a·b (apply b first, then a). */
export const quatMul = (a: EngineQuat, b: EngineQuat): EngineQuat => {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
};

export const transformOf = (position: EngineVec3, scale: EngineVec3, rotation: EngineQuat = QUAT_IDENTITY): Transform => ({
  position,
  rotation,
  scale,
});

/** Rotate a vector by a quaternion (q · v · q⁻¹). */
export const rotateByQuat = (v: EngineVec3, q: EngineQuat): EngineVec3 => {
  const [qx, qy, qz, qw] = q;
  const tx = 2 * (qy * v.z - qz * v.y);
  const ty = 2 * (qz * v.x - qx * v.z);
  const tz = 2 * (qx * v.y - qy * v.x);
  return v3(
    v.x + qw * tx + (qy * tz - qz * ty),
    v.y + qw * ty + (qz * tx - qx * tz),
    v.z + qw * tz + (qx * ty - qy * tx),
  );
};

/** The transform of a part swinging on a hinge: `offset` is the part center
 * relative to the hinge in the closed pose; `q` the hinge rotation. Chest
 * lids, latches, doors, and vault bolts all pose through this. */
export const hingedTransform = (hinge: EngineVec3, offset: EngineVec3, q: EngineQuat, scale: EngineVec3): Transform => ({
  position: addV3(hinge, rotateByQuat(offset, q)),
  rotation: q,
  scale,
});
