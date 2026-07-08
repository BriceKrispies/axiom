/*
 * Shot-result classification — a faithful port of `penalty_result.rs`.
 *
 * Strict priority: goalie contact (Save) → post/crossbar frame hit (Post) →
 * inside the goal mouth (Goal) → else Miss. Contact is detected during flight, so
 * it precedes the goal-plane crossing tests structurally: `resolveFromContact` is
 * a save, `resolveFromCrossing` handles post/goal/miss at the plane.
 */

import { type Vec3, ballBoxContact } from "./engine.ts";
import type { ContactFrame, VolumeKind } from "./goalie.ts";
import { BALL_RADIUS, GOAL_HALF_WIDTH, GOAL_HEIGHT, GROUND_Y, POST_THICKNESS } from "./scene-constants.ts";

export type ResultKind = "Goal" | "Save" | "Miss" | "Post";
export type ResultDetail =
  | "Scored"
  | "SavedByLeftHand"
  | "SavedByRightHand"
  | "SavedByTorso"
  | "SavedByBody"
  | "HitLeftPost"
  | "HitRightPost"
  | "HitCrossbar"
  | "MissedLeft"
  | "MissedRight"
  | "MissedHigh"
  | "MissedWideOrHigh";

export interface ShotResult {
  readonly kind: ResultKind;
  readonly detail: ResultDetail;
}

// ── goal mouth + frame volumes ───────────────────────────────────────────────

/** Ball-CENTER inside the goal mouth (strict on X, inclusive on Y). */
const insideMouth = (x: number, y: number): boolean =>
  x > -GOAL_HALF_WIDTH && x < GOAL_HALF_WIDTH && y >= GROUND_Y && y <= GOAL_HEIGHT;

type FrameKind = "LeftPost" | "RightPost" | "Crossbar";
interface FrameVolume {
  readonly kind: FrameKind;
  readonly center: Vec3;
  readonly half: Vec3;
}
const POST_HALF: Vec3 = { x: POST_THICKNESS, y: GOAL_HEIGHT * 0.5, z: POST_THICKNESS }; // (0.12, 1.22, 0.12)
const BAR_HALF: Vec3 = { x: GOAL_HALF_WIDTH + POST_THICKNESS, y: POST_THICKNESS, z: POST_THICKNESS }; // (3.78, 0.12, 0.12)
const FRAME_VOLUMES: readonly FrameVolume[] = [
  { kind: "LeftPost", center: { x: -GOAL_HALF_WIDTH, y: 1.22, z: 0 }, half: POST_HALF },
  { kind: "RightPost", center: { x: GOAL_HALF_WIDTH, y: 1.22, z: 0 }, half: POST_HALF },
  { kind: "Crossbar", center: { x: 0, y: GOAL_HEIGHT, z: 0 }, half: BAR_HALF },
];

const frameHit = (ballCenter: Vec3): FrameKind | null => {
  for (const volume of FRAME_VOLUMES) {
    if (ballBoxContact(ballCenter, BALL_RADIUS, volume.center, volume.half)) return volume.kind;
  }
  return null;
};

export interface GoalPlaneCrossing {
  readonly ballPosition: Vec3;
  readonly insideMouth: boolean;
  readonly frameHit: FrameKind | null;
}

export const goalPlaneCrossing = (ballPosition: Vec3): GoalPlaneCrossing => ({
  ballPosition,
  insideMouth: insideMouth(ballPosition.x, ballPosition.y),
  frameHit: frameHit(ballPosition),
});

// ── resolvers ────────────────────────────────────────────────────────────────

const saveDetail = (kind: VolumeKind): ResultDetail =>
  kind === "LeftHand" ? "SavedByLeftHand" : kind === "RightHand" ? "SavedByRightHand" : kind === "Torso" ? "SavedByTorso" : "SavedByBody";

export const resolveFromContact = (frame: ContactFrame): ShotResult => ({
  kind: "Save",
  detail: frame.contact ? saveDetail(frame.contact.volumeKind) : "SavedByBody",
});

const postDetail = (kind: FrameKind): ResultDetail =>
  kind === "LeftPost" ? "HitLeftPost" : kind === "RightPost" ? "HitRightPost" : "HitCrossbar";

const missDetail = (pos: Vec3): ResultDetail =>
  pos.x < -GOAL_HALF_WIDTH ? "MissedLeft" : pos.x > GOAL_HALF_WIDTH ? "MissedRight" : pos.y > GOAL_HEIGHT ? "MissedHigh" : "MissedWideOrHigh";

export const resolveFromCrossing = (crossing: GoalPlaneCrossing): ShotResult => {
  if (crossing.frameHit) return { kind: "Post", detail: postDetail(crossing.frameHit) };
  if (crossing.insideMouth) return { kind: "Goal", detail: "Scored" };
  return { kind: "Miss", detail: missDetail(crossing.ballPosition) };
};

export interface ResolvedShotState {
  readonly result: ShotResult;
  readonly finalBallPosition: Vec3;
  readonly crossing: GoalPlaneCrossing | null;
}

// ── HUD text ─────────────────────────────────────────────────────────────────

export const resultText = (kind: ResultKind): string => kind.toUpperCase();

export const detailText = (detail: ResultDetail): string | null => {
  switch (detail) {
    case "Scored":
      return null;
    case "SavedByLeftHand":
      return "LEFT HAND";
    case "SavedByRightHand":
      return "RIGHT HAND";
    case "SavedByTorso":
      return "TORSO";
    case "SavedByBody":
      return "BODY";
    case "HitLeftPost":
      return "LEFT POST";
    case "HitRightPost":
      return "RIGHT POST";
    case "HitCrossbar":
      return "CROSSBAR";
    case "MissedLeft":
      return "WIDE LEFT";
    case "MissedRight":
      return "WIDE RIGHT";
    case "MissedHigh":
      return "TOO HIGH";
    case "MissedWideOrHigh":
      return "WIDE";
  }
};
