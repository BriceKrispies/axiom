/*
 * Renderer: the backend-constructing PLATFORM EDGE of the store. This is the
 * one place that touches a real drawing backend: it resolves WebGL2 (the default
 * hardware path) or the Canvas2D software fallback, then injects the chosen
 * backend into the pure `store.ts` singleton via `initStore`. Everything else —
 * meshes, materials, nodes, lights, camera, and `renderScene` — lives branchless
 * and fully covered in `store.ts` and is re-exported by the package barrel, not
 * here.
 *
 * Backend selection:
 *   - "auto" (default) tries WebGL2 (`backend-webgl2.ts`) and falls back to
 *     Canvas2D (`backend-canvas2d.ts`) when the context is unavailable.
 *   - "webgl2" forces the hardware path and throws if it is unavailable.
 *   - "canvas2d" forces the software rasterizer.
 *
 * As a browser-API boundary this file is coverage-exempt (test-exempt.json) and
 * outside the Branchless Law — it keeps ordinary control flow.
 */

import type { RenderBackend } from "./backend.ts";
import { createCanvas2dBackend } from "./backend-canvas2d.ts";
import { createWebGl2Backend } from "./backend-webgl2.ts";
import { initStore } from "./store.ts";

/** Which drawing backend to use; "auto" tries WebGL2 and falls back to Canvas2D. */
export type BackendChoice = "auto" | "webgl2" | "canvas2d";

const resolveBackend = (canvas: HTMLCanvasElement, choice: BackendChoice): RenderBackend => {
  let backend: RenderBackend | null = null;
  if (choice !== "canvas2d") {
    backend = createWebGl2Backend(canvas);
    if (backend === null && choice === "webgl2") {
      throw new Error("renderer: WebGL2 was forced but is not available in this browser/canvas");
    }
  }
  return backend ?? createCanvas2dBackend(canvas);
};

/** Initialize the singleton renderer on `canvas`. `choice` defaults to "auto":
 * WebGL2 when the context is available, otherwise the Canvas2D software
 * fallback. Logs the selected backend once, then seeds the pure store. */
export const initRenderer = (canvas: HTMLCanvasElement, choice: BackendChoice = "auto"): void => {
  const backend = resolveBackend(canvas, choice);
  console.log(`axiom-engine: render backend = ${backend.name}`);
  initStore(backend, canvas);
};
