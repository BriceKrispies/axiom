/*
 * cinematic-camera.ts — the home-run cinematic's camera director: pure functions
 * from (batter transform | ball transform) + `HOME_RUN_CINEMATIC_TUNING` to a
 * camera pose. No engine call, no handle, no hidden state — every function is a
 * total, deterministic `(inputs, tuning) → pose`, exactly like `swing.ts`'s
 * `batDir`/`batPlaneY`. `session.ts` owns blending between these poses and the
 * ordinary gameplay camera; this file only answers "where does the camera want
 * to be right now."
 */

import { type Vec3, clamp01, mix, vec3 } from "./vec.ts";
import type { BatterPosition } from "./types.ts";
import type { HomeRunCinematicTuning } from "./cinematic-constants.ts";
import * as C from "./constants.ts";

export interface CameraPose {
  readonly position: Vec3;
  readonly target: Vec3;
}

/**
 * The low-angle contact camera: down and to the batter's right, close to the
 * ground, slightly behind, looking up toward the contact area. Derived entirely
 * from the batter's transform and the tuning offsets — never a hardcoded
 * world-space position, so it tracks wherever the batter is standing in the box.
 */
export const contactCameraPose = (batter: BatterPosition, tuning: HomeRunCinematicTuning): CameraPose => ({
  position: vec3(batter.x + tuning.lowCameraLateralOffset, tuning.lowCameraHeight, batter.z - tuning.lowCameraBackwardOffset),
  target: vec3(batter.x - tuning.lowCameraLateralOffset * 0.4, tuning.lowCameraLookAtHeight, batter.z + 1.1),
});

/**
 * The ground-tracking camera: planted behind the batter, NOT following the ball
 * through the air — it pivots in place to keep pointing at wherever the ball
 * currently is. `session.ts` stops calling this (freezing the last computed
 * pose) the instant the ball clears the outfield wall, so the shot never chases
 * the ball out of the park.
 */
export const groundTrackingCameraPose = (batter: BatterPosition, ballPos: Vec3, tuning: HomeRunCinematicTuning): CameraPose => ({
  position: vec3(batter.x + tuning.groundCameraLateralOffset, tuning.groundCameraHeight, batter.z - tuning.groundCameraBackwardOffset),
  target: ballPos,
});

/** The ground-tracking camera's zoom TARGET (0…1, fed through `cinematicFovY`
 * alongside the contact zoom) — zoomed out while the ball climbs, zoomed in a
 * bit once it starts falling so it stays readable against the sky. */
export const groundTrackingZoomTarget = (ballVel: Vec3, tuning: HomeRunCinematicTuning): number =>
  ballVel.y < 0 ? tuning.groundCameraDescentZoomAmount : 0;

/** The cinematic zoom's effect on vertical FOV — narrower at `zoomBlend === 1`. */
export const cinematicFovY = (zoomBlend: number, tuning: HomeRunCinematicTuning): number =>
  mix(C.CAMERA_FOV_Y, C.CAMERA_FOV_Y * (1 - tuning.cinematicZoomAmount), clamp01(zoomBlend));
