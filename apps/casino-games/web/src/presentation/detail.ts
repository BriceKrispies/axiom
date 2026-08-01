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

/** The DOM renderer, which pays per ELEMENT rather than per pixel. */
const DOM_BACKEND = "CSS3D";

/**
 * True on the DOM renderer, the signal to build the SPARSEST variant of a scene.
 *
 * `lowDetail()` is not enough for it, and the difference is a difference in kind,
 * not degree. The software rasterizer pays per pixel, so a 3-pixel gold trim is
 * nearly free there and `lowDetail()` only needs to shed the geometry that costs
 * fill. The DOM renderer pays per ELEMENT: that same 3-pixel trim costs a full
 * composite and depth-sort, exactly as much as the chest behind it. Its whole
 * budget is ~300 elements for 60fps, and this scene's chests alone are nine
 * copies of a twenty-part model.
 *
 * So on this backend the app drops what exists only to add a VALUE STEP at board
 * scale — end-cap wood, corner brackets, lid ribs — plus the set-dressing the
 * game never refers to (the sandcastle, the resident crab, the courier crab) and
 * the losing chests once the reveal's veil is over them. The chest is still a
 * chest: body, lid, dome, rim, hasp, plaque.
 *
 * This is a scene-authoring decision and belongs here with the rest of the detail
 * policy, for the same reason `weldedLetteringReads()` does: the backend's job is
 * HOW to draw, not WHAT is worth drawing.
 */
export const sparseDetail = (): boolean => {
  try {
    return rendererBackendName() === DOM_BACKEND;
  } catch {
    return false;
  }
};

/**
 * False when the live backend cannot render HAIRLINE WELDED GEOMETRY — lettering
 * built from many sub-pixel stroke boxes, as `stampText` produces.
 *
 * The pixel backends rasterize such a stroke for free: it costs a few fragments
 * and sub-pixel coverage makes it read. The CSS3D backend spends one composited
 * DOM element per stroke, and "ACME" alone is 23 of them (its `C` is a 12-segment
 * arc). Drawn at board scale each stroke projects to well under a pixel of area,
 * so the renderer's size LOD drops most of them — and a PARTIAL word ("A ME") is
 * far worse than none. Keeping them all means disabling the LOD outright, which
 * measured ~3900 elements at ~2fps.
 *
 * So the app declines to stamp welded lettering on that backend rather than
 * asking it to draw something it cannot. This is a scene-authoring decision and
 * belongs here with the rest of the detail policy; the backend's job is HOW to
 * draw, not WHAT is worth drawing.
 *
 * This is a statement about the BACKEND, not about a given piece of lettering —
 * it says nothing about how big the text is on screen. Callers combine it with
 * scale: the chest scene still stamps the brand on the SELECTED chest, which
 * flies to hero framing where the strokes are an order of magnitude larger and
 * render correctly.
 */
export const weldedLetteringReads = (): boolean => {
  try {
    return rendererBackendName() !== DOM_BACKEND;
  } catch {
    return true;
  }
};
