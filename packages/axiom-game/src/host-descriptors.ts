/*
 * The neutral parameter records the `HostBridge`'s grid / 3D / math methods
 * traffic in (SPEC-06 / SPEC-11), plus the inert default values those reads return
 * before a host binds. They live here, beside `host-binding.ts`, so each bridge
 * method stays within the SDK's ≤3-parameter law by taking ONE descriptor record
 * instead of a long flat argument list (the same record-argument shape
 * `createMaterial`/`setCamera3D` use). Pure value shapes — plain numbers and
 * tuples that marshal 1:1 across the wasm boundary.
 */

import type { Cell, Mat4, Quat, Rgba, Vec2, Vec3 } from "./vocabulary.ts";

/** A grid plus its row-major passability mask — the field a path query runs over (SPEC-06). */
export interface GridField {
  /** The column count. */
  readonly cols: number;
  /** The row count. */
  readonly rows: number;
  /** The row-major passable-cell mask the projection built from the author's predicate. */
  readonly passable: readonly boolean[];
}

/** A resolved lit-material description (SPEC-11) — optional fields already defaulted. */
export interface MaterialDescriptor {
  /** The diffuse base colour. */
  readonly baseColor: Rgba;
  /** The self-illumination colour. */
  readonly emissive: Rgba;
  /** The surface roughness (`0` smooth … `1` matte). */
  readonly roughness: number;
  /** The opacity (`1` opaque). */
  readonly opacity: number;
}

/** Resolved author mesh geometry (SPEC-11 §11) — optional UVs already defaulted to `[]`. */
export interface MeshDataDescriptor {
  /** The per-vertex positions. */
  readonly positions: readonly Vec3[];
  /** The per-vertex normals (one per position). */
  readonly normals: readonly Vec3[];
  /** The per-vertex UVs (`[]` ⇒ the engine defaults each vertex to the origin). */
  readonly uvs: readonly Vec2[];
  /** The triangle-list indices into the vertices. */
  readonly indices: readonly number[];
}

/** A perspective camera placement (SPEC-11) — look-at endpoints plus intrinsics. */
export interface CameraDescriptor {
  /** The eye position. */
  readonly position: Vec3;
  /** The look-at target. */
  readonly target: Vec3;
  /** The vertical field of view (radians). */
  readonly fovY: number;
  /** The near clip distance. */
  readonly near: number;
  /** The far clip distance. */
  readonly far: number;
}

/** A first-person controller-camera placement (SPEC-11) — like a camera but with no look-at target; its orientation is driven by per-frame controller input. */
export interface ControllerSpec {
  /** The eye position. */
  readonly position: Vec3;
  /** The vertical field of view (radians). */
  readonly fovY: number;
  /** The near clip distance. */
  readonly near: number;
  /** The far clip distance. */
  readonly far: number;
}

/** One frame's resolved first-person controller input (SPEC-11): a local-frame move plus yaw/pitch deltas, addressed to a controller index. */
export interface ControllerInput {
  /** The controller index this input drives. */
  readonly index: number;
  /** Translation in the camera's own frame: `-Z` forward, `+X` right. */
  readonly moveLocal: Vec3;
  /** Yaw delta about world `+Y` (radians). */
  readonly yawDelta: number;
  /** Pitch delta about local `+X` (radians; the engine clamps it). */
  readonly pitchDelta: number;
}

/** A scene light (SPEC-11): dense kind index plus its direction/position vector and colour. */
export interface LightDescriptor {
  /** The dense light-kind index (0=directional, 1=point). */
  readonly kind: number;
  /** The direction (directional) or position (point). */
  readonly vector: Vec3;
  /** The light colour. */
  readonly color: Rgba;
  /** The light intensity. */
  readonly intensity: number;
}

/** A perspective-projection specification (SPEC-11) — for `mat4Perspective`. */
export interface PerspectiveSpec {
  /** The vertical field of view (radians). */
  readonly fovY: number;
  /** The aspect ratio (width / height). */
  readonly aspect: number;
  /** The near clip distance. */
  readonly near: number;
  /** The far clip distance. */
  readonly far: number;
}

/** The zero vector an inert `v2` read returns before a host binds. */
export const ZERO_VEC2: Vec2 = { x: 0, y: 0 };

/** The zero vector an inert `v3` read returns before a host binds. */
export const ZERO_VEC3: Vec3 = { x: 0, y: 0, z: 0 };

/** The 4×4 identity an inert `mat4` read returns before a host binds. */
export const IDENTITY_MAT4: Mat4 = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

/** The identity quaternion an inert `quat` read returns before a host binds. */
export const IDENTITY_QUAT: Quat = [0, 0, 0, 1];

/** The origin cell an inert grid read returns before a host binds. */
export const ORIGIN_CELL: Cell = { x: 0, y: 0 };
