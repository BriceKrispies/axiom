/*
 * vec.ts — the pure-TypeScript linear-algebra core the whole game runs on. It
 * imports NOTHING (not even a type) from `@axiom/game`, so the gameplay + session
 * modules that build on it are constructible in a bare `node --test` process with no
 * wasm and no DOM. The SDK adaptation (turning a `Vec3` into the SDK's `Transform`)
 * lives in `scene.ts`, the one file allowed to touch the engine.
 *
 * `Vec3` is a plain `{x,y,z}` and `Quat` is an `[x,y,z,w]` tuple — structurally
 * identical to the SDK's own types, so `scene.ts` hands them straight through.
 */

/** A 3-vector — plain f64 `{x,y,z}`, structurally the SDK's `Vec3`. */
export interface Vec3 {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

/** A rotation quaternion as the SDK's `[x, y, z, w]`. */
export type Quat = readonly [number, number, number, number];

export const IDENTITY_QUAT: Quat = [0, 0, 0, 1];

export const vec3 = (x: number, y: number, z: number): Vec3 => ({ x, y, z });

export const ZERO: Vec3 = vec3(0, 0, 0);

export const add = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x + b.x, y: a.y + b.y, z: a.z + b.z });

export const sub = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z });

export const scale = (a: Vec3, s: number): Vec3 => ({ x: a.x * s, y: a.y * s, z: a.z * s });

export const length = (a: Vec3): number => Math.sqrt(a.x * a.x + a.y * a.y + a.z * a.z);

/** Horizontal (XZ-plane) length — the field is laid out on XZ with +Y up. */
export const lengthXZ = (a: Vec3): number => Math.hypot(a.x, a.z);

export const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);

export const clamp01 = (v: number): number => clamp(v, 0, 1);

/** Linear interpolate two scalars. */
export const mix = (a: number, b: number, t: number): number => a + (b - a) * t;

/** Linear interpolate two vectors. */
export const lerp = (a: Vec3, b: Vec3, t: number): Vec3 => add(a, scale(sub(b, a), t));

/**
 * A quaternion (SDK `[x,y,z,w]`) from intrinsic XYZ Euler angles in radians — the
 * TS twin of the engine's `Quat::from_euler_xyz`, so authored rotations compose
 * identically. Used to yaw the diamond's diagonals and pose the bat and figures.
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

/**
 * A tiny deterministic hash → [0, 1) noise source. All gameplay variation (pitch
 * selection, aim jitter, fielder wander phases) derives from `hash01(seed, …keys)`,
 * so the same seed reproduces the same round bit-for-bit — there is no stateful RNG
 * to fall out of sync. Integer-only mixing (imul + xorshift) so every platform
 * computes the identical value.
 */
export const hash01 = (seed: number, ...keys: readonly number[]): number => {
  let h = (seed | 0) ^ 0x9e3779b9;
  for (const k of keys) {
    h = Math.imul(h ^ (k | 0), 0x85ebca6b);
    h ^= h >>> 13;
    h = Math.imul(h, 0xc2b2ae35);
    h ^= h >>> 16;
  }
  return (h >>> 8) / 16777216;
};
