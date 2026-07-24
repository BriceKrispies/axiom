/*
 * detail.ts — the app's render-detail policy. The Canvas2D backend is a per-pixel
 * SOFTWARE rasterizer, so its cost is dominated by triangle count; the WebGL2
 * backend shrugs off the same geometry on the GPU. `lowDetail()` lets the scene
 * builders shed purely-decorative geometry (fine letter reliefs, extra lid slats,
 * groove lines, spare palm fronds) on the software path to hold frame rate, while
 * WebGL2 keeps the full-fidelity scene.
 *
 * It reads the LIVE backend from the engine store, so it reflects whatever the
 * renderer actually resolved to (including the `?backend=canvas2d` force and any
 * automatic WebGL2→Canvas2D fallback). Before the renderer is initialized — e.g.
 * in a headless unit test that never mounts one — it degrades to full detail.
 */

import { rendererBackendName } from "@axiom/web-engine";

/** The engine's name for the software fallback backend. */
const SOFTWARE_BACKEND = "Canvas2D";

/**
 * True when the live renderer is the Canvas2D software rasterizer — the signal to
 * build the triangle-frugal variant of a scene. False on WebGL2, and false when no
 * renderer is mounted yet (full detail is the safe default).
 */
export const lowDetail = (): boolean => {
  try {
    return rendererBackendName() === SOFTWARE_BACKEND;
  } catch {
    return false;
  }
};
