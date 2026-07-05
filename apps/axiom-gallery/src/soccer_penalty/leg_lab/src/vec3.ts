/*
 * The minimal 3-vector the procedural-leg sim runs on — a plain `{x, y, z}` with
 * named operations. Deliberately independent of the `@axiom/game` SDK's `v3`
 * (which routes every op through the native wasm MathApi): the sim is hot,
 * per-frame arithmetic, so it runs in plain TS f64 and stays deterministic within
 * the browser. The scene layer converts these into SDK `Transform`s at the edge.
 *
 * (This is the TypeScript twin of the Rust lab's use of `axiom_math::Vec3`.)
 */

/** A 3D vector / point. Structurally identical to the SDK's `Vec3`. */
export interface Vec3 {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

export const vec3 = (x: number, y: number, z: number): Vec3 => ({ x, y, z });

export const add = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x + b.x, y: a.y + b.y, z: a.z + b.z });

export const sub = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z });

export const scale = (v: Vec3, k: number): Vec3 => ({ x: v.x * k, y: v.y * k, z: v.z * k });

export const dot = (a: Vec3, b: Vec3): number => a.x * b.x + a.y * b.y + a.z * b.z;

export const cross = (a: Vec3, b: Vec3): Vec3 => ({
  x: a.y * b.z - a.z * b.y,
  y: a.z * b.x - a.x * b.z,
  z: a.x * b.y - a.y * b.x,
});

export const length = (v: Vec3): number => Math.hypot(v.x, v.y, v.z);

export const distance = (a: Vec3, b: Vec3): number => length(sub(a, b));

/** Normalize `v`, or return `fallback` when `v` is (near) zero-length. */
export const normalizeOr = (v: Vec3, fallback: Vec3): Vec3 => {
  const len = length(v);
  return len < 1e-6 ? fallback : scale(v, 1 / len);
};
