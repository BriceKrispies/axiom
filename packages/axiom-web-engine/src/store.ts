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
  DEFAULT_AMBIENT,
  type FrameDirLight,
  type FrameLabel,
  type FrameNode,
  type FramePointLight,
  MAX_DIR_LIGHTS,
  MAX_POINT_LIGHTS,
  type RenderBackend,
  type ResolvedMaterial,
} from "./backend.ts";
import type { Camera3D, Entity, Handle, Light, MaterialSpec, MeshData, MeshKind, Rgba, Transform } from "./api.ts";
import { absentProbe, assert, demand, orCompute, orElse, presentOf } from "./branchless.ts";
import { buildPrimitive, primitiveCacheKey, resolveFacets } from "./tessellation.ts";
import { isDirectional, isPoint, resolveDirLight, resolvePointLight } from "./light-resolve.ts";

/** The canvas backing store the renderer resizes (a real `HTMLCanvasElement`
 * structurally satisfies this; a fake `{ width, height }` does too). */
interface EngineCanvas {
  width: number;
  height: number;
}

interface RendererState {
  readonly canvas: EngineCanvas;
  readonly backend: RenderBackend;
  /** Built primitives, keyed by `kind:facets` — NOT by kind alone. Tessellation
   * is a property of how big a primitive is ON SCREEN, so two nodes of the same
   * kind at different sizes are legitimately different geometry. */
  readonly primitiveCache: Map<string, Handle>;
  readonly materials: Map<Handle, ResolvedMaterial>;
  readonly nodes: Map<Entity, FrameNode>;
  /** Lights are retained as their authored specs (keyed by entity, so they can
   * be re-posed via `setLight`) and resolved to frame lights at render time. */
  readonly lights: Map<Entity, Light>;
  camera: Camera3D;
  /** Scene text for the next frame. Replaced wholesale rather than diffed: there
   * are a handful at most, so a key-based reconcile would cost more than it saves
   * — and keeping labels out of the reconciler keeps that diff about geometry. */
  labels: readonly FrameLabel[];
  clearColor: [number, number, number];
  /** The scene's ambient (sky/bounce) floor, linear RGB — see `SceneFrame.ambient`. */
  ambient: [number, number, number];
}

const DEFAULT_CAMERA_HEIGHT = 2;
const DEFAULT_CAMERA_DISTANCE = 6;
const DEFAULT_FOV_DIVISOR = 3;
const DEFAULT_NEAR_PLANE = 0.1;
const DEFAULT_FAR_PLANE = 200;
const DEFAULT_EMISSIVE: Rgba = [0, 0, 0, 1];
const DEFAULT_OPACITY = 1;
// Absent roughness is fully MATTE: glossiness 0 -> zero specular, so a material
// that never sets roughness renders byte-identically to the old Lambert-only path.
const DEFAULT_ROUGHNESS = 1;

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
 * canvas it draws into. Sets the default camera, clear color, and ambient. */
export const initStore = (backend: RenderBackend, canvas: EngineCanvas): void => {
  const [cr, cg, cb] = CLEAR_COLOR;
  const [ar, ag, ab] = DEFAULT_AMBIENT;
  state = {
    ambient: [ar, ag, ab],
    backend,
    camera: DEFAULT_CAMERA,
    canvas,
    clearColor: [cr, cg, cb],
    labels: [],
    lights: new Map(),
    materials: new Map(),
    nodes: new Map(),
    primitiveCache: new Map(),
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
  // An `ao` array, when present, must carry one scalar per vertex; absent -> the
  // backends default it to 1.0, so this only fires on a genuine length mismatch.
  const aoLength = orElse(
    presentOf(data.ao).map((ao): number => ao.length)[0],
    data.positions.length,
  );
  assert(
    aoLength === data.positions.length,
    `store: createMeshData ao (${aoLength}) must match positions (${data.positions.length})`,
  );
  const handle = nextHandle;
  nextHandle += 1;
  st.backend.uploadMesh(handle, data);
  return handle;
};

/**
 * Get (or lazily build + cache) the shared geometry for a primitive kind at a
 * radial facet budget.
 *
 * `segments` is the budget at FULL detail; `tessellation.ts` resolves it against
 * the active backend's `detailScale` (the software path halves it), and the
 * result keys the cache — so asking for the same budget twice reuses one upload,
 * and asking for a bigger one adds geometry beside the default rather than
 * replacing it. Omitting `segments` reproduces the previous fixed counts exactly
 * (24/12 cylinder, 16×24 / 8×12 sphere).
 */
export const createMesh = (kind: MeshKind, segments?: number): Handle => {
  const st = requireState();
  const facets = resolveFacets(segments, st.backend.detailScale);
  const cacheKey = primitiveCacheKey(kind, facets);
  return orCompute(st.primitiveCache.get(cacheKey), (): Handle => {
    const handle = createMeshData(buildPrimitive(kind, facets));
    st.primitiveCache.set(cacheKey, handle);
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
    roughness: orElse(spec.roughness, DEFAULT_ROUGHNESS),
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

/** Set the scene TEXT drawn by the next `renderScene` (see `SceneLabel`). A
 * backend with no way to draw text ignores it. */
export const setLabels = (labels: readonly FrameLabel[]): void => {
  requireState().labels = labels;
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

/**
 * Set the scene's AMBIENT (sky/bounce) light — the omni-directional diffuse floor
 * every surface receives regardless of orientation, before the albedo multiply
 * (the alpha channel is ignored). Both backends read it per frame.
 *
 * This is the honest knob for "how bright and what color is the environment
 * around the subject." A warm value keeps away-facing faces in the family of the
 * lit ones (a sunlit beach, where sand bounces light back up); near-zero gives a
 * hard-vacuum key-only look. It is NOT a substitute for a light: it has no
 * direction and casts nothing. Use it instead of the two workarounds it replaces
 * — a near-white directional "fill" (still directional, so it leaves a third set
 * of faces crushed) and fake material `emissive` (paint compensating for light).
 *
 * Defaults to `DEFAULT_AMBIENT`; a scene that never calls this is unchanged.
 */
export const setAmbient = (color: Rgba): void => {
  const [ar, ag, ab] = color;
  requireState().ambient = [ar, ag, ab];
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
  st.primitiveCache.clear();
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
    ambient: st.ambient,
    camera: st.camera,
    clearColor: st.clearColor,
    dirLights: lights
      .filter((light): light is Extract<Light, { kind: "directional" }> => isDirectional(light))
      .slice(0, MAX_DIR_LIGHTS)
      .map((light): FrameDirLight => resolveDirLight(light)),
    labels: st.labels,
    materials: st.materials,
    nodes: st.nodes.values(),
    pointLights: lights
      .filter((light): light is Extract<Light, { kind: "point" }> => isPoint(light))
      .slice(0, MAX_POINT_LIGHTS)
      .map((light): FramePointLight => resolvePointLight(light)),
  });
};
