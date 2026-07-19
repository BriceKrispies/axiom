/*
 * figure.ts — the rigged 17-box player figure, ported from the end-zone (Rust)
 * `axiom-figure` player: a jointed skeleton (pelvis → torso → pads/helmet/arms,
 * pelvis → thighs → shins → feet) whose per-tick pose is a set of joint rotations
 * plus a few root adjustments. This file owns the STATIC skeleton (the part specs,
 * ported verbatim from `player/model.rs`), the RIG that resolves a `JointPose` to
 * world-space boxes (ported from `player/rig.rs` + the figure facade's
 * `posed_parts`), and the two-bone analytic LEG SOLVER (ported from
 * `presentation/locomotion/leg.rs`). The pose *builders* — running, batting, idle —
 * live in `figure-pose.ts`; this file is purely the body and how it bakes to boxes.
 *
 * Units are world units (= yards on the toy field). Y up, toes point +Z at
 * facing 0, parent-before-child. A resolved part is a unit-cube `box` instance:
 * position/rotation from the joint chain, scale = the part's box extents (times the
 * body squash). The tag selects the render material (see `figure-pose.ts` palettes).
 */

import { type Quat, type Vec3, IDENTITY_QUAT, quatFromEulerXyz, scale, vec3 } from "./vec.ts";
import { type Transform, combine, cross, distance, dot, normalizeOr, quatFromAxisAngle, quatInverse, quatMul, quatRotate, rotationBetween, sub, transform, transformPoint } from "./figure-math.ts";

// ── part + tag indices (from model.rs) ───────────────────────────────────────────

export const PART_COUNT = 17;
/** Uniform scale on the whole figure — the players render ~25% shorter than the
 * original end-zone proportions. Applied to every part offset/box AND to the
 * locomotion stance/stride constants (see `figure-pose.ts`) so the gait stays
 * self-similar: the planted-fraction ratio and leg reachability are scale-free. */
export const FIGURE_SCALE = 0.75;
const CENTER_Y_BASE = 1.0;
/** The figure's local origin height above the feet (body transform is the ground
 * position raised by this), at the current figure scale. */
export const FIGURE_CENTER_Y = CENTER_Y_BASE * FIGURE_SCALE;

// Palette slots (part tags) — the render side maps each to a material. `TAG_HELMET`
// is now the baseball CAP color (team); `TAG_FACEMASK` is retired (no facemask).
export const TAG_HELMET = 0;
export const TAG_FACEMASK = 1;
export const TAG_JERSEY = 2;
export const TAG_PANTS = 3;
export const TAG_SKIN = 4;
export const TAG_SHOES = 5;
export const TAG_TRIM = 6;
export const TAG_COUNT = 7;

// Part indices (the animation addresses joints by these). The football helmet +
// facemask are gone: the head is now a rounded skin sphere (HEAD) wearing a domed
// baseball CAP crown and a forward BRIM.
export const PELVIS = 0;
export const TORSO = 1;
export const HEAD = 2;
export const CAP = 3;
export const L_THIGH = 4;
export const L_SHIN = 5;
export const L_FOOT = 6;
export const R_THIGH = 7;
export const R_SHIN = 8;
export const R_FOOT = 9;
export const L_UPPER_ARM = 10;
export const L_FOREARM = 11;
export const L_HAND = 12;
export const R_UPPER_ARM = 13;
export const R_FOREARM = 14;
export const R_HAND = 15;
export const BRIM = 16;

/** `(parent, joint offset, box size, box offset, tag)` — the joint offset is from
 * the parent's pivot; the box offset centers the limb box while it pivots at the
 * joint. Ported one-to-one from `model.rs::PARTS`. */
/** The primitive a part renders as: a unit box, or a unit-diameter sphere (heads
 * and cap crowns are rounded). Scale carries the extents either way. */
export type PartMesh = "box" | "sphere";

export interface PartSpec {
  readonly parent: number;
  readonly offset: Vec3;
  readonly boxSize: Vec3;
  readonly boxOffset: Vec3;
  readonly tag: number;
  readonly mesh: PartMesh;
}

/** Build a part spec, baking the uniform `FIGURE_SCALE` into every length so the
 * whole skeleton (and the leg lengths the IK reads back from it) shrinks together. */
const p = (parent: number, offset: Vec3, boxSize: Vec3, boxOffset: Vec3, tag: number, mesh: PartMesh = "box"): PartSpec => ({
  boxOffset: scale(boxOffset, FIGURE_SCALE),
  boxSize: scale(boxSize, FIGURE_SCALE),
  mesh,
  offset: scale(offset, FIGURE_SCALE),
  parent,
  tag,
});

/** Feet on y≈0. Offsets are the ORIGINAL end-zone lengths; `p()` scales them by
 * `FIGURE_SCALE`. No football shoulder-pad slab, helmet, or facemask — the head is
 * a rounded skin sphere in a domed baseball cap with a forward brim. */
export const PARTS: readonly PartSpec[] = [
  // 0 pelvis (root)
  p(-1, vec3(0, 1.08 - CENTER_Y_BASE, 0), vec3(0.5, 0.3, 0.3), vec3(0, 0, 0), TAG_PANTS),
  // 1 torso
  p(0, vec3(0, 0.36, 0), vec3(0.62, 0.48, 0.36), vec3(0, 0.05, 0), TAG_JERSEY),
  // 2 head (rounded, skin)
  p(1, vec3(0, 0.42, 0), vec3(0.34, 0.38, 0.36), vec3(0, 0.1, 0), TAG_SKIN, "sphere"),
  // 3 cap crown (a team-color dome perched on top of the head)
  p(2, vec3(0, 0.24, 0.01), vec3(0.44, 0.3, 0.44), vec3(0, 0, 0), TAG_HELMET, "sphere"),
  // 4/5/6 left thigh, shin, foot
  p(0, vec3(-0.14, -0.14, 0), vec3(0.2, 0.46, 0.22), vec3(0, -0.24, 0), TAG_PANTS),
  p(4, vec3(0, -0.48, 0), vec3(0.16, 0.42, 0.18), vec3(0, -0.2, 0), TAG_TRIM),
  p(5, vec3(0, -0.4, 0), vec3(0.17, 0.12, 0.34), vec3(0, -0.02, 0.08), TAG_SHOES),
  // 7/8/9 right thigh, shin, foot
  p(0, vec3(0.14, -0.14, 0), vec3(0.2, 0.46, 0.22), vec3(0, -0.24, 0), TAG_PANTS),
  p(7, vec3(0, -0.48, 0), vec3(0.16, 0.42, 0.18), vec3(0, -0.2, 0), TAG_TRIM),
  p(8, vec3(0, -0.4, 0), vec3(0.17, 0.12, 0.34), vec3(0, -0.02, 0.08), TAG_SHOES),
  // 10/11/12 left upper arm, forearm, hand
  p(1, vec3(-0.42, 0.2, 0), vec3(0.16, 0.4, 0.16), vec3(0, -0.2, 0), TAG_JERSEY),
  p(10, vec3(0, -0.4, 0), vec3(0.13, 0.34, 0.13), vec3(0, -0.17, 0), TAG_SKIN),
  p(11, vec3(0, -0.34, 0), vec3(0.12, 0.14, 0.13), vec3(0, -0.07, 0), TAG_SKIN),
  // 13/14/15 right upper arm, forearm, hand
  p(1, vec3(0.42, 0.2, 0), vec3(0.16, 0.4, 0.16), vec3(0, -0.2, 0), TAG_JERSEY),
  p(13, vec3(0, -0.4, 0), vec3(0.13, 0.34, 0.13), vec3(0, -0.17, 0), TAG_SKIN),
  p(14, vec3(0, -0.34, 0), vec3(0.12, 0.14, 0.13), vec3(0, -0.07, 0), TAG_SKIN),
  // 16 cap brim (a flat bill jutting forward from the front of the crown)
  p(3, vec3(0, -0.09, 0.13), vec3(0.4, 0.05, 0.26), vec3(0, 0, 0.1), TAG_HELMET),
];

// ── pose ─────────────────────────────────────────────────────────────────────────

/** A resolved pose: per-joint LOCAL rotations plus the root adjustments the rig
 * applies to the whole body. Ported from `animation.rs::JointPose`. */
export interface JointPose {
  readonly joints: Quat[];
  /** Root vertical offset (bob, falls), world units. */
  readonly rootLift: number;
  /** Root pitch (forward lean +, backward -), radians. */
  readonly rootPitch: number;
  /** Root roll, radians. */
  readonly rootRoll: number;
}

/** A rest pose: identity joints, no root adjustment. */
export const neutralPose = (): JointPose => ({
  joints: Array.from({ length: PART_COUNT }, () => IDENTITY_QUAT),
  rootLift: 0,
  rootPitch: 0,
  rootRoll: 0,
});

// ── rig ────────────────────────────────────────────────────────────────────────

/** The world body transform for a player: ground position raised to the figure
 * center, yaw from facing, pitch/roll from the pose, squash from presentation
 * (0 = none, 1 = fully squashed). Ported from `rig.rs::body_transform`. */
export const bodyTransform = (ground: Vec3, facing: number, pose: JointPose, squash: number): Transform => {
  const sq = Math.min(Math.max(squash, 0), 1);
  const s = vec3(1 + sq * 0.25, 1 - sq * 0.32, 1 + sq * 0.25);
  const rotation = quatMul(quatFromEulerXyz(0, facing, 0), quatFromEulerXyz(pose.rootPitch, 0, pose.rootRoll));
  return transform(vec3(ground.x, ground.y + FIGURE_CENTER_Y + pose.rootLift, ground.z), rotation, s);
};

/** One resolved figure part ready to render: a unit-cube box (or unit-diameter
 * `sphere`) at `transform`, its `tag` selecting the material. */
export interface PosedPart {
  readonly transform: Transform;
  readonly tag: number;
  readonly mesh: PartMesh;
}

/** Resolve every part to world space: the joint chain under the body transform,
 * with each part's box offset/extent baked in (matching the Rust figure facade's
 * `posed_parts`). The returned `transform.scale` is the full box extent — a
 * `SceneInstance` of mesh `box` uses it directly. */
export const posedParts = (pose: JointPose, body: Transform): PosedPart[] => {
  const locals: Transform[] = [];
  for (let i = 0; i < PARTS.length; i += 1) {
    const spec = PARTS[i]!;
    const local = transform(spec.offset, pose.joints[i]!, vec3(1, 1, 1));
    locals.push(spec.parent < 0 ? local : combine(locals[spec.parent]!, local));
  }
  return PARTS.map((spec, i) => {
    const world = combine(body, locals[i]!);
    const boxWorld = combine(world, transform(spec.boxOffset, IDENTITY_QUAT, vec3(1, 1, 1)));
    // The rendered primitive: joint world pose, scaled to the part's full extents.
    return { mesh: spec.mesh, tag: spec.tag, transform: transform(boxWorld.position, boxWorld.rotation, mul3(boxWorld.scale, spec.boxSize)) };
  });
};

const mul3 = (a: Vec3, b: Vec3): Vec3 => vec3(a.x * b.x, a.y * b.y, a.z * b.z);

// ── two-bone leg solver (leg.rs) ─────────────────────────────────────────────────

/** One leg's fixed segment lengths, read from the shared model so the solver and
 * the rendered rig can never disagree on limb proportion. */
export interface LegDims {
  readonly thigh: number;
  readonly shin: number;
}

const vlen = (v: Vec3): number => Math.hypot(v.x, v.y, v.z);

/** The thigh (hip→knee) and shin (knee→ankle) lengths are the two segment joint
 * offsets in the model. */
export const legDims = (): LegDims => ({
  shin: Math.max(vlen(PARTS[L_THIGH + 2]!.offset), vlen(PARTS[R_THIGH + 2]!.offset)),
  thigh: Math.max(vlen(PARTS[L_THIGH + 1]!.offset), vlen(PARTS[R_THIGH + 1]!.offset)),
});

/** The result of one leg solve: local thigh/shin joint rotations, the world ankle
 * the solve actually reaches, and whether the target was clamped in-reach. */
export interface LegSolve {
  readonly thigh: Quat;
  readonly shin: Quat;
  readonly ankle: Vec3;
  readonly clamped: boolean;
}

/** Solve one leg. `parentRot` is the world rotation of the pelvis frame the thigh
 * hangs from; `hipWorld` the thigh pivot's world position; `ankleTarget` the
 * desired world ankle; `kneeForward` the world direction the knee bends toward
 * (the player's facing), which disambiguates the two-bone solution and prevents
 * knee inversion. Ported from `leg.rs::solve`. */
export const solveLeg = (dims: LegDims, parentRot: Quat, hipWorld: Vec3, ankleTarget: Vec3, kneeForward: Vec3): LegSolve => {
  const a = Math.max(dims.thigh, 1e-3);
  const b = Math.max(dims.shin, 1e-3);
  const reach = a + b;
  const minReach = Math.abs(a - b) + 1e-3;

  const inv = quatInverse(parentRot);
  const toTarget = quatRotate(inv, sub(ankleTarget, hipWorld));
  const forwardLocal = quatRotate(inv, kneeForward);

  const rawLen = vlen(toTarget);
  const clamped = rawLen > reach - 1e-3 || rawLen < minReach;
  const d = Math.min(Math.max(rawLen, minReach), reach - 1e-3);
  const dir = normalizeOr(toTarget, vec3(0, -1, 0));

  const hinge = normalizeOr(cross(dir, forwardLocal), normalizeOr(cross(dir, vec3(1, 0, 0)), vec3(1, 0, 0)));

  const cosHip = Math.min(Math.max((a * a + d * d - b * b) / (2 * a * d), -1), 1);
  const hipAngle = Math.acos(cosHip);

  const lift = quatFromAxisAngle(hinge, hipAngle);
  const thighDir = normalizeOr(quatRotate(lift, dir), dir);
  const kneeWorld = add3(hipWorld, quatRotate(parentRot, scale(thighDir, a)));

  const ankle = add3(hipWorld, quatRotate(parentRot, scale(dir, d)));
  const shinDirWorld = normalizeOr(sub(ankle, kneeWorld), quatRotate(parentRot, thighDir));
  const shinDir = quatRotate(inv, shinDirWorld);

  const rest = vec3(0, -1, 0);
  const thigh = rotationBetween(rest, thighDir);
  const shinInThigh = quatRotate(quatInverse(thigh), shinDir);
  const shin = rotationBetween(rest, shinInThigh);

  return { ankle, clamped, shin, thigh };
};

const add3 = (a: Vec3, b: Vec3): Vec3 => vec3(a.x + b.x, a.y + b.y, a.z + b.z);

export { combine, distance, dot, transformPoint };
export type { Transform };
