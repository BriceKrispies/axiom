/*
 * The kicker — a box-man that runs up and strikes the ball.
 *
 * The Rust game evaluates a 9-phase authored kick through the engine's
 * animation-authoring FK, posing a binary `.figure` asset. That asset's box
 * dimensions do not live in source, and the sampler's overwrite-from-bind
 * semantics are asset-dependent, so — as the port spec sanctions — this is a
 * visually-equivalent kick authored from scratch to the same phase timing:
 *
 *   setup → sprint (run-up, 2-stride gait) → pre-plant → plant → backswing
 *   (cocked at ~t44) → hip drive → strike (ball contact at tick 55) →
 *   follow-through → recover.  Total clip = 74 ticks; aim-independent, sampled
 *   by a fractional tick the gameplay derives from the shot state.
 *
 * A simplified 13-bone hierarchy (pelvis → chest → head/arms, pelvis → legs) is
 * posed by joint rotations, composed by TRS, then mapped into world scene space
 * at the kicker spot facing the goal (−Z).
 */

import { type Quat, type Transform, type Vec3, add, combine, fromTranslation, quatFromEulerXyz, sampleCurve as curve, vec3 } from "./engine.ts";
import type { MaterialName } from "./palette.ts";

export const KICK_DURATION_TICKS = 74;
export const STRIKE_CONTACT_TICK = 55;
const IDLE_FRAME = 4;
const DISPLAY_FRAME = 44;

const KICKER_X = -0.7;
const KICKER_Z = 12.6;

// ── the 13 rendered bones (simplified hierarchy) ─────────────────────────────

type Bone =
  | "pelvis"
  | "chest"
  | "head"
  | "leftUpperArm"
  | "leftForearm"
  | "rightUpperArm"
  | "rightForearm"
  | "leftThigh"
  | "leftShin"
  | "leftFoot"
  | "rightThigh"
  | "rightShin"
  | "rightFoot";

const BONES: readonly Bone[] = [
  "pelvis",
  "chest",
  "head",
  "leftUpperArm",
  "leftForearm",
  "rightUpperArm",
  "rightForearm",
  "leftThigh",
  "leftShin",
  "leftFoot",
  "rightThigh",
  "rightShin",
  "rightFoot",
];

const PARENT: Record<Bone, Bone | null> = {
  pelvis: null,
  chest: "pelvis",
  head: "chest",
  leftUpperArm: "chest",
  leftForearm: "leftUpperArm",
  rightUpperArm: "chest",
  rightForearm: "rightUpperArm",
  leftThigh: "pelvis",
  leftShin: "leftThigh",
  leftFoot: "leftShin",
  rightThigh: "pelvis",
  rightShin: "rightThigh",
  rightFoot: "rightShin",
};

// Bind offsets (figure space: +X = character left, +Y = up, +Z = forward toward goal).
const BIND: Record<Bone, Vec3> = {
  pelvis: vec3(0, 0.92, 0),
  chest: vec3(0, 0.45, 0),
  head: vec3(0, 0.36, 0),
  leftUpperArm: vec3(0.24, 0.3, 0),
  leftForearm: vec3(0, -0.28, 0),
  rightUpperArm: vec3(-0.24, 0.3, 0),
  rightForearm: vec3(0, -0.28, 0),
  leftThigh: vec3(0.13, -0.05, 0),
  leftShin: vec3(0, -0.45, 0),
  leftFoot: vec3(0, -0.42, 0.06),
  rightThigh: vec3(-0.13, -0.05, 0),
  rightShin: vec3(0, -0.45, 0),
  rightFoot: vec3(0, -0.42, 0.06),
};

// Box center offset from the joint (down the bone) + full box extents.
const BOX_OFFSET: Record<Bone, Vec3> = {
  pelvis: vec3(0, 0, 0),
  chest: vec3(0, 0, 0),
  head: vec3(0, 0.12, 0),
  leftUpperArm: vec3(0, -0.14, 0),
  leftForearm: vec3(0, -0.14, 0),
  rightUpperArm: vec3(0, -0.14, 0),
  rightForearm: vec3(0, -0.14, 0),
  leftThigh: vec3(0, -0.22, 0),
  leftShin: vec3(0, -0.21, 0),
  leftFoot: vec3(0, -0.03, 0.08),
  rightThigh: vec3(0, -0.22, 0),
  rightShin: vec3(0, -0.21, 0),
  rightFoot: vec3(0, -0.03, 0.08),
};

const BOX_SIZE: Record<Bone, Vec3> = {
  pelvis: vec3(0.32, 0.26, 0.24),
  chest: vec3(0.35, 0.5, 0.26),
  head: vec3(0.24, 0.28, 0.24),
  leftUpperArm: vec3(0.13, 0.3, 0.13),
  leftForearm: vec3(0.11, 0.28, 0.11),
  rightUpperArm: vec3(0.13, 0.3, 0.13),
  rightForearm: vec3(0.11, 0.28, 0.11),
  leftThigh: vec3(0.17, 0.46, 0.17),
  leftShin: vec3(0.15, 0.42, 0.15),
  leftFoot: vec3(0.15, 0.11, 0.28),
  rightThigh: vec3(0.17, 0.46, 0.17),
  rightShin: vec3(0.15, 0.42, 0.15),
  rightFoot: vec3(0.15, 0.11, 0.28),
};

const BOX_MATERIAL: Record<Bone, MaterialName> = {
  pelvis: "KickerShortsWhite",
  chest: "KickerJerseyBlue",
  head: "KickerHair",
  leftUpperArm: "KickerJerseyBlue",
  leftForearm: "KickerSkin",
  rightUpperArm: "KickerJerseyBlue",
  rightForearm: "KickerSkin",
  leftThigh: "KickerSkin",
  leftShin: "KickerSocksBlue",
  leftFoot: "KickerShoes",
  rightThigh: "KickerSkin",
  rightShin: "KickerSocksBlue",
  rightFoot: "KickerShoes",
};

// ── animation channels ───────────────────────────────────────────────────────

/** A 0→1 window that is 1 across the run-up and ramps to 0 at the plant. */
const gaitEnvelope = (t: number): number => curve([[3, 0], [6, 1], [17, 1], [22, 0]], t);

/** The gait oscillator phase over the 2-stride run-up (ticks 4..20). */
const gaitPhase = (t: number): number => ((t - 4) / 16) * 2 * Math.PI * 2;

// Right leg = the kicking leg; left leg = the plant leg.
const rightThighX = (t: number): number =>
  curve([[0, 0], [20, 0.05], [28, 0.12], [36, 0.28], [44, 0.72], [52, 0.42], [55, -0.12], [60, -0.95], [68, -1.3], [74, -0.35]], t);
const rightShinX = (t: number): number =>
  curve([[0, -0.15], [28, -0.2], [36, -0.35], [44, -0.75], [52, -0.5], [55, -0.12], [60, -0.5], [74, -0.45]], t);
const rightFootX = (t: number): number => curve([[0, 0], [44, -0.25], [55, 0.15], [60, -0.2], [74, 0]], t);

const leftThighX = (t: number): number => curve([[0, 0], [22, 0.14], [36, 0.1], [74, 0.06]], t);
const leftShinX = (t: number): number => curve([[0, -0.15], [28, -0.32], [60, -0.3], [74, -0.25]], t);

const chestPitchX = (t: number): number => curve([[0, 0.05], [12, 0.28], [36, 0.24], [55, 0.16], [68, 0.1], [74, 0.06]], t);
const chestYawY = (t: number): number => curve([[0, 0], [36, 0.15], [46, 0.05], [55, -0.2], [68, -0.12], [74, 0]], t);
const pelvisYawY = (t: number): number => curve([[0, 0], [28, -0.06], [40, 0.12], [50, -0.1], [60, -0.14], [74, 0]], t);

// Arm balance spread (Z) ramps in at the plant and holds through the strike.
const armSpread = (t: number): number => curve([[0, 0.1], [28, 0.7], [52, 0.87], [64, 0.6], [74, 0.35]], t);

const localRotation = (bone: Bone, t: number): Quat => {
  const env = gaitEnvelope(t);
  const swing = Math.sin(gaitPhase(t)) * env;
  const lift = Math.max(Math.sin(gaitPhase(t) + Math.PI / 2), 0) * env;
  switch (bone) {
    case "pelvis":
      return quatFromEulerXyz(0, pelvisYawY(t), 0);
    case "chest":
      return quatFromEulerXyz(chestPitchX(t), chestYawY(t), 0);
    case "head":
      return quatFromEulerXyz(-chestPitchX(t) * 0.5, 0, 0);
    case "leftThigh":
      return quatFromEulerXyz(leftThighX(t) + swing * 0.7, 0, 0);
    case "leftShin":
      return quatFromEulerXyz(leftShinX(t) - lift * 0.6, 0, 0);
    case "leftFoot":
      return quatFromEulerXyz(0.1, 0, 0);
    case "rightThigh":
      return quatFromEulerXyz(rightThighX(t) - swing * 0.7, 0, 0);
    case "rightShin":
      return quatFromEulerXyz(rightShinX(t) - Math.max(-Math.sin(gaitPhase(t) + Math.PI / 2), 0) * env * 0.6, 0, 0);
    case "rightFoot":
      return quatFromEulerXyz(rightFootX(t), 0, 0);
    case "leftUpperArm":
      return quatFromEulerXyz(-swing * 0.7, 0, armSpread(t) * (1 - env));
    case "leftForearm":
      return quatFromEulerXyz(-0.6 - lift * 0.3, 0, 0);
    case "rightUpperArm":
      return quatFromEulerXyz(swing * 0.7, 0, -armSpread(t) * (1 - env));
    case "rightForearm":
      return quatFromEulerXyz(-0.6, 0, 0);
  }
};

// The run-up: root travels from 3.2 behind the kicker spot to the spot over ticks 4..20.
const rootZ = (t: number): number => curve([[0, -3.2], [4, -3.2], [20, 0], [74, 0]], t);

const localTransform = (bone: Bone, t: number): Transform => ({
  translation: BIND[bone],
  rotation: localRotation(bone, t),
  scale: vec3(1, 1, 1),
});

/** A posed kicker box already in world scene space. */
export interface KickerBox {
  readonly position: Vec3;
  readonly rotation: Quat;
  readonly scale: Vec3;
  readonly material: MaterialName;
}

const mirrorZ = (q: Quat): Quat => [-q[0], -q[1], q[2], q[3]];

/** Pose all 13 kicker boxes at (fractional) authored tick `t`, mapped to world space. */
export const kickerBoxesAt = (t: number): KickerBox[] => {
  const rootTransform: Transform = { translation: vec3(0, 0, rootZ(t)), rotation: [0, 0, 0, 1], scale: vec3(1, 1, 1) };
  const world = new Map<Bone, Transform>();
  for (const bone of BONES) {
    const parent = PARENT[bone];
    const parentWorld = parent ? world.get(parent)! : rootTransform;
    world.set(bone, combine(parentWorld, localTransform(bone, t)));
  }
  return BONES.map((bone): KickerBox => {
    const boxWorld = combine(world.get(bone)!, fromTranslation(BOX_OFFSET[bone]));
    const p = boxWorld.translation;
    return {
      position: vec3(KICKER_X + p.x, p.y, KICKER_Z - p.z),
      rotation: mirrorZ(boxWorld.rotation),
      scale: BOX_SIZE[bone],
      material: BOX_MATERIAL[bone],
    };
  });
};

/** The kicker's hair cap box (world space) at authored tick `t`, tracking the head. */
export const kickerHairAt = (t: number): KickerBox => {
  const boxes = kickerBoxesAt(t);
  const head = boxes[BONES.indexOf("head")]!;
  return { position: add(head.position, vec3(0, 0.16, 0)), rotation: head.rotation, scale: vec3(0.26, 0.14, 0.26), material: "KickerHair" };
};

// ── gameplay state → authored tick ───────────────────────────────────────────

const RUNUP_PLAYBACK_RATE = 0.5;
const RUNUP_STRIKE_GAP = 3;

/** Map the shot state to the authored kick tick to display. */
export const kickerFrameTick = (opts: {
  state: "Aiming" | "Charging" | "LockedPreview" | "BallInFlight" | "ContactDetected" | "ArrivedAtGoalPlane" | "Resolved";
  chargeTicks: number;
  flightProgress: number;
}): number => {
  if (opts.state === "Aiming") return IDLE_FRAME;
  if (opts.state === "Charging") return Math.min(IDLE_FRAME + opts.chargeTicks * RUNUP_PLAYBACK_RATE, STRIKE_CONTACT_TICK - RUNUP_STRIKE_GAP);
  return STRIKE_CONTACT_TICK + (KICK_DURATION_TICKS - 1 - STRIKE_CONTACT_TICK) * opts.flightProgress;
};

export const KICKER_DISPLAY_FRAME = DISPLAY_FRAME;
