/*
 * figure-math.ts — the quaternion + TRS-transform primitives the rigged player
 * figure and its two-bone leg IK need, ported faithfully from the engine's
 * `axiom-math` (`Quat` / `Transform`) so the TypeScript rig composes joint chains
 * byte-for-byte the way the end-zone (Rust) figure does. `vec.ts` only carries the
 * scalar/`Vec3` core and `quatFromEulerXyz`; everything a skeleton needs beyond
 * that — Hamilton product, axis-angle, vector rotation, inverse, shortest-arc
 * rotation-between, and `Transform.combine`/`transformPoint` — lives here.
 *
 * These are pure functions of their inputs (no wall clock, no state), matching the
 * `axiom-math` semantics exactly:
 *   - `Transform` applies as T·R·S (scale, then rotate, then translate).
 *   - `combine(parent, child)` = parent ∘ child (child expressed in parent space).
 *   - `quatMul(a, b)` is the Hamilton product: rotate first by `b`, then by `a`.
 * Where the Rust returns a `Result` (inverse / axis-angle / normalize of a
 * degenerate input) this returns a finite fallback instead, so a pose is always
 * renderable — the same defensive shape the Rust `.unwrap_or(...)` sites take.
 */

import { type Quat, type Vec3, IDENTITY_QUAT, add, scale, sub, vec3 } from "./vec.ts";

// ── Vec3 helpers not in vec.ts ───────────────────────────────────────────────────

export const dot = (a: Vec3, b: Vec3): number => a.x * b.x + a.y * b.y + a.z * b.z;

export const cross = (a: Vec3, b: Vec3): Vec3 =>
  vec3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);

/** Normalize `v`, or return `fallback` when `v` is (near) zero-length. Mirrors the
 * Rust `v.normalize().unwrap_or(fallback)` used all through the leg solver. */
export const normalizeOr = (v: Vec3, fallback: Vec3): Vec3 => {
  const len = Math.hypot(v.x, v.y, v.z);
  return len > 1e-9 ? scale(v, 1 / len) : fallback;
};

// ── Quat ─────────────────────────────────────────────────────────────────────────

/** Hamilton product `a * b` — rotate first by `b`, then by `a` (axiom-math order). */
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

const conjugate = (q: Quat): Quat => [-q[0], -q[1], -q[2], q[3]];

/** Inverse rotation (`conjugate / |q|²`); identity for a degenerate input. */
export const quatInverse = (q: Quat): Quat => {
  const ls = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
  if (!(ls > 0) || !Number.isFinite(ls)) {
    return IDENTITY_QUAT;
  }
  const c = conjugate(q);
  return [c[0] / ls, c[1] / ls, c[2] / ls, c[3] / ls];
};

/** Rotation about a unit-normalized `axis` by `angle` rad; identity if degenerate. */
export const quatFromAxisAngle = (axis: Vec3, angle: number): Quat => {
  const len = Math.hypot(axis.x, axis.y, axis.z);
  if (!Number.isFinite(angle) || len <= 1e-9) {
    return IDENTITY_QUAT;
  }
  const half = angle * 0.5;
  const s = Math.sin(half) / len;
  return [axis.x * s, axis.y * s, axis.z * s, Math.cos(half)];
};

/** Rotate a vector by a (unit) quaternion — the standard 18-multiply form. */
export const quatRotate = (q: Quat, v: Vec3): Vec3 => {
  const qv = vec3(q[0], q[1], q[2]);
  const t = scale(cross(qv, v), 2);
  return add(add(v, scale(t, q[3])), cross(qv, t));
};

/** The shortest-arc rotation taking unit `from` onto unit `to`. Degenerate cases
 * (parallel / anti-parallel / zero) fall back to identity or a stable
 * perpendicular half-turn, so the result is always a finite unit quaternion.
 * Ported from `leg.rs::rotation_between`. */
export const rotationBetween = (from: Vec3, to: Vec3): Quat => {
  const down = vec3(0, -1, 0);
  const a = normalizeOr(from, down);
  const b = normalizeOr(to, down);
  const d = Math.min(Math.max(dot(a, b), -1), 1);
  if (d > 0.9999) {
    return IDENTITY_QUAT;
  }
  if (d < -0.9999) {
    const axis = normalizeOr(cross(a, vec3(1, 0, 0)), normalizeOr(cross(a, vec3(0, 0, 1)), vec3(0, 1, 0)));
    return quatFromAxisAngle(axis, Math.PI);
  }
  return quatFromAxisAngle(cross(a, b), Math.acos(d));
};

// ── Transform (T·R·S) ──────────────────────────────────────────────────────────

/** A translation/rotation/scale transform — structurally the engine's `Transform`
 * (`{ position, rotation, scale }`), so a resolved part drops straight into a
 * `SceneInstance.transform`. */
export interface Transform {
  readonly position: Vec3;
  readonly rotation: Quat;
  readonly scale: Vec3;
}

export const transform = (position: Vec3, rotation: Quat, scale: Vec3): Transform => ({ position, rotation, scale });

const mulComponents = (a: Vec3, b: Vec3): Vec3 => vec3(a.x * b.x, a.y * b.y, a.z * b.z);

/** Apply the transform to a point: scale, then rotate, then translate. */
export const transformPoint = (t: Transform, p: Vec3): Vec3 => add(t.position, quatRotate(t.rotation, mulComponents(t.scale, p)));

/** Compose two transforms: `child` expressed in `parent`'s space (parent ∘ child).
 * Matches `axiom-math::Transform::combine` exactly. */
export const combine = (parent: Transform, child: Transform): Transform => ({
  position: add(parent.position, quatRotate(parent.rotation, mulComponents(parent.scale, child.position))),
  rotation: quatMul(parent.rotation, child.rotation),
  scale: mulComponents(parent.scale, child.scale),
});

export const distance = (a: Vec3, b: Vec3): number => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);

export type { Vec3, Quat };
export { sub };
