/*
 * probe-webgl.ts — the WebGL2 and WebGL1 rungs. One file, because the two
 * differ only in which context names to ask for: everything that makes the
 * probe trustworthy is identical.
 *
 * Everything here is a hard-won requirement, not a preference:
 *
 *   - `preserveDrawingBuffer: true`. Without it the drawing buffer is cleared
 *     the moment the page composites, and the probe reads back a blank buffer
 *     from a perfectly healthy GPU. For the same reason the draw and the
 *     `readPixels` happen in the SAME TASK — no `await`, no timer, nothing that
 *     could let a composite slip in between.
 *   - `failIfMajorPerformanceCaveat: true` FIRST, then a retry without it. The
 *     strict attempt is how the probe learns the difference between a real GPU
 *     and a software fallback (SwiftShader, a remote-desktop session with
 *     hardware acceleration off). Both are usable; only the first is
 *     `accelerated`, and that flag is what lets `detect.ts` skip the entire
 *     async WebGPU budget on a machine that has no acceleration at all.
 *   - `isContextLost()` BEFORE and AFTER drawing. A context can be created and
 *     immediately lost (a GPU reset, a driver blocklist applied late); a probe
 *     that only checked up front would report a working tier.
 *   - Disposal through `WEBGL_lose_context` in a `finally`, always. Chrome caps
 *     a page at roughly 16 live WebGL contexts and evicts the OLDEST when the
 *     cap is hit — which, without explicit disposal, would eventually be the
 *     GAME's context, killed by its own capability probe.
 *   - A small OFFSCREEN canvas, never the game's. Acquiring a context on the
 *     real canvas would permanently fix its type: a canvas that has handed out
 *     a WebGL context can never hand out a 2D one, so a failed probe would
 *     destroy the very fallback it exists to select.
 *
 * The pattern is painted two ways on purpose: three stripes by scissored
 * `gl.clear`, the fourth by a real compiled + linked + drawn quad. That
 * separates "the GL pipeline works" from "clears work but shaders do not" — a
 * real state on locked-down drivers, and one that would otherwise present as an
 * inexplicable black screen in the game.
 *
 * Platform edge: browser-API boundary — ordinary control flow, coverage-exempt.
 */

import {
  EXPECTED_SIGNATURE,
  PATTERN_HEIGHT,
  PATTERN_WIDTH,
  STRIPE_COLORS,
  STRIPE_COUNT,
  type ReadbackTrust,
  classifyPattern,
  signature,
  stripeBounds,
} from "./probe-pattern.ts";
import type { TierProbe } from "./tier.ts";

/** Which rung this probe is testing. */
export type GlVersion = "webgl2" | "webgl1";

type GlContext = WebGLRenderingContext | WebGL2RenderingContext;

const CONTEXT_NAMES: Record<GlVersion, readonly string[]> = {
  webgl1: ["webgl", "experimental-webgl"],
  webgl2: ["webgl2"],
};

const CHANNELS = 4;

/** GLSL ES 1.00 — accepted by WebGL1 and by WebGL2 alike, so one shader pair
 * covers both rungs. The probe is asking whether the compiler and linker work
 * at all, not what language level they support. */
const VERTEX_SOURCE = `attribute vec2 aPos;
void main() { gl_Position = vec4(aPos, 0.0, 1.0); }`;

const FRAGMENT_SOURCE = `precision mediump float;
void main() { gl_FragColor = vec4(1.0, 1.0, 1.0, 1.0); }`;

interface Acquired {
  readonly accelerated: boolean;
  readonly gl: GlContext;
}

/** `preserveDrawingBuffer` is the load-bearing one (the buffer is cleared at
 * composite without it); `strict` is the `failIfMajorPerformanceCaveat` pass
 * that tells a real GPU from a software fallback. */
const contextAttrs = (strict: boolean): WebGLContextAttributes => ({
  alpha: false,
  antialias: false,
  depth: false,
  failIfMajorPerformanceCaveat: strict,
  premultipliedAlpha: false,
  preserveDrawingBuffer: true,
  stencil: false,
});

/** A fresh offscreen canvas per attempt: a canvas whose context creation failed
 * is not guaranteed to hand one out later. */
const attempt = (version: GlVersion, strict: boolean): GlContext | null => {
  for (const name of CONTEXT_NAMES[version]) {
    const canvas = document.createElement("canvas");
    canvas.width = PATTERN_WIDTH;
    canvas.height = PATTERN_HEIGHT;
    const ctx = canvas.getContext(name, contextAttrs(strict));
    if (ctx) {
      return ctx as GlContext;
    }
  }
  return null;
};

/** Strict first (a real GPU), then relaxed (a software fallback is still a
 * usable tier — it just is not acceleration). */
const acquire = (version: GlVersion): Acquired | null => {
  const accelerated = attempt(version, true);
  if (accelerated) {
    return { accelerated: true, gl: accelerated };
  }
  const relaxed = attempt(version, false);
  return relaxed ? { accelerated: false, gl: relaxed } : null;
};

const compile = (gl: GlContext, type: number, source: string): WebGLShader | null => {
  const shader = gl.createShader(type);
  if (!shader) {
    return null;
  }
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (gl.getShaderParameter(shader, gl.COMPILE_STATUS) !== true) {
    gl.deleteShader(shader);
    return null;
  }
  return shader;
};

/** Compile + link + draw one white quad over the LAST stripe. Returns false
 * when any stage of the shader pipeline failed. */
const drawStripeQuad = (gl: GlContext, stripe: number): boolean => {
  const vertex = compile(gl, gl.VERTEX_SHADER, VERTEX_SOURCE);
  const fragment = compile(gl, gl.FRAGMENT_SHADER, FRAGMENT_SOURCE);
  const program = gl.createProgram();
  if (!vertex || !fragment) {
    return false;
  }
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  if (gl.getProgramParameter(program, gl.LINK_STATUS) !== true) {
    return false;
  }
  gl.useProgram(program);
  const bounds = stripeBounds(stripe, PATTERN_WIDTH);
  const left = (2 * bounds.start) / PATTERN_WIDTH - 1;
  const right = (2 * (bounds.start + bounds.span)) / PATTERN_WIDTH - 1;
  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([left, -1, right, -1, left, 1, right, 1]), gl.STATIC_DRAW);
  const location = gl.getAttribLocation(program, "aPos");
  gl.enableVertexAttribArray(location);
  gl.vertexAttribPointer(location, 2, gl.FLOAT, false, 0, 0);
  gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  return true;
};

/** Paint the pattern: scissored clears for every stripe but the last, a real
 * draw for the last. */
const paint = (gl: GlContext): boolean => {
  gl.viewport(0, 0, PATTERN_WIDTH, PATTERN_HEIGHT);
  gl.disable(gl.SCISSOR_TEST);
  gl.clearColor(0, 0, 0, 1);
  gl.clear(gl.COLOR_BUFFER_BIT);
  gl.enable(gl.SCISSOR_TEST);
  for (let stripe = 0; stripe < STRIPE_COUNT - 1; stripe += 1) {
    const bounds = stripeBounds(stripe, PATTERN_WIDTH);
    const [red, green, blue] = STRIPE_COLORS[stripe]!;
    gl.scissor(bounds.start, 0, bounds.span, PATTERN_HEIGHT);
    gl.clearColor(red, green, blue, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
  }
  gl.disable(gl.SCISSOR_TEST);
  return drawStripeQuad(gl, STRIPE_COUNT - 1);
};

/** Hand the context back to the driver immediately. Chrome's ~16-context cap
 * makes this mandatory, not tidy. */
const dispose = (gl: GlContext): void => {
  try {
    gl.getExtension("WEBGL_lose_context")?.loseContext();
  } catch {
    // A context that cannot be released explicitly is left to the GC; nothing
    // useful remains to be done, and a probe must never throw at its caller.
  }
};

const failed = (detail: string): TierProbe => ({ accelerated: false, detail, outcome: "fail" });

/** Which half of the pattern survived: the scissored clears, the shader draw,
 * both, or neither. */
const explainMismatch = (actual: readonly number[]): string => {
  const clears = EXPECTED_SIGNATURE.slice(0, STRIPE_COUNT - 1).every((code, index) => code === actual[index]);
  const drawn = actual[STRIPE_COUNT - 1] === EXPECTED_SIGNATURE[STRIPE_COUNT - 1];
  if (clears && !drawn) {
    return "scissored clears verified but the shader draw never appeared";
  }
  if (!clears && drawn) {
    return "the shader draw appeared but the scissored clears did not";
  }
  return "the rendered pattern does not match";
};

/**
 * Probe one GL rung. `trust` is the control probe's verdict: under
 * `neutralised` no pixel evidence is admissible, so the tier passes as
 * `degraded` on structural grounds — a context that was created, never lost,
 * linked its shaders, and raised no GL error.
 */
export const probeWebgl = (version: GlVersion, trust: ReadbackTrust): TierProbe => {
  let acquired: Acquired | null = null;
  try {
    acquired = acquire(version);
    if (!acquired) {
      return failed(`no ${version} context`);
    }
    const { accelerated, gl } = acquired;
    if (gl.isContextLost()) {
      return failed("context lost before drawing");
    }
    const shaders = paint(gl);
    const read = new Uint8Array(PATTERN_WIDTH * PATTERN_HEIGHT * CHANNELS);
    gl.readPixels(0, 0, PATTERN_WIDTH, PATTERN_HEIGHT, gl.RGBA, gl.UNSIGNED_BYTE, read);
    const error = gl.getError();
    if (gl.isContextLost()) {
      return failed("context lost while drawing");
    }
    if (error !== gl.NO_ERROR) {
      return failed(`GL error 0x${error.toString(16)}`);
    }
    if (!shaders) {
      return failed("shaders did not compile or link");
    }
    const caveat = accelerated ? "" : " (software: major performance caveat)";
    if (trust === "neutralised") {
      return { accelerated, detail: `context + shaders verified structurally${caveat}`, outcome: "degraded" };
    }
    const actual = signature(read, PATTERN_WIDTH, PATTERN_HEIGHT);
    const verdict = classifyPattern(actual, EXPECTED_SIGNATURE);
    if (verdict === "match") {
      return { accelerated, detail: `clears + shader draw verified${caveat}`, outcome: accelerated ? "pass" : "degraded" };
    }
    if (verdict === "uniform") {
      return failed("the drawing buffer came back uniform: nothing was drawn");
    }
    return failed(explainMismatch(actual));
  } catch (error) {
    return failed(`${version} probe threw: ${String(error)}`);
  } finally {
    if (acquired) {
      dispose(acquired.gl);
    }
  }
};
