/*
 * backend.ts — the INTERNAL contract between the retained-scene store
 * (`renderer.ts`) and the two drawing backends: `backend-webgl2.ts` (the
 * default, hardware path) and `backend-canvas2d.ts` (the software fallback,
 * auto-selected when WebGL2 is unavailable or forced with `?backend=canvas2d`).
 * The store owns meshes/materials/nodes/lights/camera as plain data; a backend
 * only knows how to ingest mesh geometry and draw one frame of it.
 */

import type { Camera3D, Handle, MeshData, Transform } from "./api.ts";

/** A material resolved to plain arrays (defaults applied by the store). */
export interface ResolvedMaterial {
  readonly baseColor: readonly [number, number, number, number];
  readonly emissive: readonly [number, number, number];
  readonly opacity: number;
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

/** Ambient floor and clear color, shared by both backends so they match. */
export const AMBIENT = 0.12;
/** The default near-black void, as 8-bit RGB channels normalized to 0..1. */
const BYTE_MAX = 255;
const VOID_R = 5;
const VOID_G = 6;
const VOID_B = 10;
export const CLEAR_COLOR: readonly [number, number, number] = [VOID_R / BYTE_MAX, VOID_G / BYTE_MAX, VOID_B / BYTE_MAX];
export const MAX_DIR_LIGHTS = 8;
export const MAX_POINT_LIGHTS = 8;
