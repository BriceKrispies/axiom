/*
 * Store: the retained-scene store behind the `api.ts` contract — a module-level
 * singleton (`initStore` once with an INJECTED backend, then the free scene
 * functions). It owns meshes/materials/nodes/lights/camera as plain data and
 * delegates drawing to a `backend.ts` backend. This is the branchless, fully-
 * covered spine half of the old `renderer.ts`: it builds no WebGL/Canvas context
 * itself, so every path is exercisable with a fake backend. The thin `renderer.ts`
 * edge resolves a real backend and hands it here.
 */

import {
  CLEAR_COLOR,
  type FrameDirLight,
  type FrameNode,
  type FramePointLight,
  MAX_DIR_LIGHTS,
  MAX_POINT_LIGHTS,
  type RenderBackend,
  type ResolvedMaterial,
} from "./backend.ts";
import type { Camera3D, Entity, Handle, Light, MaterialSpec, MeshData, MeshKind, Rgba, Transform } from "./api.ts";
import { absentProbe, assert, demand, orCompute, orElse, select } from "./branchless.ts";
import { unitBox, unitCylinderY, unitSphere } from "./meshes.ts";

/** Color · intensity, resolved to a plain RGB triple for the frame. */
type Rgb = readonly [number, number, number];

/** The canvas backing store the renderer resizes (a real `HTMLCanvasElement`
 * structurally satisfies this; a fake `{ width, height }` does too). */
interface EngineCanvas {
  width: number;
  height: number;
}

interface RendererState {
  readonly canvas: EngineCanvas;
  readonly backend: RenderBackend;
  readonly meshKindCache: Map<MeshKind, Handle>;
  readonly materials: Map<Handle, ResolvedMaterial>;
  readonly nodes: Map<Entity, FrameNode>;
  /** Lights are retained as their authored specs (keyed by entity, so they can
   * be re-posed via `setLight`) and resolved to frame lights at render time. */
  readonly lights: Map<Entity, Light>;
  camera: Camera3D;
  clearColor: [number, number, number];
}

const DEFAULT_CAMERA_HEIGHT = 2;
const DEFAULT_CAMERA_DISTANCE = 6;
const DEFAULT_FOV_DIVISOR = 3;
const DEFAULT_NEAR_PLANE = 0.1;
const DEFAULT_FAR_PLANE = 200;
// A zero-magnitude direction is treated as straight down.
const DIRECTION_EPSILON = 1e-9;
const DEFAULT_EMISSIVE: Rgba = [0, 0, 0, 1];
const DEFAULT_OPACITY = 1;
// The software rasterizer pays per triangle, so its primitives are lower-poly.
const LOW_CYLINDER_SEGMENTS = 12;
const LOW_SPHERE_LAT_SEGMENTS = 8;
const LOW_SPHERE_LON_SEGMENTS = 12;

const DEFAULT_CAMERA: Camera3D = {
  far: DEFAULT_FAR_PLANE,
  fovY: Math.PI / DEFAULT_FOV_DIVISOR,
  near: DEFAULT_NEAR_PLANE,
  position: { x: 0, y: DEFAULT_CAMERA_HEIGHT, z: DEFAULT_CAMERA_DISTANCE },
  target: { x: 0, y: 0, z: 0 },
};

let state = absentProbe<RendererState>();
let nextEntity: Entity = 1;
let nextHandle: Handle = 1;

const requireState = (): RendererState =>
  demand(state, "store: initStore(backend, canvas) must be called before any other store function");

/** Initialize the singleton store with an already-resolved backend and the
 * canvas it draws into. Sets the default camera and clear color. */
export const initStore = (backend: RenderBackend, canvas: EngineCanvas): void => {
  const [cr, cg, cb] = CLEAR_COLOR;
  state = {
    backend,
    camera: DEFAULT_CAMERA,
    canvas,
    clearColor: [cr, cg, cb],
    lights: new Map(),
    materials: new Map(),
    meshKindCache: new Map(),
    nodes: new Map(),
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
  assert(
    data.positions.length === data.normals.length,
    `store: createMeshData positions (${data.positions.length}) and normals (${data.normals.length}) differ`,
  );
  const handle = nextHandle;
  nextHandle += 1;
  st.backend.uploadMesh(handle, data);
  return handle;
};

/** Primitive builders per backend detail level. */
const KIND_BUILDERS: Record<"high" | "low", Record<MeshKind, () => MeshData>> = {
  high: {
    box: unitBox,
    cylinder: (): MeshData => unitCylinderY(),
    sphere: (): MeshData => unitSphere(),
  },
  low: {
    box: unitBox,
    cylinder: (): MeshData => unitCylinderY(LOW_CYLINDER_SEGMENTS),
    sphere: (): MeshData => unitSphere(LOW_SPHERE_LAT_SEGMENTS, LOW_SPHERE_LON_SEGMENTS),
  },
};

/** Get (or lazily build + cache) the shared geometry for a primitive kind. */
export const createMesh = (kind: MeshKind): Handle => {
  const st = requireState();
  return orCompute(st.meshKindCache.get(kind), (): Handle => {
    const handle = createMeshData(KIND_BUILDERS[st.backend.meshDetail][kind]());
    st.meshKindCache.set(kind, handle);
    return handle;
  });
};

/** Register a Lambert material (diffuse base + additive emissive + opacity). */
export const createMaterial = (spec: MaterialSpec): Handle => {
  const st = requireState();
  const [br, bg, bb, ba] = spec.baseColor;
  const [er, eg, eb] = orElse(spec.emissive, DEFAULT_EMISSIVE);
  const handle = nextHandle;
  nextHandle += 1;
  st.materials.set(handle, {
    baseColor: [br, bg, bb, ba],
    emissive: [er, eg, eb],
    opacity: orElse(spec.opacity, DEFAULT_OPACITY),
  });
  return handle;
};

/** Add a scene node drawing `mesh` with `material` at `transform`. */
export const spawnRenderable = (mesh: Handle, material: Handle, transform: Transform): Entity => {
  const st = requireState();
  assert(st.materials.has(material), `store: spawnRenderable got unknown material handle ${material}`);
  const entity = nextEntity;
  nextEntity += 1;
  st.nodes.set(entity, { material, mesh, transform });
  return entity;
};

/** Re-pose an existing node. */
export const setNodeTransform = (entity: Entity, transform: Transform): void => {
  const node = demand(requireState().nodes.get(entity), `store: setNodeTransform got unknown entity ${entity}`);
  node.transform = transform;
};

/** Remove a node from the retained scene (its geometry/material handles live on;
 * only this drawable is dropped). The reconciler uses this so an immediate-mode
 * `view` that stops emitting a node makes it disappear. */
export const despawnRenderable = (entity: Entity): void => {
  const st = requireState();
  assert(st.nodes.has(entity), `store: despawnRenderable got unknown entity ${entity}`);
  st.nodes.delete(entity);
};

/** Set the look-at perspective camera used by the next `renderScene`. */
export const setCamera3D = (cam: Camera3D): void => {
  requireState().camera = cam;
};

/** Set the background clear color (the alpha channel is ignored). */
export const setClearColor = (color: Rgba): void => {
  const [cr, cg, cb] = color;
  requireState().clearColor = [cr, cg, cb];
};

const isDirectional = (light: Light): light is Extract<Light, { kind: "directional" }> =>
  light.kind === "directional";
const isPoint = (light: Light): light is Extract<Light, { kind: "point" }> => light.kind === "point";

/** Color · intensity, resolved to the plain RGB triple a frame light carries. */
const litColor = (light: Light): Rgb => {
  const [cr, cg, cb] = light.color;
  return [cr * light.intensity, cg * light.intensity, cb * light.intensity];
};

const resolveDirLight = (light: Extract<Light, { kind: "directional" }>): FrameDirLight => {
  const dir = light.direction;
  const len = Math.hypot(dir.x, dir.y, dir.z);
  const tiny = len < DIRECTION_EPSILON;
  const inv = select(tiny, 0, 1 / len);
  return { color: litColor(light), direction: [dir.x * inv, select(tiny, -1, dir.y * inv), dir.z * inv] };
};

const resolvePointLight = (light: Extract<Light, { kind: "point" }>): FramePointLight => {
  const pos = light.position;
  return { color: litColor(light), position: [pos.x, pos.y, pos.z] };
};

/** Add a directional or point light and return its entity (re-posable via
 * `setLight`). Lights beyond the backends' capacity (8 directional + 8 point)
 * are accepted but do not contribute. */
export const addLight = (light: Light): Entity => {
  const st = requireState();
  const entity = nextEntity;
  nextEntity += 1;
  st.lights.set(entity, light);
  return entity;
};

/** Re-pose an existing light (direction/position, color, intensity — the whole
 * spec is replaced, so a light can be animated per frame like a node). */
export const setLight = (entity: Entity, light: Light): void => {
  const st = requireState();
  assert(st.lights.has(entity), `store: setLight got unknown light entity ${entity}`);
  st.lights.set(entity, light);
};

/** Remove a light from the retained scene (the reconciler drops a light whose
 * key a later `view` stops emitting). */
export const removeLight = (entity: Entity): void => {
  const st = requireState();
  assert(st.lights.has(entity), `store: removeLight got unknown light entity ${entity}`);
  st.lights.delete(entity);
};

/** Drop every node, light, mesh, and material (backend resources included). */
export const clearScene = (): void => {
  const st = requireState();
  st.backend.dropMeshes();
  st.meshKindCache.clear();
  st.materials.clear();
  st.nodes.clear();
  st.lights.clear();
};

/** Clear and draw the retained scene through the active backend. Lights are
 * resolved from their retained specs here, so a `setLight` re-pose is visible
 * on the very next frame. */
export const renderScene = (): void => {
  const st = requireState();
  const lights = [...st.lights.values()];
  st.backend.render({
    camera: st.camera,
    clearColor: st.clearColor,
    dirLights: lights
      .filter((light): light is Extract<Light, { kind: "directional" }> => isDirectional(light))
      .slice(0, MAX_DIR_LIGHTS)
      .map((light): FrameDirLight => resolveDirLight(light)),
    materials: st.materials,
    nodes: st.nodes.values(),
    pointLights: lights
      .filter((light): light is Extract<Light, { kind: "point" }> => isPoint(light))
      .slice(0, MAX_POINT_LIGHTS)
      .map((light): FramePointLight => resolvePointLight(light)),
  });
};
