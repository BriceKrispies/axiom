/*
 * The fake-perspective (pseudo-3D) camera. The world is modelled in path-space —
 * forward distance `z`, lateral world-x, and height `wy` above the ground — and this
 * projects a world point to the screen the way a chase camera behind and above the
 * runner would see it: distant points shrink toward the horizon, the path narrows,
 * and the centerline curves. It is deliberately simple arithmetic (one focal
 * divide), not a real 3D pipeline — the brief asks for readable fake perspective.
 */

import { HEIGHT, WIDTH } from "./constants.ts";
import { centerlineAt } from "./level.ts";
import type { State } from "./types.ts";

/** Screen-space y of the vanishing horizon. */
export const HORIZON = HEIGHT * 0.44;

const CAM_BACK = 130; // how far behind the runner the camera sits (world units)
const CAM_HEIGHT = 235; // camera height above the ground plane
const FOCAL = 760; // focal length (larger = flatter / more zoomed)
const NEAR = 20; // near clip in world units (points closer are culled)
const FOLLOW = 0.5; // how much the camera tracks the runner's lateral offset

/** A projected point plus the depth scale to size world objects at that depth. */
export interface Projected {
  readonly x: number;
  readonly y: number;
  /** Screen units per world unit at this depth. */
  readonly scale: number;
  /** True when the point is in front of the near plane (safe to draw). */
  readonly visible: boolean;
}

/** The resolved camera for one frame. */
export interface Camera {
  readonly camX: number;
  readonly camZ: number;
}

/** Resolve the chase camera from the runner's position. */
export const makeCamera = (state: State): Camera => {
  const dist = state.runner.dist;
  return {
    camX: centerlineAt(state.level.nodes, dist) + state.runner.lateral * FOLLOW,
    camZ: dist - CAM_BACK,
  };
};

/** Project a world point `(wx, z, wy)` through `cam` to the screen. */
export const project = (cam: Camera, wx: number, z: number, wy = 0): Projected => {
  const dz = z - cam.camZ;
  const scale = FOCAL / Math.max(NEAR, dz);
  return {
    scale,
    visible: dz > NEAR,
    x: WIDTH / 2 + (wx - cam.camX) * scale,
    y: HORIZON + (CAM_HEIGHT - wy) * scale,
  };
};
