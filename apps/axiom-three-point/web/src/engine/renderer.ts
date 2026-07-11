/*
 * engine/renderer.ts — the retained-scene store behind the contract in
 * `api.ts`, a module-level singleton (`initRenderer` once, then the free
 * functions the game's `scene.ts` calls). The store owns everything as plain
 * data — meshes (`createMesh` caches one geometry per primitive kind,
 * `createMeshData` takes custom triangle lists), Lambert materials, nodes,
 * directional/point lights, and the look-at camera — and delegates the actual
 * drawing to one of two backends (`backend.ts`):
 *
 *   - WebGL2 (`backend-webgl2.ts`) — the default hardware path.
 *   - Canvas2D (`backend-canvas2d.ts`) — the software fallback, auto-selected
 *     when WebGL2 is unavailable, or forced with `initRenderer(canvas,
 *     "canvas2d")` (the harness wires that to `?backend=canvas2d`).
 *
 * Primitive meshes are built at the backend's detail level (the software
 * rasterizer gets lower-poly spheres/cylinders); custom `MeshData` uploads
 * unchanged on both.
 */

import type { Camera3D, Entity, Handle, Light, MaterialSpec, MeshData, MeshKind, Transform } from "./api.ts";
import {
  type FrameDirLight,
  type FrameNode,
  type FramePointLight,
  type RenderBackend,
  type ResolvedMaterial,
  MAX_DIR_LIGHTS,
  MAX_POINT_LIGHTS,
} from "./backend.ts";
import { createCanvas2dBackend } from "./backend-canvas2d.ts";
import { createWebGl2Backend } from "./backend-webgl2.ts";
import { unitBox, unitCylinderY, unitSphere } from "./meshes.ts";

/** Which drawing backend to use; "auto" tries WebGL2 and falls back to Canvas2D. */
export type BackendChoice = "auto" | "webgl2" | "canvas2d";

interface RendererState {
  readonly canvas: HTMLCanvasElement;
  readonly backend: RenderBackend;
  readonly meshKindCache: Map<MeshKind, Handle>;
  readonly materials: Map<Handle, ResolvedMaterial>;
  readonly nodes: Map<Entity, FrameNode>;
  readonly dirLights: FrameDirLight[];
  readonly pointLights: FramePointLight[];
  camera: Camera3D;
}

let state: RendererState | null = null;
let nextEntity: Entity = 1;
let nextHandle: Handle = 1;

const requireState = (): RendererState => {
  if (state === null) {
    throw new Error("renderer: initRenderer(canvas) must be called before any other renderer function");
  }
  return state;
};

/** Initialize the singleton renderer on `canvas`. `choice` defaults to "auto":
 * WebGL2 when the context is available, otherwise the Canvas2D software
 * fallback. Logs the selected backend once. */
export const initRenderer = (canvas: HTMLCanvasElement, choice: BackendChoice = "auto"): void => {
  let backend: RenderBackend | null = null;
  if (choice !== "canvas2d") {
    backend = createWebGl2Backend(canvas);
    if (backend === null && choice === "webgl2") {
      throw new Error("renderer: WebGL2 was forced but is not available in this browser/canvas");
    }
  }
  if (backend === null) {
    backend = createCanvas2dBackend(canvas);
  }
  console.log(`three-point: render backend = ${backend.name}`);
  state = {
    backend,
    camera: {
      position: { x: 0, y: 2, z: 6 },
      target: { x: 0, y: 0, z: 0 },
      fovY: Math.PI / 3,
      near: 0.1,
      far: 200,
    },
    canvas,
    dirLights: [],
    materials: new Map(),
    meshKindCache: new Map(),
    nodes: new Map(),
    pointLights: [],
  };
};

/** The active backend's name (for HUD/debug readouts). */
export const rendererBackendName = (): string => requireState().backend.name;

/** Total retained scene nodes (development counter). */
export const rendererNodeCount = (): number => requireState().nodes.size;

/** Resize the canvas backing store and the backend's viewport. */
export const resizeRenderer = (width: number, height: number): void => {
  const st = requireState();
  st.canvas.width = Math.max(1, Math.floor(width));
  st.canvas.height = Math.max(1, Math.floor(height));
  st.backend.resize(st.canvas.width, st.canvas.height);
};

/** Register custom triangle-list geometry and return its handle. */
export const createMeshData = (data: MeshData): Handle => {
  const st = requireState();
  if (data.positions.length !== data.normals.length) {
    throw new Error(
      `renderer: createMeshData positions (${data.positions.length}) and normals (${data.normals.length}) differ`,
    );
  }
  const handle = nextHandle;
  nextHandle += 1;
  st.backend.uploadMesh(handle, data);
  return handle;
};

/** Primitive builders per backend detail level: the software rasterizer pays
 * per triangle, so it gets gentler tessellation. */
const KIND_BUILDERS: Record<"high" | "low", Record<MeshKind, () => MeshData>> = {
  high: {
    box: unitBox,
    cylinder: () => unitCylinderY(),
    sphere: () => unitSphere(),
  },
  low: {
    box: unitBox,
    cylinder: () => unitCylinderY(12),
    sphere: () => unitSphere(8, 12),
  },
};

/** Get (or lazily build + cache) the shared geometry for a primitive kind. */
export const createMesh = (kind: MeshKind): Handle => {
  const st = requireState();
  const cached = st.meshKindCache.get(kind);
  if (cached !== undefined) {
    return cached;
  }
  const handle = createMeshData(KIND_BUILDERS[st.backend.meshDetail][kind]());
  st.meshKindCache.set(kind, handle);
  return handle;
};

/** Register a Lambert material (diffuse base + additive emissive + opacity). */
export const createMaterial = (spec: MaterialSpec): Handle => {
  const st = requireState();
  const emissive = spec.emissive ?? [0, 0, 0, 1];
  const handle = nextHandle;
  nextHandle += 1;
  st.materials.set(handle, {
    baseColor: [spec.baseColor[0], spec.baseColor[1], spec.baseColor[2], spec.baseColor[3]],
    emissive: [emissive[0], emissive[1], emissive[2]],
    opacity: spec.opacity ?? 1,
  });
  return handle;
};

/** Add a scene node drawing `mesh` with `material` at `transform`. */
export const spawnRenderable = (mesh: Handle, material: Handle, transform: Transform): Entity => {
  const st = requireState();
  if (!st.materials.has(material)) {
    throw new Error(`renderer: spawnRenderable got unknown material handle ${material}`);
  }
  const entity = nextEntity;
  nextEntity += 1;
  st.nodes.set(entity, { material, mesh, transform });
  return entity;
};

/** Re-pose an existing node. */
export const setNodeTransform = (entity: Entity, t: Transform): void => {
  const node = requireState().nodes.get(entity);
  if (node === undefined) {
    throw new Error(`renderer: setNodeTransform got unknown entity ${entity}`);
  }
  node.transform = t;
};

/** Set the look-at perspective camera used by the next `renderScene`. */
export const setCamera3D = (cam: Camera3D): void => {
  requireState().camera = cam;
};

/** Add a directional or point light. Lights beyond the backends' capacity
 * (8 directional + 8 point) are accepted but do not contribute. */
export const addLight = (light: Light): Entity => {
  const st = requireState();
  const entity = nextEntity;
  nextEntity += 1;
  const color: readonly [number, number, number] = [
    light.color[0] * light.intensity,
    light.color[1] * light.intensity,
    light.color[2] * light.intensity,
  ];
  if (light.kind === "directional") {
    const d = light.direction;
    const len = Math.sqrt(d.x * d.x + d.y * d.y + d.z * d.z);
    const inv = len < 1e-9 ? 0 : 1 / len;
    if (st.dirLights.length < MAX_DIR_LIGHTS) {
      st.dirLights.push({ color, direction: [d.x * inv, len < 1e-9 ? -1 : d.y * inv, d.z * inv] });
    }
  } else if (st.pointLights.length < MAX_POINT_LIGHTS) {
    st.pointLights.push({ color, position: [light.position.x, light.position.y, light.position.z] });
  }
  return entity;
};

/** Drop every node, light, mesh, and material (backend resources included). */
export const clearScene = (): void => {
  const st = requireState();
  st.backend.dropMeshes();
  st.meshKindCache.clear();
  st.materials.clear();
  st.nodes.clear();
  st.dirLights.length = 0;
  st.pointLights.length = 0;
};

/** Clear and draw the retained scene through the active backend. */
export const renderScene = (): void => {
  const st = requireState();
  st.backend.render({
    camera: st.camera,
    dirLights: st.dirLights,
    materials: st.materials,
    nodes: st.nodes.values(),
    pointLights: st.pointLights,
  });
};
