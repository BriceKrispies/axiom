/*
 * detail.ts — the app's render-detail policy. The Canvas2D backend is a per-pixel
 * SOFTWARE rasterizer, so its cost is dominated by triangle count; the CSS3D
 * backend draws one DOM element per merged polygon face, so its cost is dominated
 * by element count; the WebGL2 backend shrugs off the same geometry on the GPU.
 * `lowDetail()` lets the scene builders shed purely-decorative geometry (fine
 * letter reliefs, extra lid slats, groove lines, spare palm fronds) on both
 * non-GPU paths to hold frame rate, while WebGL2 keeps the full-fidelity scene.
 *
 * It reads the LIVE backend from the engine store, so it reflects whatever the
 * renderer actually resolved to (including the `?backend=canvas2d` / `?backend=css`
 * forces and any automatic WebGL2→Canvas2D fallback). Before the renderer is
 * initialized — e.g. in a headless unit test that never mounts one — it degrades
 * to full detail.
 */

import { rendererBackendName } from "@axiom/web-engine";

/** The engine's names for the two non-GPU backends: the software rasterizer and
 * the canvas-free DOM renderer. Both pay per primitive rather than per pixel on
 * the GPU, so both want the geometry-frugal variant of a scene. */
const FRUGAL_BACKENDS: readonly string[] = ["Canvas2D", "CSS3D"];

/**
 * True when the live renderer is a non-GPU backend (the Canvas2D software
 * rasterizer or the CSS3D DOM renderer) — the signal to build the triangle-frugal
 * variant of a scene. False on WebGL2, and false when no renderer is mounted yet
 * (full detail is the safe default).
 */
export const lowDetail = (): boolean => {
  try {
    return FRUGAL_BACKENDS.includes(rendererBackendName());
  } catch {
    return false;
  }
};
