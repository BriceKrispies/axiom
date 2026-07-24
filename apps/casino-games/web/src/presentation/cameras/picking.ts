/*
 * picking.ts — pointer hit-testing against a game's own layout. The engine
 * deliberately keeps its matrix pipeline private, so this file mirrors the
 * standard look-at + perspective mapping ONLY to answer "which of my objects
 * is under the cursor" — it renders nothing and owns no camera. Games express
 * hit targets as world-space points with a screen radius; selection state
 * (hover, focus, press) is resolved here once for every game.
 */

import type { Camera3D, EngineVec3, PointerSample } from "@axiom/web-engine";

/** The fixed logical canvas size the whole app renders at. The DOM shell
 * normalizes pointer samples into this space (see application/shell.ts). */
export const CANVAS_WIDTH = 960;
export const CANVAS_HEIGHT = 600;

const sub = (a: EngineVec3, b: EngineVec3): EngineVec3 => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z });
const cross = (a: EngineVec3, b: EngineVec3): EngineVec3 => ({
  x: a.y * b.z - a.z * b.y,
  y: a.z * b.x - a.x * b.z,
  z: a.x * b.y - a.y * b.x,
});
const dot = (a: EngineVec3, b: EngineVec3): number => a.x * b.x + a.y * b.y + a.z * b.z;
const normalize = (a: EngineVec3): EngineVec3 => {
  const len = Math.sqrt(dot(a, a)) || 1;
  return { x: a.x / len, y: a.y / len, z: a.z / len };
};

/** Project a world point to logical canvas coordinates (y down), or null when
 * the point is behind the camera. */
export const worldToCanvas = (camera: Camera3D, point: EngineVec3): { readonly x: number; readonly y: number } | null => {
  const forward = normalize(sub(camera.target, camera.position));
  const right = normalize(cross(forward, { x: 0, y: 1, z: 0 }));
  const up = cross(right, forward);
  const rel = sub(point, camera.position);
  const zCam = dot(rel, forward);
  if (zCam <= camera.near) {
    return null;
  }
  const xCam = dot(rel, right);
  const yCam = dot(rel, up);
  const halfTan = Math.tan(camera.fovY / 2);
  const aspect = CANVAS_WIDTH / CANVAS_HEIGHT;
  const ndcX = xCam / (zCam * halfTan * aspect);
  const ndcY = yCam / (zCam * halfTan);
  return { x: (ndcX * 0.5 + 0.5) * CANVAS_WIDTH, y: (0.5 - ndcY * 0.5) * CANVAS_HEIGHT };
};

/** Un-project a pointer sample onto the ground plane (y = 0), or null when the
 * cursor is undefined or its ray does not meet the plane in front of the camera.
 * The exact inverse of `worldToCanvas` for the y = 0 plane — it answers "where on
 * the ground is the cursor", which is what placing a dragged object needs. */
export const canvasToGround = (camera: Camera3D, pointer: PointerSample | undefined): EngineVec3 | null => {
  if (pointer === undefined) {
    return null;
  }
  const forward = normalize(sub(camera.target, camera.position));
  const right = normalize(cross(forward, { x: 0, y: 1, z: 0 }));
  const up = cross(right, forward);
  const halfTan = Math.tan(camera.fovY / 2);
  const aspect = CANVAS_WIDTH / CANVAS_HEIGHT;
  const ndcX = (pointer.pos.x / CANVAS_WIDTH) * 2 - 1;
  const ndcY = 1 - (pointer.pos.y / CANVAS_HEIGHT) * 2;
  // Camera-space ray direction (xCam:yCam:zCam ∝ this), lifted into world space
  // through the same basis worldToCanvas projects with.
  const cx = ndcX * halfTan * aspect;
  const cy = ndcY * halfTan;
  const dir = {
    x: right.x * cx + up.x * cy + forward.x,
    y: right.y * cx + up.y * cy + forward.y,
    z: right.z * cx + up.z * cy + forward.z,
  };
  if (Math.abs(dir.y) < 1e-6) {
    return null;
  }
  const t = -camera.position.y / dir.y;
  if (t <= 0) {
    return null;
  }
  return { x: camera.position.x + t * dir.x, y: 0, z: camera.position.z + t * dir.z };
};

/** One selectable target: a world anchor and its clickable screen radius. */
export interface PickTarget {
  readonly index: number;
  readonly at: EngineVec3;
  readonly radiusPx: number;
}

/** The index of the nearest target within its radius of the pointer, or null. */
export const pickAt = (camera: Camera3D, targets: readonly PickTarget[], pointer: PointerSample | undefined): number | null => {
  if (pointer === undefined) {
    return null;
  }
  let best: number | null = null;
  let bestDist = Number.POSITIVE_INFINITY;
  for (const target of targets) {
    const screen = worldToCanvas(camera, target.at);
    if (screen === null) {
      continue;
    }
    const dx = screen.x - pointer.pos.x;
    const dy = screen.y - pointer.pos.y;
    const dist = Math.sqrt(dx * dx + dy * dy);
    if (dist <= target.radiusPx && dist < bestDist) {
      best = target.index;
      bestDist = dist;
    }
  }
  return best;
};
