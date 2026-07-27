/*
 * probe-readback.ts — the CONTROL PROBE, and the first thing the detection
 * ladder runs. It writes a known buffer with `putImageData` and reads it
 * straight back with `getImageData`. Nothing is rasterized: no fill, no shader,
 * no compositor. The browser is asked only to hand back bytes it was just
 * given.
 *
 * That makes it a control in the experimental sense. If THIS round trip comes
 * back perturbed, the perturbation is the browser's privacy policy — Brave's
 * farbling, Firefox's resistFingerprinting, Tor's blank canvas — and not a
 * driver, a shader, or a broken GPU. Every probe that follows reads this
 * verdict before believing a single pixel it reads back:
 *
 *   - `exact` / `noisy` → pixel evidence is admissible (the classifiers in
 *     `probe-pattern.ts` see through the noise).
 *   - `neutralised`     → pixel evidence has been taken away. A tier must then
 *     prove itself STRUCTURALLY (context created, no API error, context not
 *     lost, shaders linked), or a Tor user would be dropped to CSS3D on a
 *     perfectly good GPU.
 *
 * Platform edge: browser-API boundary, so ordinary control flow, coverage-exempt
 * (test-exempt.json), outside the Branchless Law. The rules it applies are pure
 * and fully covered in `probe-pattern.ts`.
 */

import { PATTERN_HEIGHT, PATTERN_WIDTH, type ReadbackTrust, classifyReadbackDelta, patternBytes } from "./probe-pattern.ts";

export interface ReadbackProbe {
  readonly detail: string;
  readonly trust: ReadbackTrust;
}

/** Run the control probe on a small offscreen canvas. Never throws: any failure
 * is reported as `neutralised`, the conservative verdict. */
export const probeReadback = (): ReadbackProbe => {
  try {
    const canvas = document.createElement("canvas");
    canvas.width = PATTERN_WIDTH;
    canvas.height = PATTERN_HEIGHT;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) {
      return { detail: "no 2d context for the control probe", trust: "neutralised" };
    }
    const written = patternBytes();
    // Copied into a fresh buffer because `ImageData` insists on an
    // `ArrayBuffer`-backed view; the bytes are identical.
    ctx.putImageData(new ImageData(new Uint8ClampedArray(written), PATTERN_WIDTH, PATTERN_HEIGHT), 0, 0);
    const read = ctx.getImageData(0, 0, PATTERN_WIDTH, PATTERN_HEIGHT).data;
    const trust = classifyReadbackDelta(written, read);
    return { detail: `putImageData -> getImageData: ${trust}`, trust };
  } catch (error) {
    return { detail: `readback threw: ${String(error)}`, trust: "neutralised" };
  }
};
