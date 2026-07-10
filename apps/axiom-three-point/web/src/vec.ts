/*
 * vec.ts — the pure-TypeScript linear-algebra core the whole game runs on. It
 * imports nothing, so the gameplay + physics + session modules that build on it
 * are constructible in a bare `node --test` process with no DOM. The renderer
 * adaptation (turning a `Vec3` into the engine's `Transform`) lives in
 * `scene.ts`, the one file allowed to touch the renderer.
 *
 * `Vec3` is a plain `{x,y,z}` and `Quat` is an `[x,y,z,w]` tuple — structurally
 * identical to the engine's own types, so `scene.ts` hands them straight through.
 */

/** A 3-vector — plain f64 `{x,y,z}`, structurally the SDK's `Vec3`. */
export interface Vec3 {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

/** A 2-vector — plain f64 `{x,y}`, structurally the SDK's `Vec2` (also a pointer sample). */
export interface Vec2 {
  readonly x: number;
  readonly y: number;
}

export const vec2 = (x: number, y: number): Vec2 => ({ x, y });

/** A rotation quaternion as the SDK's `[x, y, z, w]`. */
export type Quat = readonly [number, number, number, number];

export const IDENTITY_QUAT: Quat = [0, 0, 0, 1];

export const vec3 = (x: number, y: number, z: number): Vec3 => ({ x, y, z });

export const ZERO: Vec3 = vec3(0, 0, 0);

export const add = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x + b.x, y: a.y + b.y, z: a.z + b.z });

export const sub = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z });

export const scale = (a: Vec3, s: number): Vec3 => ({ x: a.x * s, y: a.y * s, z: a.z * s });

export const dot = (a: Vec3, b: Vec3): number => a.x * b.x + a.y * b.y + a.z * b.z;

export const length = (a: Vec3): number => Math.sqrt(dot(a, a));

export const normalize = (a: Vec3): Vec3 => {
  const len = length(a);
  return len < 1e-9 ? vec3(0, 1, 0) : scale(a, 1 / len);
};

export const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);

/** Linear interpolate two scalars. */
export const mix = (a: number, b: number, t: number): number => a + (b - a) * t;

/** Linear interpolate two vectors. */
export const lerp = (a: Vec3, b: Vec3, t: number): Vec3 => add(a, scale(sub(b, a), t));

/** The classic Hermite ease (0..1 → 0..1) used for the rack-to-rack camera glide. */
export const smoothstep = (t: number): number => {
  const c = clamp(t, 0, 1);
  return c * c * (3 - 2 * c);
};

/** Componentwise clamp of `p` into the AABB `[center - half, center + half]`. */
export const clampToBox = (p: Vec3, center: Vec3, half: Vec3): Vec3 => ({
  x: clamp(p.x, center.x - half.x, center.x + half.x),
  y: clamp(p.y, center.y - half.y, center.y + half.y),
  z: clamp(p.z, center.z - half.z, center.z + half.z),
});

/**
 * A quaternion (SDK `[x,y,z,w]`) from intrinsic XYZ Euler angles in radians — the
 * TS twin of the engine's `Quat::from_euler_xyz`, so authored rotations compose
 * identically.
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

/** Hamilton product `a * b` of two `[x,y,z,w]` quaternions. */
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

/** Rotate `v` by unit quaternion `q` (`[x,y,z,w]`). */
export const quatRotate = (q: Quat, v: Vec3): Vec3 => {
  const [qx, qy, qz, qw] = q;
  const tx = 2 * (qy * v.z - qz * v.y);
  const ty = 2 * (qz * v.x - qx * v.z);
  const tz = 2 * (qx * v.y - qy * v.x);
  return {
    x: v.x + qw * tx + (qy * tz - qz * ty),
    y: v.y + qw * ty + (qz * tx - qx * tz),
    z: v.z + qw * tz + (qx * ty - qy * tx),
  };
};

/**
 * Integrate a unit orientation quaternion by angular velocity `w` (rad/s, world
 * frame) over `dt` seconds: `q ← normalize(q + dt/2 · (w ⊗ q))`. The ball's visible
 * spin — purely visual, so first-order integration + renormalize is plenty.
 */
export const integrateOrientation = (q: Quat, w: Vec3, dt: number): Quat => {
  const [qx, qy, qz, qw] = q;
  const h = dt * 0.5;
  // (w ⊗ q) with w promoted to the pure quaternion (w.x, w.y, w.z, 0).
  const dx = h * (w.x * qw + w.y * qz - w.z * qy);
  const dy = h * (-w.x * qz + w.y * qw + w.z * qx);
  const dz = h * (w.x * qy - w.y * qx + w.z * qw);
  const dw = h * (-w.x * qx - w.y * qy - w.z * qz);
  const nx = qx + dx;
  const ny = qy + dy;
  const nz = qz + dz;
  const nw = qw + dw;
  const n = Math.sqrt(nx * nx + ny * ny + nz * nz + nw * nw);
  return n < 1e-9 ? IDENTITY_QUAT : [nx / n, ny / n, nz / n, nw / n];
};
