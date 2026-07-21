/*
 * compose.ts — flattens a figure's part hierarchy to WORLD transforms on the CPU,
 * because the engine scene is flat (no parenting) and does not export its matrix
 * math. It walks a parent-before-child part list once, accumulating each part's
 * world frame as a RIGID + UNIFORM-scale transform (the engine transform is a
 * single TRS with no shear, so a parent's non-uniform shape can never legally
 * propagate through a child's rotation). Each part's non-uniform `extents` are
 * applied ONLY at its own leaf node. An optional per-part `PoseDelta` (from the
 * animation controller) layers on top of the rest pose. Pure and SDK-free.
 */

import { type Quat, type Vec3, IDENTITY_QUAT, add, quatFromEulerXyz, quatMul, rotateVec, scale, vec3 } from "./vec3.ts";
import type { RestTransform } from "./grammar.ts";

/** Structurally the engine's `Transform` — handed straight to `setNodeTransform`. */
export interface WorldTransform {
  readonly position: Vec3;
  readonly rotation: Quat;
  readonly scale: Vec3;
}

/** One composable part: everything `composeWorld` needs, resolved from the grammar. */
export interface ComposePart {
  /** Index of the parent in this same array, or -1 for a root (uses `rootFrame`). */
  readonly parentIndex: number;
  readonly rest: RestTransform;
  readonly extents: Vec3;
  readonly offset: Vec3;
}

/** A per-part animation offset layered onto its rest pose. */
export interface PoseDelta {
  readonly rot?: Quat;
  readonly pos?: Vec3;
  readonly scale?: number;
}

/** The world placement of a whole figure (slot position, facing, overall scale). */
export interface RootFrame {
  readonly position: Vec3;
  readonly rotation: Quat;
  readonly scale: number;
}

interface Frame {
  pos: Vec3;
  rot: Quat;
  s: number;
}

const restQuat = (r: RestTransform): Quat => quatFromEulerXyz(r.rotationEuler.x, r.rotationEuler.y, r.rotationEuler.z);

/**
 * Compose world transforms for every part. `frames` and `out` are caller-owned
 * scratch arrays of length `parts.length` (reused across ticks to avoid GC).
 */
export const composeWorld = (
  parts: readonly ComposePart[],
  rootFrame: RootFrame,
  poses: readonly (PoseDelta | undefined)[],
  frames: Frame[],
  out: WorldTransform[],
): void => {
  for (let i = 0; i < parts.length; i += 1) {
    const part = parts[i] as ComposePart;
    const pose = poses[i];
    const localRot = pose?.rot ? quatMul(restQuat(part.rest), pose.rot) : restQuat(part.rest);
    const localPos = pose?.pos ? add(part.rest.position, pose.pos) : part.rest.position;
    const localScale = part.rest.scale * (pose?.scale ?? 1);

    const parent: Frame =
      part.parentIndex < 0
        ? { pos: rootFrame.position, rot: rootFrame.rotation, s: rootFrame.scale }
        : (frames[part.parentIndex] as Frame);

    const worldRot = quatMul(parent.rot, localRot);
    const worldScale = parent.s * localScale;
    const worldPos = add(parent.pos, rotateVec(parent.rot, scale(localPos, parent.s)));
    const frame = frames[i] as Frame;
    frame.pos = worldPos;
    frame.rot = worldRot;
    frame.s = worldScale;

    // Leaf transform: extents scaled by the frame's uniform scale; offset placed
    // in the frame's rotated space.
    const o = out[i] as { position: Vec3; rotation: Quat; scale: Vec3 };
    o.position = add(worldPos, rotateVec(worldRot, scale(part.offset, worldScale)));
    o.rotation = worldRot;
    o.scale = vec3(part.extents.x * worldScale, part.extents.y * worldScale, part.extents.z * worldScale);
  }
};

/** Allocate the caller-owned scratch buffers for `composeWorld`. */
export const composeBuffers = (n: number): { frames: Frame[]; out: WorldTransform[] } => ({
  frames: Array.from({ length: n }, () => ({ pos: vec3(0, 0, 0), rot: IDENTITY_QUAT, s: 1 })),
  out: Array.from({ length: n }, () => ({ position: vec3(0, 0, 0), rotation: IDENTITY_QUAT, scale: vec3(1, 1, 1) })),
});
