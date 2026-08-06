/*
 * Store: the retained-scene store behind the `api.ts` contract — a module-level
 * singleton (`initStore` once with an INJECTED backend, then the free scene
 * functions). It owns the SCENE — nodes, lights, camera, labels, clear color,
 * ambient — and assembles the per-frame `SceneFrame` it hands a `backend.ts`
 * backend. Resource REGISTRATION (geometry uploads, materials, the primitive
 * cache) is the other half of the store and lives in `store-resources.ts`; the
 * singleton itself lives here, and that file reaches it through the two internal
 * accessors below.
 *
 * This is the branchless, fully-covered spine half of the old `renderer.ts`: it
 * builds no WebGL/Canvas context itself, so every path is exercisable with a
 * fake backend. The thin `renderer.ts` edge resolves a real backend and hands it
 * here.
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
import type { Camera3D, Entity, Handle, Light, Rgba, Transform } from "./api.ts";
import { absentProbe, assert, demand } from "./branchless.ts";
import { fromTrs, normalMatrix } from "./mat4.ts";
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

/** The singleton, or a thrown error if the renderer was never initialized.
 * Exported for `store-resources.ts`, the other half of this store — NOT part of
 * the engine's public surface (`index.ts` does not re-export it). */
export const requireState = (): RendererState =>
  demand(state, "store: initStore(backend, canvas) must be called before any other store function");

/** Allocate the next resource handle. Lives here because the counter is part of
 * the singleton's identity; `store-resources.ts` draws from it so mesh and
 * material handles cannot collide. Internal, like `requireState`. */
export const allocHandle = (): Handle => {
  const handle = nextHandle;
  nextHandle += 1;
  return handle;
};

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

/**
 * Build a node record: its pose PLUS the two matrices every backend derives from
 * that pose.
 *
 * The matrices are cached here, at the one place a pose can change, rather than
 * recomputed per frame in each backend — which is what all three used to do. That
 * was the single largest avoidable cost in the frame: `fromTrs` ran once per node
 * per frame in `backend-webgl2`, `backend-canvas2d` AND `backend-css`, and
 * `normalMatrix` (the top named function in a live CPU profile of the chest
 * scene) ran once per node per frame in the GL path, each call allocating a
 * handful of vectors and a fresh `Float32Array`. A scene of a few hundred mostly
 * STATIC nodes was rebuilding every one of those matrices sixty times a second to
 * arrive at the same numbers.
 *
 * The store is the lowest correct owner because it is the only thing that knows
 * when a pose actually changed: `spawnRenderable` and `setNodeTransform` are the
 * only two ways in. So a static node now computes its matrices exactly once, ever,
 * and a moving node exactly once per re-pose — and all three backends get the
 * saving for free, from one implementation instead of three.
 *
 * `FrameNode` is fully readonly and replaced wholesale rather than mutated in
 * place, which is what makes the cache impossible to desync: there is no way to
 * write a new `transform` without also producing its matrices.
 */
const posedNode = (mesh: Handle, material: Handle, transform: Transform): FrameNode => {
  const model = fromTrs(transform.position, transform.rotation, transform.scale);
  return { material, mesh, model, normal: normalMatrix(model), transform };
};

/** Add a scene node drawing `mesh` with `material` at `transform`. */
export const spawnRenderable = (mesh: Handle, material: Handle, transform: Transform): Entity => {
  const st = requireState();
  assert(st.materials.has(material), `store: spawnRenderable got unknown material handle ${material}`);
  const entity = nextEntity;
  nextEntity += 1;
  st.nodes.set(entity, posedNode(mesh, material, transform));
  return entity;
};

/** Re-pose an existing node (its cached matrices are rebuilt with it). */
export const setNodeTransform = (entity: Entity, transform: Transform): void => {
  const st = requireState();
  const node = demand(st.nodes.get(entity), `store: setNodeTransform got unknown entity ${entity}`);
  st.nodes.set(entity, posedNode(node.mesh, node.material, transform));
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
