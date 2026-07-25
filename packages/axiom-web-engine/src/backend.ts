/*
 * backend.ts — the INTERNAL contract between the retained-scene store
 * (`renderer.ts`) and the two drawing backends: `backend-webgl2.ts` (the
 * default, hardware path) and `backend-canvas2d.ts` (the software fallback,
 * auto-selected when WebGL2 is unavailable or forced with `?backend=canvas2d`).
 * The store owns meshes/materials/nodes/lights/camera as plain data; a backend
 * only knows how to ingest mesh geometry and draw one frame of it.
 */

import type { Camera3D, Handle, MeshData, Transform } from "./api.ts";

/** A material resolved to plain arrays (defaults applied by the store).
 * `roughness ∈ [0, 1]` drives the specular highlight (1 = matte, no specular). */
export interface ResolvedMaterial {
  readonly baseColor: readonly [number, number, number, number];
  readonly emissive: readonly [number, number, number];
  readonly opacity: number;
  readonly roughness: number;
}

/** One drawable node (the store mutates `transform` in place on re-pose). */
export interface FrameNode {
  readonly mesh: Handle;
  readonly material: Handle;
  transform: Transform;
}

export interface FrameDirLight {
  /** Normalized travel direction of the light. */
  readonly direction: readonly [number, number, number];
  /** color · intensity. */
  readonly color: readonly [number, number, number];
}

export interface FramePointLight {
  readonly position: readonly [number, number, number];
  /** color · intensity. */
  readonly color: readonly [number, number, number];
}

/** Everything a backend needs to draw one frame. */
export interface SceneFrame {
  readonly nodes: Iterable<FrameNode>;
  readonly materials: ReadonlyMap<Handle, ResolvedMaterial>;
  /** The AMBIENT (sky/bounce) term, linear RGB — the omni-directional diffuse
   * floor every surface receives regardless of which way it faces, before the
   * albedo multiply. It is per-frame SCENE DATA, not an engine constant, because
   * ambient is a property of the environment being lit: a night interior wants
   * near-zero, a bright beach wants a strong WARM sand-bounce that keeps
   * away-facing surfaces in the family of the surfaces around them.
   *
   * Before this field existed the floor was one hard-coded monochrome scalar
   * (`AMBIENT`), and a scene that needed more had only two illegal moves: burn a
   * directional-light slot on a near-white "fill" (which still leaves faces
   * pointing away from BOTH lights crushed, because a directional fill is not
   * omni-directional), or add fake `emissive` to the materials — paint
   * compensating for light. Both are app-tier workarounds for a missing
   * render-contract field; this is the field.
   *
   * The store defaults it to `DEFAULT_AMBIENT`, so a scene that never sets it
   * renders byte-identically to before. */
  readonly ambient: readonly [number, number, number];
  readonly dirLights: readonly FrameDirLight[];
  readonly pointLights: readonly FramePointLight[];
  readonly camera: Camera3D;
  /** Background clear color (RGB, 0..1). Defaults to `CLEAR_COLOR`; the store
   * overrides it via `setClearColor`. Both backends read it per frame so a game
   * can paint its own sky/void instead of the near-black default. */
  readonly clearColor: readonly [number, number, number];
}

/** The drawing backend the store delegates to. */
export interface RenderBackend {
  readonly name: "WebGL2" | "Canvas2D";
  /** Softer geometry suits the software rasterizer: the store builds primitive
   * meshes at this detail level. */
  readonly meshDetail: "high" | "low";
  /** Ingest triangle-list geometry under the store's handle. */
  readonly uploadMesh: (handle: Handle, data: MeshData) => void;
  /** Forget every uploaded mesh (the store is clearing the scene). */
  readonly dropMeshes: () => void;
  /** The canvas backing store was resized. */
  readonly resize: (width: number, height: number) => void;
  /** Clear and draw the whole frame. */
  readonly render: (frame: SceneFrame) => void;
}

/** The DEFAULT ambient floor — the historical hard-coded value, kept as the
 * store's default so an unset `SceneFrame.ambient` reproduces the previous
 * render exactly. Scenes that want a different environment set their own. */
export const AMBIENT = 0.12;
/** `AMBIENT` as the neutral grey triple the store seeds `SceneFrame.ambient` with. */
export const DEFAULT_AMBIENT: readonly [number, number, number] = [AMBIENT, AMBIENT, AMBIENT];
/** The default near-black void, as 8-bit RGB channels normalized to 0..1. */
const BYTE_MAX = 255;
const VOID_R = 5;
const VOID_G = 6;
const VOID_B = 10;
export const CLEAR_COLOR: readonly [number, number, number] = [VOID_R / BYTE_MAX, VOID_G / BYTE_MAX, VOID_B / BYTE_MAX];
export const MAX_DIR_LIGHTS = 8;
export const MAX_POINT_LIGHTS = 8;
