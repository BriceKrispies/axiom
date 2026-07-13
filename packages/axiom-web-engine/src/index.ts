/*
 * index.ts — the public surface of @axiom/web-engine. A consumer imports
 * everything it needs from this one entry point: the value contract types, the
 * retained-scene store (create meshes/materials, spawn + pose nodes, lights,
 * camera, clear color, render), the backend-selecting `initRenderer` facade, the
 * fixed-step loop, input, and the tone/ambience audio.
 *
 * The internal spine (matrix math, mesh + shading generators, the backend
 * contract) and the concrete WebGL2 / Canvas2D backends are deliberately NOT
 * re-exported: they are the engine's private machinery, reachable only through
 * the store + `initRenderer`.
 */

// ── value contract ────────────────────────────────────────────────────────────
export type {
  Camera3D,
  EngineQuat,
  EngineVec3,
  Entity,
  Handle,
  Light,
  MaterialSpec,
  MeshData,
  MeshKind,
  Rgba,
  TickInput,
  ToneSpec,
  Transform,
} from "./api.ts";

// ── retained-scene store ────────────────────────────────────────────────────────
export {
  addLight,
  clearScene,
  createMaterial,
  createMesh,
  createMeshData,
  rendererBackendName,
  rendererNodeCount,
  renderScene,
  resizeRenderer,
  setCamera3D,
  setClearColor,
  setLight,
  setNodeTransform,
  spawnRenderable,
} from "./store.ts";

// ── backend-selecting facade ────────────────────────────────────────────────────
export type { BackendChoice } from "./renderer.ts";
export { initRenderer } from "./renderer.ts";

// ── fixed-step loop ─────────────────────────────────────────────────────────────
export type { LoopConfig } from "./raf-loop.ts";
export { startLoop } from "./raf-loop.ts";
export { FixedStepper } from "./stepper.ts";

// ── input ───────────────────────────────────────────────────────────────────────
export { InputState } from "./input.ts";
export { attachDomInput } from "./dom-input.ts";

// ── audio ───────────────────────────────────────────────────────────────────────
export { playTone, setAmbienceLevel, startAmbience, stopAmbience } from "./audio.ts";
