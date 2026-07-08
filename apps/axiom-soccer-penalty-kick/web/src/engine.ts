/*
 * engine.ts — the reusable, game-agnostic toolkit this app builds its soccer
 * penalty game on top of. Nothing in this file knows about penalties, goalies,
 * scoring, or the pitch: it is the small pile of generic engine primitives the
 * rest of `web/src/*.ts` share, pulled out of the game modules so the boundary
 * between "engine" and "game" is explicit.
 *
 * It has six parts, each a primitive a from-scratch @axiom/game app would want:
 *
 *   1. Linear algebra & transforms — Vec3 / Quat / Transform math (the faithful
 *      TS twin of `axiom_math`: add/sub/scale/dot/length/normalize/lerp, the
 *      Euler→quat builder, quat rotate/multiply, and TRS `combine`).
 *   2. Articulated-hierarchy FK — resolve a parent-indexed skeleton of local
 *      transforms into world transforms.
 *   3. Collision primitives — moving-ball vs. sphere / vs. AABB contact tests.
 *   4. Projectile physics — a semi-implicit-Euler integrator and the two-probe
 *      affine launch solve that lands a projectile on a target at tick N.
 *   5. Animation curves — smoothstep and piecewise keyframe sampling.
 *   6. @axiom/game scene adapters — the mesh-catalog conventions (box = unit cube
 *      → scale is full extents, sphere = unit diameter → scale is 2·radius) and
 *      the `{position,rotation,scale}` transform the SDK's `spawnRenderable` /
 *      `setNodeTransform` consume.
 *
 * The game modules import from here; this file imports only pure math + the SDK.
 */

import type { Transform as SdkTransform } from "@axiom/game";

// ── 1. linear algebra & transforms ───────────────────────────────────────────

/**
 * The minimal 3-vector the game runs on — a plain-object `{x,y,z}` that is
 * structurally the SDK's `Vec3`, so a sim vector can be handed straight to
 * `spawnRenderable` / `setNodeTransform` (via `sdkVec`) as a transform position.
 * Pure f64 math, a faithful twin of the Rust engine's `axiom_math::Vec3`.
 */
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

export const dot = (a: Vec3, b: Vec3): number => a.x * b.x + a.y * b.y + a.z * b.z;

export const length = (a: Vec3): number => Math.sqrt(dot(a, a));

export const normalize = (a: Vec3): Vec3 => {
  const len = length(a);
  return len < 1e-9 ? vec3(0, 1, 0) : scale(a, 1 / len);
};

export const lerp = (a: Vec3, b: Vec3, t: number): Vec3 => add(a, scale(sub(b, a), t));

export const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);

/** Componentwise clamp of `p` into the AABB `[center - half, center + half]`. */
export const clampToBox = (p: Vec3, center: Vec3, half: Vec3): Vec3 => ({
  x: clamp(p.x, center.x - half.x, center.x + half.x),
  y: clamp(p.y, center.y - half.y, center.y + half.y),
  z: clamp(p.z, center.z - half.z, center.z + half.z),
});

/**
 * A quaternion (SDK `[x,y,z,w]`) from intrinsic XYZ Euler angles in radians — the
 * TS twin of the engine's `Quat::from_euler_xyz`, so authored dive/kick joint
 * rotations compose identically. Rz·Ry·Rx applied to a vector (X first).
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

/** Rotate `v` by unit quaternion `q` (`[x,y,z,w]`). Used to compose the box hierarchies. */
export const quatRotate = (q: Quat, v: Vec3): Vec3 => {
  const [qx, qy, qz, qw] = q;
  // t = 2 * cross(q.xyz, v)
  const tx = 2 * (qy * v.z - qz * v.y);
  const ty = 2 * (qz * v.x - qx * v.z);
  const tz = 2 * (qx * v.y - qy * v.x);
  // v + qw * t + cross(q.xyz, t)
  return {
    x: v.x + qw * tx + (qy * tz - qz * ty),
    y: v.y + qw * ty + (qz * tx - qx * tz),
    z: v.z + qw * tz + (qx * ty - qy * tx),
  };
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

/** A rigid transform (translation + rotation + scale), the game's `Transform`. */
export interface Transform {
  readonly translation: Vec3;
  readonly rotation: Quat;
  readonly scale: Vec3;
}

export const fromTranslation = (t: Vec3): Transform => ({ translation: t, rotation: IDENTITY_QUAT, scale: vec3(1, 1, 1) });

/**
 * Compose parent∘local (standard TRS composition) — the engine's
 * `Transform::combine`. Child world = parent applied to the local transform:
 * translation is parent.translation + parent.rot·(parent.scale ⊙ local.translation),
 * rotation is parent.rot · local.rot, scale is componentwise product.
 */
export const combine = (parent: Transform, local: Transform): Transform => ({
  translation: add(parent.translation, quatRotate(parent.rotation, {
    x: parent.scale.x * local.translation.x,
    y: parent.scale.y * local.translation.y,
    z: parent.scale.z * local.translation.z,
  })),
  rotation: quatMul(parent.rotation, local.rotation),
  scale: {
    x: parent.scale.x * local.scale.x,
    y: parent.scale.y * local.scale.y,
    z: parent.scale.z * local.scale.z,
  },
});

// ── 2. articulated-hierarchy forward kinematics ──────────────────────────────

/**
 * Resolve a parent-indexed skeleton of local transforms into world transforms by
 * TRS composition. `parents[i]` is the index of part `i`'s parent, or negative
 * for a root; parents must precede their children (index order), so one forward
 * pass suffices. The generic FK the box puppets (keeper, kicker) pose through.
 */
export const resolveHierarchy = (parents: readonly number[], locals: readonly Transform[]): Transform[] => {
  const world: Transform[] = new Array<Transform>(parents.length);
  for (let i = 0; i < parents.length; i += 1) {
    const parent = parents[i]!;
    world[i] = parent < 0 ? locals[i]! : combine(world[parent]!, locals[i]!);
  }
  return world;
};

// ── 3. collision primitives ──────────────────────────────────────────────────

/**
 * Contact of a moving ball (`ballCenter`, `ballRadius`) with a static sphere.
 * Returns the contact point on the sphere's surface (toward the ball), or `null`
 * if the two do not overlap. A degenerate coincident-center falls back to +Y.
 */
export const ballSphereContact = (ballCenter: Vec3, ballRadius: number, sphereCenter: Vec3, sphereRadius: number): Vec3 | null => {
  const diff = sub(ballCenter, sphereCenter);
  const dist = length(diff);
  if (dist > sphereRadius + ballRadius) return null;
  const dir = dist <= 1e-6 ? vec3(0, 1, 0) : scale(diff, 1 / dist);
  return add(sphereCenter, scale(dir, sphereRadius));
};

/**
 * Contact of a moving ball (`ballCenter`, `ballRadius`) with a static AABB
 * (`boxCenter` ± `boxHalf`). Returns the closest point on the box to the ball if
 * they overlap (ball center within `ballRadius` of the box), else `null`.
 */
export const ballBoxContact = (ballCenter: Vec3, ballRadius: number, boxCenter: Vec3, boxHalf: Vec3): Vec3 | null => {
  const closest = clampToBox(ballCenter, boxCenter, boxHalf);
  const d = sub(ballCenter, closest);
  return dot(d, d) <= ballRadius * ballRadius ? closest : null;
};

// ── 4. projectile physics ────────────────────────────────────────────────────

/**
 * Semi-implicit (symplectic) Euler projectile: from `start` with launch velocity
 * `v0` under constant `gravity`, stepped `dt` per tick for `steps` ticks (clamped
 * to `[1, capLen - 1]`). Returns a `capLen`-long path; positions past the last
 * step are held at the landing position. `v += g·dt; pos += v·dt` per tick.
 */
export const integrateProjectile = (start: Vec3, v0: Vec3, gravity: Vec3, dt: number, steps: number, capLen: number): Vec3[] => {
  const n = Math.min(Math.max(steps, 1), capLen - 1);
  const path: Vec3[] = new Array<Vec3>(capLen);
  path[0] = start;
  let vel = v0;
  let pos = start;
  for (let k = 1; k <= n; k += 1) {
    vel = add(vel, scale(gravity, dt)); // v += g·dt
    pos = add(pos, scale(vel, dt)); // pos += v·dt
    path[k] = pos;
  }
  for (let k = n + 1; k < capLen; k += 1) {
    path[k] = path[n]!;
  }
  return path;
};

/**
 * Solve for the launch velocity that lands the projectile exactly on `target` at
 * tick `n` (= `steps`, clamped), then pin the endpoints by distributing the
 * sub-step discrete-integration residual linearly (0 at launch → 1 at landing).
 * Two probe integrations recover the integrator's per-tick affine response (the
 * same coefficient on every axis for constant gravity); requires a nonzero
 * `target - start` on Z. Returns the full `capLen`-long path.
 */
export const solveLaunchToTarget = (start: Vec3, target: Vec3, gravity: Vec3, dt: number, steps: number, capLen: number): Vec3[] => {
  const n = Math.min(Math.max(steps, 1), capLen - 1);
  const probe = scale(sub(target, start), 1 / n); // nonzero z guaranteed by the caller
  const endZero = integrateProjectile(start, ZERO, gravity, dt, n, capLen)[n]!;
  const endProbe = integrateProjectile(start, probe, gravity, dt, n, capLen)[n]!;
  const c = (endProbe.z - endZero.z) / probe.z; // per-tick response coefficient (same on all axes)
  const v0 = vec3((target.x - endZero.x) / c, (target.y - endZero.y) / c, (target.z - endZero.z) / c);
  const path = integrateProjectile(start, v0, gravity, dt, n, capLen);
  const residual = sub(target, path[n]!);
  for (let k = 0; k <= n; k += 1) {
    path[k] = add(path[k]!, scale(residual, k / n));
  }
  for (let k = n + 1; k < capLen; k += 1) {
    path[k] = path[n]!;
  }
  return path;
};

// ── 5. animation curves ──────────────────────────────────────────────────────

export const smoothstep = (t: number): number => t * t * (3 - 2 * t);

/**
 * Piecewise smoothstep interpolation of `(tick, value)` keyframes, sorted by
 * tick. Holds the first value before the first key and the last value after the
 * last key. The scalar animation channel the kicker's joints are driven by.
 */
export const sampleCurve = (keys: readonly [number, number][], t: number): number => {
  if (t <= keys[0]![0]) return keys[0]![1];
  const last = keys[keys.length - 1]!;
  if (t >= last[0]) return last[1];
  for (let i = 0; i < keys.length - 1; i += 1) {
    const [ta, va] = keys[i]!;
    const [tb, vb] = keys[i + 1]!;
    if (t >= ta && t <= tb) {
      return va + (vb - va) * smoothstep((t - ta) / (tb - ta));
    }
  }
  return last[1];
};

// ── 6. @axiom/game scene adapters ────────────────────────────────────────────

export const DEG_TO_RAD = Math.PI / 180;

/** Thin quads/lines are drawn as boxes; their near-zero extent is clamped to this. */
export const MIN_EXTENT = 0.02;

/** A sim `Vec3` as the SDK's plain `{x,y,z}` (structurally identical, named for intent). */
export const sdkVec = (v: Vec3): { x: number; y: number; z: number } => ({ x: v.x, y: v.y, z: v.z });

/** Full box extents → the unit-cube `box` mesh scale (thin dims clamped to `MIN_EXTENT`). */
export const boxScale = (size: Vec3): Vec3 =>
  vec3(Math.max(size.x, MIN_EXTENT), Math.max(size.y, MIN_EXTENT), Math.max(size.z, MIN_EXTENT));

/** A radius → the unit-diameter `sphere` mesh scale (2·radius on each axis). */
export const sphereScale = (radius: number): Vec3 => vec3(radius * 2, radius * 2, radius * 2);

/** Build an SDK `Transform` (`spawnRenderable` / `setNodeTransform` input) from sim values. */
export const xform = (position: Vec3, scale: Vec3, rotation: Quat = IDENTITY_QUAT): SdkTransform => ({
  position: sdkVec(position),
  rotation,
  scale: sdkVec(scale),
});
