/*
 * The 3D scene authoring surface (SPEC-11 §4.2): `createMesh` / `createMaterial` /
 * `setCamera3D` / `addLight`. Each marshals the contract's flat record to a neutral
 * `HostBridge` descriptor (`host-descriptors.ts`) and the bridge forwards it to the
 * existing native scene/material/mesh facades, returning an opaque handle (or, for
 * a light, its `Entity`). This is projection only — the native 3D core (perspective
 * camera, primitive meshes, lit/shadowed rasterizer) already exists; nothing here
 * re-implements geometry or shading.
 *
 * The string discriminants the contract names (`"box"|"sphere"|"cylinder"`,
 * `"directional"|"point"`) are resolved to the native dense table index by
 * `indexOf` — a table select, not a `switch`. A light variant's vector field is
 * named `direction` or `position` per the contract; it is read branchlessly by
 * widening the light to a `{ direction?, position? }` view (a sound assignment, not
 * an unsafe cast) and `orElse`-defaulting to whichever channel is present.
 */

import type { CameraDescriptor, ControllerSpec, LightDescriptor, MaterialDescriptor, MeshDataDescriptor } from "./host-descriptors.ts";
import type { Entity, Handle, Rgba, Transform, Vec2, Vec3 } from "./vocabulary.ts";
import { boundHost } from "./host-binding.ts";
import { orElse } from "./control-flow.ts";

/** A primitive mesh kind (SPEC-11 §4.2). `"box"` is the engine `Cube`. */
export type MeshKind = "box" | "cylinder" | "sphere";

/** The mesh kinds in their dense native table order (box=0, sphere=1, cylinder=2). */
const MESH_KINDS: readonly MeshKind[] = ["box", "sphere", "cylinder"];

/** Create a primitive mesh, returning its opaque handle (SPEC-11 §4.2). */
export const createMesh = (kind: MeshKind): Handle => boundHost().createMesh(MESH_KINDS.indexOf(kind));

/**
 * Author-supplied mesh geometry (SPEC-11 §4.2 / §11) — the non-catalog
 * counterpart to a primitive [`MeshKind`]: one `positions` and one `normals`
 * vector per vertex, an optional `uvs` (omit to default each vertex to the
 * origin), and a triangle-list `indices` into the vertices.
 */
export interface MeshData {
  /** The per-vertex positions. */
  readonly positions: readonly Vec3[];
  /** The per-vertex normals (one per position). */
  readonly normals: readonly Vec3[];
  /** The optional per-vertex UVs (omitted ⇒ origin per vertex). */
  readonly uvs?: readonly Vec2[];
  /** The triangle-list indices into the vertices. */
  readonly indices: readonly number[];
}

/** The default UVs when an author omits them — the engine fills the origin per vertex. */
const NO_UVS: readonly Vec2[] = [];

/**
 * Create a mesh from author-supplied vertex data, returning its opaque handle
 * (SPEC-11 §11) — the same handle a primitive [`createMesh`] returns, so it
 * spawns identically. A distinct function rather than a `createMesh` overload:
 * discriminating a kind string from a data record at runtime would need a
 * `typeof` branch, which the Branchless Law forbids; two named entry points keep
 * each path branchless and fully typed. Malformed geometry resolves to the null
 * handle engine-side.
 */
export const createMeshData = (data: MeshData): Handle => {
  const descriptor: MeshDataDescriptor = {
    indices: data.indices,
    normals: data.normals,
    positions: data.positions,
    uvs: orElse(data.uvs, NO_UVS),
  };
  return boundHost().createMeshData(descriptor);
};

/** A lit-material description (SPEC-11 §4.2); emissive/roughness/opacity default engine-side. */
export interface MaterialSpec {
  /** The diffuse base colour. */
  readonly baseColor: Rgba;
  /** Self-illumination colour (default: none). */
  readonly emissive?: Rgba;
  /** Surface roughness, `0` mirror-smooth … `1` matte (default: matte). */
  readonly roughness?: number;
  /** Opacity, `1` opaque (default: opaque; blends only after SPEC-04). */
  readonly opacity?: number;
}

/** The default emissive colour — no self-illumination. */
const NO_EMISSIVE: Rgba = [0, 0, 0, 0];
/** The default roughness — fully matte (SPEC-11 §4.2 `1 = matte`). */
const MATTE = 1;
/** The default opacity — fully opaque. */
const OPAQUE = 1;

/** Create a lit material from its spec, returning its opaque handle (SPEC-11 §4.2). */
export const createMaterial = (spec: MaterialSpec): Handle => {
  const material: MaterialDescriptor = {
    baseColor: spec.baseColor,
    emissive: orElse(spec.emissive, NO_EMISSIVE),
    opacity: orElse(spec.opacity, OPAQUE),
    roughness: orElse(spec.roughness, MATTE),
  };
  return boundHost().createMaterial(material);
};

/** A perspective camera placement (SPEC-11 §4.2). */
export interface Camera3D {
  /** The eye position. */
  readonly position: Vec3;
  /** The point the camera looks at. */
  readonly target: Vec3;
  /** The vertical field of view in radians. */
  readonly fovY: number;
  /** The near clip plane distance. */
  readonly near: number;
  /** The far clip plane distance. */
  readonly far: number;
}

/** Build the scene's perspective camera (look-at + intrinsics) from the flat record (SPEC-11 §4.2). */
export const setCamera3D = (camera: Camera3D): void => {
  const descriptor: CameraDescriptor = {
    far: camera.far,
    fovY: camera.fovY,
    near: camera.near,
    position: camera.position,
    target: camera.target,
  };
  boundHost().setCamera3D(descriptor);
};

/** A scene light — directional (sun) or point (SPEC-11 §4.2). */
export type Light =
  | { readonly kind: "directional"; readonly direction: Vec3; readonly color: Rgba; readonly intensity: number }
  | { readonly kind: "point"; readonly position: Vec3; readonly color: Rgba; readonly intensity: number };

/** The light kinds in their dense native table order (directional=0, point=1). */
const LIGHT_KINDS: readonly Light["kind"][] = ["directional", "point"];

/** The fallback vector when a light somehow carries neither channel (never reached for a valid `Light`). */
const ORIGIN: Vec3 = { x: 0, y: 0, z: 0 };

/** Add a light to the scene, returning its `Entity` (SPEC-11 §4.2). */
export const addLight = (light: Light): Entity => {
  const channels: { readonly direction?: Vec3; readonly position?: Vec3 } = light;
  const descriptor: LightDescriptor = {
    color: light.color,
    intensity: light.intensity,
    kind: LIGHT_KINDS.indexOf(light.kind),
    vector: orElse(channels.direction, orElse(channels.position, ORIGIN)),
  };
  return boundHost().addLight(descriptor);
};

/*
 * Spawn a renderable node from a `(mesh, material)` handle pair at `transform`,
 * returning its `Entity` (SPEC-11). The handles are the ones [`createMesh`] /
 * [`createMaterial`] returned; the node draws every frame and can be moved with
 * [`setNodeTransform`] or made solid to queries with [`setNodeBounds`]. This is
 * the verb that actually *places* geometry in the world — `createMesh` only
 * registers the shape.
 */
export const spawnRenderable = (mesh: Handle, material: Handle, transform: Transform): Entity =>
  boundHost().spawnRenderable(mesh, material, transform);

/*
 * Overwrite a node's transform (SPEC-11) — the per-frame move / rotate / scale a
 * game applies to a renderable (an enemy walking, a platform sliding). The write
 * is committed immediately, so a spatial query or the present render this frame
 * sees the node at its new pose.
 */
export const setNodeTransform = (entity: Entity, transform: Transform): void => {
  boundHost().setNodeTransform(entity, transform);
};

/*
 * Set a node's collision bounds to an axis-aligned box of `halfExtents` (SPEC-11),
 * so it answers `overlapBox` / `raycast` — how a level's walls become solid and an
 * enemy becomes a hitscan target.
 */
export const setNodeBounds = (entity: Entity, halfExtents: Vec3): void => {
  boundHost().setNodeBounds(entity, halfExtents);
};

/*
 * Clear the whole 3D scene (SPEC-11), leaving a blank scene to author from. A 3D
 * game calls this once at startup before building its own scene, so the runtime's
 * default content does not bleed through; afterwards [`createMesh`] /
 * [`createMaterial`] mint fresh 1-based handles.
 */
export const clearScene = (): void => {
  boundHost().clearScene();
};

/** The default controller index — most games drive a single first-person camera. */
const ROOT_CONTROLLER = 0;

/** One frame's first-person input for a controller (SPEC-11): a local-frame move plus yaw/pitch deltas. */
export interface FirstPersonControl {
  /** The controller this input drives (default: the root controller). */
  readonly index?: number;
  /** Translation in the camera's own frame: `-Z` forward, `+X` right. */
  readonly moveLocal: Vec3;
  /** Yaw delta about world `+Y`, in radians. */
  readonly yawDelta: number;
  /** Pitch delta about local `+X`, in radians (the engine clamps it). */
  readonly pitchDelta: number;
}

/*
 * Spawn the active camera as a first-person **controller** (SPEC-11) and return
 * its `Entity`. The engine then owns the camera node: [`controlFirstPerson`] yaws,
 * pitches, and moves it each frame, so a game never re-authors the camera
 * transform — it just hands the engine a per-frame intent. This is the
 * engine-driven counterpart to setting the camera with [`setCamera3D`] every tick.
 */
export const createController = (spec: ControllerSpec, index?: number): Entity =>
  boundHost().createController(spec, orElse(index, ROOT_CONTROLLER));

/*
 * Apply one frame's first-person input to a controller (SPEC-11), **immediately**
 * — the engine yaws/pitches (clamped) and moves the camera node now, with no
 * one-frame lag and no camera re-authoring. The move is in the camera's own frame
 * (`-Z` forward); a game collision-resolves it in world space and rotates it into
 * the local frame before handing it over (the engine then re-applies only the yaw).
 */
export const controlFirstPerson = (control: FirstPersonControl): void => {
  boundHost().controlFirstPerson({
    index: orElse(control.index, ROOT_CONTROLLER),
    moveLocal: control.moveLocal,
    pitchDelta: control.pitchDelta,
    yawDelta: control.yawDelta,
  });
};
