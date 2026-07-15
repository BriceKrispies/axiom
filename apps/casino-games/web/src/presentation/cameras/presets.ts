/*
 * presets.ts — the reusable camera language. Every game takes its camera from
 * here; no game hand-rolls camera math.
 *
 * The MACHINE-INTERIOR preset implements the machine-camera rule: the camera
 * mounts near the UPPER-LEFT INTERIOR corner of the machine volume, aims
 * diagonally toward the center of the playable volume, faces slightly
 * downward, stays stable during normal interaction, and is allowed subtle
 * cinematic movement only during the final reveal (via `revealNudge`).
 */

import type { Camera3D, EngineVec3 } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import { clamp01, lerp, smoothstep } from "../stage/easing.ts";
import { lerpV3, v3 } from "../stage/vectors.ts";

/** The interior volume of a machine (its inner housing, not the cabinet). */
export interface MachineVolume {
  readonly center: EngineVec3;
  /** Full interior extents (x = width, y = height, z = depth toward viewer). */
  readonly size: EngineVec3;
}

/** Upper-left interior mount, aimed diagonally at the playable center. */
export const machineInteriorCamera = (volume: MachineVolume, fovY = 1.05): Camera3D => {
  const { center, size } = volume;
  return {
    far: 200,
    fovY,
    near: 0.05,
    position: v3(
      center.x - size.x * 0.38,
      center.y + size.y * 0.36,
      center.z + size.z * 0.42,
    ),
    target: v3(center.x + size.x * 0.06, center.y - size.y * 0.12, center.z - size.z * 0.1),
  };
};

/** The standard showcase framing: eye-level, slightly above, subject centered. */
export const showcaseCamera = (subject: EngineVec3, distance: number, height: number, fovY = 0.9): Camera3D => ({
  far: 400,
  fovY,
  near: 0.1,
  position: v3(subject.x, subject.y + height, subject.z + distance),
  target: subject,
});

/** Tabletop framing for 2D / card-table games: high, pitched down. */
export const tabletopCamera = (center: EngineVec3, span: number): Camera3D => ({
  far: 300,
  fovY: 0.85,
  near: 0.1,
  position: v3(center.x, center.y + span * 1.15, center.z + span * 0.85),
  target: center,
});

/** Ease the camera toward a reveal focus. `t` in [0,1]; motion is smoothstep,
 * restrained by design (subtle cinematic move, not a cut). */
export const revealFocusCamera = (base: Camera3D, focus: EngineVec3, t: number, closeness = 0.45): Camera3D => {
  const s = smoothstep(clamp01(t)) * closeness;
  return {
    ...base,
    fovY: lerp(base.fovY, base.fovY * 0.82, s),
    position: lerpV3(base.position, lerpV3(base.position, focus, 0.35), s),
    target: lerpV3(base.target, focus, s),
  };
};

/**
 * Restrained celebration shake from the CAMERA stream — never more than a few
 * centimeters, scaled by the shake setting (0 disables entirely). Pure in
 * (seed, tick), so replays shake identically.
 */
export const cameraShakeOffset = (
  camera: Camera3D,
  presentationSeed: number,
  tick: number,
  magnitude: number,
): Camera3D => {
  if (magnitude <= 0) {
    return camera;
  }
  const dx = (sample01(presentationSeed, "camera", tick, 0) - 0.5) * magnitude;
  const dy = (sample01(presentationSeed, "camera", tick, 1) - 0.5) * magnitude;
  return { ...camera, position: v3(camera.position.x + dx, camera.position.y + dy, camera.position.z) };
};
