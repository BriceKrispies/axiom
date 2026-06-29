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

import type { CameraDescriptor, LightDescriptor, MaterialDescriptor } from "./host-descriptors.ts";
import type { Entity, Handle, Rgba, Vec3 } from "./vocabulary.ts";
import { boundHost } from "./host-binding.ts";
import { orElse } from "./branchless.ts";

/** A primitive mesh kind (SPEC-11 §4.2). `"box"` is the engine `Cube`. */
export type MeshKind = "box" | "cylinder" | "sphere";

/** The mesh kinds in their dense native table order (box=0, sphere=1, cylinder=2). */
const MESH_KINDS: readonly MeshKind[] = ["box", "sphere", "cylinder"];

/** Create a primitive mesh, returning its opaque handle (SPEC-11 §4.2). */
export const createMesh = (kind: MeshKind): Handle => boundHost().createMesh(MESH_KINDS.indexOf(kind));

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
