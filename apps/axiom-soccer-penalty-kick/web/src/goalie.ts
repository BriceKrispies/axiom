/*
 * The goalie — a faithful port of `penalty_goalie.rs` + `penalty_goalie_pose.rs`.
 *
 * Three things live here:
 *   1. A 16-part articulated box puppet (hierarchy resolved by TRS composition),
 *      with a render-only braced idle stance and five authored 24-tick dive clips.
 *   2. The dive-lane selector that maps the locked aim to one of five clips.
 *   3. The save volumes (2 glove spheres + torso/body boxes) that ride the posed
 *      rig, and the priority-ordered sphere/box contact test the ball is tested
 *      against each flight tick.
 *
 * Collision rides the un-rotated idle rig at rest and the dive clip during a dive
 * (`collisionWorld`); the braced `idle_display` stance is render-only.
 */

import {
  type Transform,
  type Vec3,
  add,
  ballBoxContact,
  ballSphereContact,
  fromTranslation,
  quatFromEulerXyz,
  resolveHierarchy,
  vec3,
} from "./engine.ts";
import type { MaterialName } from "./palette.ts";
import { BALL_RADIUS, GOALIE_X, GOALIE_Z } from "./scene-constants.ts";

// ── the 16-part rig ──────────────────────────────────────────────────────────

export const PART_COUNT = 16;
const ROOT = 0;
const PELVIS = 1;
const TORSO = 2;
const LEFT_UPPER_ARM = 4;
const LEFT_FOREARM = 5;
const LEFT_HAND = 6;
const RIGHT_UPPER_ARM = 7;
const RIGHT_FOREARM = 8;
const RIGHT_HAND = 9;
const LEFT_THIGH = 10;
const LEFT_SHIN = 11;
const RIGHT_THIGH = 13;
const RIGHT_SHIN = 14;

const PARENT: readonly number[] = [-1, 0, 1, 2, 2, 4, 5, 2, 7, 8, 1, 10, 11, 1, 13, 14];

const IDLE_LOCAL: readonly Vec3[] = [
  vec3(GOALIE_X, 0, GOALIE_Z), // Root
  vec3(0, 0.92, 0), // Pelvis
  vec3(0, 0.4, 0), // Torso
  vec3(0, 0.52, 0), // Head
  vec3(-0.36, 0.28, 0), // LeftUpperArm
  vec3(-0.12, -0.28, 0), // LeftForearm
  vec3(-0.1, -0.28, 0), // LeftHand
  vec3(0.36, 0.28, 0), // RightUpperArm
  vec3(0.12, -0.28, 0), // RightForearm
  vec3(0.1, -0.28, 0), // RightHand
  vec3(-0.14, -0.1, 0), // LeftThigh
  vec3(0, -0.42, 0), // LeftShin
  vec3(0, -0.36, 0.06), // LeftFoot
  vec3(0.14, -0.1, 0), // RightThigh
  vec3(0, -0.42, 0), // RightShin
  vec3(0, -0.36, 0.06), // RightFoot
];

/** Full box extents per part; Root is invisible (skipped when rendering). */
export const PART_SIZE: readonly Vec3[] = [
  vec3(0, 0, 0), // Root
  vec3(0.34, 0.26, 0.26), // Pelvis
  vec3(0.5, 0.6, 0.3), // Torso
  vec3(0.26, 0.28, 0.26), // Head
  vec3(0.16, 0.34, 0.16), // LeftUpperArm
  vec3(0.14, 0.3, 0.14), // LeftForearm
  vec3(0.18, 0.18, 0.18), // LeftHand
  vec3(0.16, 0.34, 0.16), // RightUpperArm
  vec3(0.14, 0.3, 0.14), // RightForearm
  vec3(0.18, 0.18, 0.18), // RightHand
  vec3(0.2, 0.44, 0.2), // LeftThigh
  vec3(0.18, 0.4, 0.18), // LeftShin
  vec3(0.18, 0.12, 0.28), // LeftFoot
  vec3(0.2, 0.44, 0.2), // RightThigh
  vec3(0.18, 0.4, 0.18), // RightShin
  vec3(0.18, 0.12, 0.28), // RightFoot
];

export const PART_MATERIAL: readonly MaterialName[] = [
  "GoalieShortsBlack", // Root (unused)
  "GoalieShortsBlack", // Pelvis
  "GoalieJerseyYellow", // Torso
  "GoalieSkin", // Head
  "GoalieJerseyYellow", // LeftUpperArm
  "GoalieJerseyYellow", // LeftForearm
  "GoalieGloves", // LeftHand
  "GoalieJerseyYellow", // RightUpperArm
  "GoalieJerseyYellow", // RightForearm
  "GoalieGloves", // RightHand
  "GoalieShortsBlack", // LeftThigh
  "GoalieSocks", // LeftShin
  "GoalieShoes", // LeftFoot
  "GoalieShortsBlack", // RightThigh
  "GoalieSocks", // RightShin
  "GoalieShoes", // RightFoot
];

/** The plain translation-only idle locals (collision rest pose). */
const idleLocals = (): Transform[] => IDLE_LOCAL.map(fromTranslation);

/** The render-only braced stance: idle translations + authored arm/leg rotations. */
const idleDisplayLocals = (): Transform[] => {
  const locals = idleLocals();
  const rot = (i: number, x: number, y: number, z: number): void => {
    locals[i] = { translation: IDLE_LOCAL[i]!, rotation: quatFromEulerXyz(x, y, z), scale: vec3(1, 1, 1) };
  };
  rot(LEFT_UPPER_ARM, 0.1, 0, -1.05);
  rot(RIGHT_UPPER_ARM, 0.1, 0, 1.05);
  rot(LEFT_FOREARM, 0.35, 0, 0.72);
  rot(RIGHT_FOREARM, 0.35, 0, -0.72);
  rot(LEFT_THIGH, 0.2, 0, -0.3);
  rot(RIGHT_THIGH, 0.2, 0, 0.3);
  rot(LEFT_SHIN, -0.8, 0, 0);
  rot(RIGHT_SHIN, -0.8, 0, 0);
  return locals;
};

/** Resolve local transforms into world transforms (parents precede children). */
export const resolvePose = (local: readonly Transform[]): Transform[] => resolveHierarchy(PARENT, local);

// ── dive lanes + clips ───────────────────────────────────────────────────────

export type DiveLane = "LeftLow" | "LeftHigh" | "RightLow" | "RightHigh" | "Center";

export const selectDiveLane = (tx: number, ty: number): DiveLane => {
  if (tx < -35 && ty < 50) return "LeftLow";
  if (tx < -35) return "LeftHigh";
  if (tx > 35 && ty < 50) return "RightLow";
  if (tx > 35) return "RightHigh";
  return "Center";
};

const laneParams = (lane: DiveLane): { side: number; vert: number } => {
  switch (lane) {
    case "LeftLow":
      return { side: -1, vert: -1 };
    case "LeftHigh":
      return { side: -1, vert: 1 };
    case "RightLow":
      return { side: 1, vert: -1 };
    case "RightHigh":
      return { side: 1, vert: 1 };
    case "Center":
      return { side: 0, vert: 0 };
  }
};

export const CLIP_DURATION_TICKS = 24;
const KEYFRAMES: readonly { tick: number; m: number; crouch: number }[] = [
  { tick: 0, m: 0.0, crouch: 0.0 },
  { tick: 4, m: -0.1, crouch: -0.2 },
  { tick: 9, m: 0.45, crouch: -0.05 },
  { tick: 16, m: 1.0, crouch: 0.0 },
  { tick: 24, m: 0.82, crouch: -0.1 },
];

const vertRootY = (vert: number): number => (vert > 0 ? -0.05 : vert < 0 ? -0.55 : -0.1);
const vertHandY = (vert: number): number => (vert < 0 ? -0.2 : vert > 0 ? 0.8 : 0.75);

/** Build the dive pose's local transforms at keyframe magnitude `m` / `crouch`. */
const diveClipLocals = (lane: DiveLane, m: number, crouch: number): Transform[] => {
  const { side, vert } = laneParams(lane);
  const locals = idleLocals();
  const bump = (i: number, off: Vec3): void => {
    locals[i] = fromTranslation(add(IDLE_LOCAL[i]!, off));
  };
  bump(ROOT, vec3(side * 0.8 * m, vertRootY(vert) * m, 0));
  bump(PELVIS, vec3(0, crouch, 0));
  const handOff = vec3(side * 0.5 * m, vertHandY(vert) * m, 0);
  const armOff = vec3(side * 0.15 * m, vertHandY(vert) * 0.3 * m, 0);
  const leads =
    side > 0
      ? [[RIGHT_HAND, RIGHT_UPPER_ARM]]
      : side < 0
        ? [[LEFT_HAND, LEFT_UPPER_ARM]]
        : [
            [LEFT_HAND, LEFT_UPPER_ARM],
            [RIGHT_HAND, RIGHT_UPPER_ARM],
          ];
  for (const [hand, arm] of leads) {
    bump(hand!, handOff);
    bump(arm!, armOff);
  }
  return locals;
};

/** Nearest-frame sampler: the last keyframe whose tick ≤ min(tick, 24). Step-hold. */
const sampleClip = (lane: DiveLane, tick: number): Transform[] => {
  const t = Math.min(tick, CLIP_DURATION_TICKS);
  let frame = KEYFRAMES[0]!;
  for (const kf of KEYFRAMES) {
    if (kf.tick <= t) frame = kf;
  }
  return diveClipLocals(lane, frame.m, frame.crouch);
};

// ── animation state ──────────────────────────────────────────────────────────

export type GoalieState = "Idle" | "TrackingShot" | "Diving" | "Landed";

export interface GoalieAnimation {
  readonly state: GoalieState;
  readonly lane: DiveLane | null;
  readonly clipTick: number;
}

export const goalieIdle = (): GoalieAnimation => ({ state: "Idle", lane: null, clipTick: 0 });

export const goalieLocked = (tx: number, ty: number): GoalieAnimation => ({
  state: "TrackingShot",
  lane: selectDiveLane(tx, ty),
  clipTick: 0,
});

export const goalieAdvanced = (anim: GoalieAnimation): GoalieAnimation => {
  const clipTick = Math.min(anim.clipTick + 1, CLIP_DURATION_TICKS);
  const state: GoalieState = anim.lane ? (clipTick >= CLIP_DURATION_TICKS ? "Landed" : "Diving") : "Idle";
  return { state, lane: anim.lane, clipTick };
};

/** The collision rig world transforms: idle rig at rest, dive clip during a dive. */
export const goalieCollisionWorld = (anim: GoalieAnimation): Transform[] =>
  resolvePose(anim.lane ? sampleClip(anim.lane, anim.clipTick) : idleLocals());

/** The render rig world transforms: braced idle stance at rest, dive clip during a dive. */
export const goalieRenderWorld = (anim: GoalieAnimation): Transform[] =>
  resolvePose(anim.lane ? sampleClip(anim.lane, anim.clipTick) : idleDisplayLocals());

// ── save volumes + contact ───────────────────────────────────────────────────

export type VolumeKind = "LeftHand" | "RightHand" | "Torso" | "Body";
export type ContactKind = "None" | "Hand" | "Torso" | "Body";

const HAND_RADIUS = 0.22;
const TORSO_HALF = vec3(0.34, 0.42, 0.22);
const BODY_HALF = vec3(0.72, 1.02, 0.2);

interface Volume {
  readonly kind: VolumeKind;
  readonly ordinal: number;
  readonly sphere: boolean;
  readonly center: Vec3;
  readonly radius: number; // spheres
  readonly half: Vec3; // boxes
}

/** The 4 priority-ordered volumes riding the posed rig (Body rides the Pelvis part). */
export const goalieAnimatedVolumes = (world: readonly Transform[]): Volume[] => [
  { kind: "LeftHand", ordinal: 0, sphere: true, center: world[LEFT_HAND]!.translation, radius: HAND_RADIUS, half: vec3(0, 0, 0) },
  { kind: "RightHand", ordinal: 1, sphere: true, center: world[RIGHT_HAND]!.translation, radius: HAND_RADIUS, half: vec3(0, 0, 0) },
  { kind: "Torso", ordinal: 2, sphere: false, center: world[TORSO]!.translation, radius: 0, half: TORSO_HALF },
  { kind: "Body", ordinal: 3, sphere: false, center: world[PELVIS]!.translation, radius: 0, half: BODY_HALF },
];

const contactKindOf = (kind: VolumeKind): ContactKind =>
  kind === "LeftHand" || kind === "RightHand" ? "Hand" : kind === "Torso" ? "Torso" : "Body";

const overlap = (volume: Volume, ballCenter: Vec3): Vec3 | null =>
  volume.sphere
    ? ballSphereContact(ballCenter, BALL_RADIUS, volume.center, volume.radius)
    : ballBoxContact(ballCenter, BALL_RADIUS, volume.center, volume.half);

export interface ContactFrame {
  readonly tick: number;
  readonly ballPosition: Vec3;
  readonly contact: { kind: ContactKind; volumeKind: VolumeKind; contactPoint: Vec3 } | null;
}

/** Test the ball against the volumes in priority order; first overlap wins. */
export const detectContact = (volumes: readonly Volume[], ballCenter: Vec3, tick: number): ContactFrame => {
  for (const volume of volumes) {
    const point = overlap(volume, ballCenter);
    if (point) {
      return { tick, ballPosition: ballCenter, contact: { kind: contactKindOf(volume.kind), volumeKind: volume.kind, contactPoint: point } };
    }
  }
  return { tick, ballPosition: ballCenter, contact: null };
};

/** The world-space hair box (sits above the head) — a render extra. */
export const GOALIE_HAIR = { center: vec3(0, 1.97, GOALIE_Z), size: vec3(0.28, 0.14, 0.28), material: "GoalieHair" as MaterialName };
