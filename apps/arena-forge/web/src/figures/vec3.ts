/*
 * vec3.ts — the pure linear-algebra core for the procedural figure system. It
 * imports nothing from `@axiom/web-engine`, so the whole figure grammar, mesh
 * generation, and transform composition stay constructible in a bare `node --test`
 * process with no DOM and no renderer. `Vec3` is a plain `{x,y,z}` and `Quat` is
 * an `[x,y,z,w]` tuple — structurally identical to the engine's `EngineVec3` /
 * `EngineQuat`, so `scene.ts` hands them straight through to `setNodeTransform`.
 *
 * It adds the two operations the engine deliberately does NOT export (`mat4.ts`
 * is internal): `quatMul` (compose two rotations) and `rotateVec` (rotate a point
 * by a quaternion). Those are exactly what a flat, no-parenting scene graph needs
 * to compose a figure's part hierarchy to world space on the CPU.
 */

export interface Vec3 {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

/** A rotation quaternion as the engine's `[x, y, z, w]`. */
export type Quat = readonly [number, number, number, number];

export const IDENTITY_QUAT: Quat = [0, 0, 0, 1];

export const vec3 = (x: number, y: number, z: number): Vec3 => ({ x, y, z });
export const ZERO: Vec3 = vec3(0, 0, 0);
export const ONE: Vec3 = vec3(1, 1, 1);

export const add = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x + b.x, y: a.y + b.y, z: a.z + b.z });
export const sub = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z });
export const scale = (a: Vec3, s: number): Vec3 => ({ x: a.x * s, y: a.y * s, z: a.z * s });
export const mulv = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x * b.x, y: a.y * b.y, z: a.z * b.z });
export const length = (a: Vec3): number => Math.sqrt(a.x * a.x + a.y * a.y + a.z * a.z);
export const dot = (a: Vec3, b: Vec3): number => a.x * b.x + a.y * b.y + a.z * b.z;

export const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);
export const mix = (a: number, b: number, t: number): number => a + (b - a) * t;
export const lerp = (a: Vec3, b: Vec3, t: number): Vec3 => add(a, scale(sub(b, a), t));

export const normalize = (a: Vec3): Vec3 => {
  const len = length(a);
  return len < 1e-9 ? ZERO : scale(a, 1 / len);
};

/**
 * A quaternion from intrinsic XYZ Euler angles (radians) — the twin of the
 * engine's `Quat::from_euler_xyz`, so authored rest rotations compose identically.
 */
export const quatFromEulerXyz = (rx: number, ry: number, rz: number): Quat => {
  const hx = rx * 0.5;
  const hy = ry * 0.5;
  const hz = rz * 0.5;
  const cx = Math.cos(hx);
  const sx = Math.sin(hx);
  const cy = Math.cos(hy);
  const sy = Math.sin(hy);
  const cz = Math.cos(hz);
  const sz = Math.sin(hz);
  return [
    sx * cy * cz + cx * sy * sz,
    cx * sy * cz - sx * cy * sz,
    cx * cy * sz + sx * sy * cz,
    cx * cy * cz - sx * sy * sz,
  ];
};

/** Axis-angle quaternion (axis need not be normalized). */
export const quatFromAxisAngle = (axis: Vec3, angle: number): Quat => {
  const n = normalize(axis);
  const h = angle * 0.5;
  const s = Math.sin(h);
  return [n.x * s, n.y * s, n.z * s, Math.cos(h)];
};

/** The Hamilton product `a ∘ b` (apply b, then a) — composes two rotations. */
export const quatMul = (a: Quat, b: Quat): Quat => {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
};

/** Rotate a vector by a quaternion (`q · v · q⁻¹`, expanded for speed). */
export const rotateVec = (q: Quat, v: Vec3): Vec3 => {
  const [x, y, z, w] = q;
  // t = 2 * cross(q.xyz, v)
  const tx = 2 * (y * v.z - z * v.y);
  const ty = 2 * (z * v.x - x * v.z);
  const tz = 2 * (x * v.y - y * v.x);
  // v + w*t + cross(q.xyz, t)
  return {
    x: v.x + w * tx + (y * tz - z * ty),
    y: v.y + w * ty + (z * tx - x * tz),
    z: v.z + w * tz + (x * ty - y * tx),
  };
};

/** Normalized linear interpolation of two quaternions (cheap, stable for small
 * steps — figure animation never needs full slerp). */
export const quatNlerp = (a: Quat, b: Quat, t: number): Quat => {
  // Take the shorter arc.
  const d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
  const s = d < 0 ? -1 : 1;
  const x = a[0] + (b[0] * s - a[0]) * t;
  const y = a[1] + (b[1] * s - a[1]) * t;
  const z = a[2] + (b[2] * s - a[2]) * t;
  const w = a[3] + (b[3] * s - a[3]) * t;
  const len = Math.hypot(x, y, z, w) || 1;
  return [x / len, y / len, z / len, w / len];
};
