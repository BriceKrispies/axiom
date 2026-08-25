/**
 * WEBGL BOOT PROBE — what the GPU driver was actually asked to do, and when.
 *
 * WHY. Boot time in this app is overwhelmingly GPU-adjacent work: shader
 * compilation (the material bakes, the character camo bake, the 180-program
 * pre-warm), texture uploads, and the readbacks that flush the pipeline. A
 * wall-clock span tree tells you `world init took 4.6 s`; it cannot tell you
 * that 3.5 s of that was 17 shader-driven texture bakes and 60 MB of uploads.
 * This does.
 *
 * HOW. We wrap the handful of WebGL2 entry points that are (a) rare enough
 * that wrapping them costs nothing and (b) capable of blocking the main thread
 * for a measurable time. Each wrapper adds its duration to the live counter
 * object owned by `boot`; `boot` snapshots those counters when a span opens
 * and diffs them when it closes, so every millisecond of GPU-facing work lands
 * on the span that caused it, with no per-span bookkeeping at the call site.
 *
 * THE ONE SUBTLETY WORTH KNOWING. `linkProgram` is usually near-free: drivers
 * defer the real link until someone asks for the result. On a driver with
 * KHR_parallel_shader_compile (which this app has — `prewarm` reports
 * `parallel: true`) the link runs on a worker thread and the cost surfaces at
 * `getProgramParameter(LINK_STATUS)`, which blocks until it is done. So
 * `linkStatusMs` — not `programLinkMs` — is the number that explains a slow
 * pre-warm. Both are recorded, separately, for exactly that reason.
 *
 * Draw calls ARE timed here, which is the opposite of the usual advice — see
 * the comment above the draw wrappers for why boot is the one case where a
 * draw call's duration means something.
 *
 * OPT-IN, AND THAT WAS LEARNED THE HARD WAY. This used to install itself on
 * every boot. Once the rest of the boot got fast enough, the probe became the
 * single largest JavaScript hotspot in the boot it was measuring: 608 ms, 10%
 * of the total, and it was shipping to players. Nearly all of it is one call —
 * `getProgramParameter`, which the parallel-compile poll asks tens of thousands
 * of times, each answer wrapped in two `performance.now()` calls.
 *
 * An instrument that changes the thing it measures by 10% is not measuring it.
 * `?profile=1` turns it on; `tools/bootprofile.mjs` passes that automatically,
 * and a normal load pays nothing.
 */

import { boot } from './profile.js';

/** Bytes for a texImage2D/texSubImage2D upload, best-effort. */
const texBytes = (width, height, format, type, gl) => {
  const channels =
    format === gl.RGBA || format === gl.RGBA_INTEGER ? 4
    : format === gl.RGB || format === gl.RGB_INTEGER ? 3
    : format === gl.RG || format === gl.RG_INTEGER ? 2
    : 1;
  const bytesPer =
    type === gl.FLOAT ? 4
    : type === gl.HALF_FLOAT || type === gl.UNSIGNED_SHORT || type === gl.SHORT ? 2
    : type === gl.UNSIGNED_INT || type === gl.INT || type === gl.UNSIGNED_INT_24_8 ? 4
    : 1;
  return (width | 0) * (height | 0) * channels * bytesPer;
};

/**
 * Install the probe on a live context. Idempotent — a second call on the same
 * context is a no-op, so a subsystem that re-acquires the context cannot
 * double-count.
 *
 * @param {WebGL2RenderingContext} gl
 * @returns {boolean} whether the probe was installed by this call
 */
export function probeGl(gl) {
  if (!gl || gl.__bootProbed) return false;
  // See the header: the probe is heavy enough to distort the boot, so it only
  // installs when something asked to be measured.
  const wanted = typeof location === 'undefined' ||
    new URLSearchParams(location.search).get('profile') === '1';
  if (!wanted) return false;
  gl.__bootProbed = true;

  const c = boot.counters;
  const now = () => performance.now();

  /** Wrap `name` so its duration lands in `msKey` and its call in `countKey`. */
  const timed = (name, countKey, msKey, bytesOf) => {
    const orig = gl[name];
    if (typeof orig !== 'function') return;
    gl[name] = function (...args) {
      const t = now();
      const r = orig.apply(this, args);
      c[msKey] += now() - t;
      c[countKey]++;
      if (bytesOf) c[bytesOf.key] += bytesOf.fn(args) || 0;
      return r;
    };
  };

  timed('compileShader', 'shaderCompiles', 'shaderCompileMs');
  timed('linkProgram', 'programLinks', 'programLinkMs');
  timed('readPixels', 'readPixels', 'readPixelsMs');
  timed('finish', 'finishes', 'finishMs');
  timed('clientWaitSync', 'fenceWaits', 'fenceWaitMs');

  // Texture uploads. texImage2D has two overloads; both put width/height at
  // positions 3/4 in the sized form, and neither carries them in the DOM-source
  // form (where the size comes from the element). Charge bytes only when they
  // are knowable, and always charge the time.
  for (const name of ['texImage2D', 'texImage3D', 'texSubImage2D', 'texSubImage3D']) {
    const orig = gl[name];
    if (typeof orig !== 'function') continue;
    gl[name] = function (...args) {
      const t = now();
      const r = orig.apply(this, args);
      c.texUploadMs += now() - t;
      c.texUploads++;
      // Sized overloads: (target, level, internalformat, width, height, border,
      // format, type, ...) for 2D, one more dimension for 3D.
      if (args.length >= 8 && typeof args[3] === 'number' && typeof args[4] === 'number') {
        const is3d = name.includes('3D');
        const fmt = args[is3d ? 7 : 6];
        const typ = args[is3d ? 8 : 7];
        const depth = is3d ? (args[5] | 0) || 1 : 1;
        c.texBytes += texBytes(args[3], args[4], fmt, typ, gl) * depth;
      }
      return r;
    };
  }

  for (const name of ['compressedTexImage2D', 'compressedTexSubImage2D']) {
    const orig = gl[name];
    if (typeof orig !== 'function') continue;
    gl[name] = function (...args) {
      const t = now();
      const r = orig.apply(this, args);
      c.texUploadMs += now() - t;
      c.texUploads++;
      const data = args.find((a) => a && a.byteLength !== undefined);
      if (data) c.texBytes += data.byteLength;
      return r;
    };
  }

  for (const name of ['bufferData', 'bufferSubData']) {
    const orig = gl[name];
    if (typeof orig !== 'function') continue;
    gl[name] = function (...args) {
      const t = now();
      const r = orig.apply(this, args);
      c.bufferUploadMs += now() - t;
      c.bufferUploads++;
      const src = args[1] ?? args[2];
      if (src && src.byteLength !== undefined) c.bufferBytes += src.byteLength;
      else if (typeof src === 'number') c.bufferBytes += src;
      return r;
    };
  }

  // THE IMPORTANT ONE. Split the two queries three.js makes on a program so a
  // parallel-compiling driver stops hiding its cost:
  //   COMPLETION_STATUS_KHR — the non-blocking poll three.js loops on;
  //   LINK_STATUS           — the blocking wait for the finished program.
  const COMPLETION_STATUS_KHR = 0x91b1;
  const origGetProgramParameter = gl.getProgramParameter;
  gl.getProgramParameter = function (program, pname) {
    const blocking = pname === gl.LINK_STATUS;
    const polling = pname === COMPLETION_STATUS_KHR;
    if (!blocking && !polling) return origGetProgramParameter.call(this, program, pname);
    const t = now();
    const r = origGetProgramParameter.call(this, program, pname);
    const ms = now() - t;
    if (blocking) {
      c.linkStatusMs += ms;
      c.linkStatusWaits++;
    } else {
      c.completionMs += ms;
      c.completionPolls++;
    }
    return r;
  };

  // Draws ARE timed, which is not the usual advice. In steady state a draw
  // call returns long before the GPU does the work and timing it measures
  // nothing. During BOOT it measures something specific and important: a
  // driver that defers program linking (every driver without
  // KHR_parallel_shader_compile, including the software rasterizer a headless
  // browser falls back to) does the link on the first draw that USES the
  // program. That cost has to land somewhere, and if it does not land here it
  // shows up as unexplained wall time and the profile is useless. Two
  // performance.now() calls across a few thousand boot draws is nothing.
  for (const name of [
    'drawArrays', 'drawElements',
    'drawArraysInstanced', 'drawElementsInstanced',
    'drawRangeElements', 'multiDrawArraysWEBGL', 'multiDrawElementsWEBGL',
  ]) {
    const orig = gl[name];
    if (typeof orig !== 'function') continue;
    gl[name] = function (...args) {
      const t = now();
      const r = orig.apply(this, args);
      c.drawMs += now() - t;
      c.drawCalls++;
      return r;
    };
  }

  // Same reasoning for useProgram: some drivers surface a deferred link there
  // rather than at the draw. We also record the DISTINCT set of programs ever
  // bound, which answers a question the pre-warm cannot answer about itself:
  // of the ~220 programs boot compiles, how many does the game ever draw with?
  // A program compiled and never bound is boot time spent on nothing.
  const origUseProgram = gl.useProgram;
  const bound = new Set();
  boot.programsBound = bound;
  gl.useProgram = function (program) {
    const t = now();
    const r = origUseProgram.call(this, program);
    c.programBindMs += now() - t;
    c.programBinds++;
    if (program) bound.add(program);
    c.distinctPrograms = bound.size;
    return r;
  };

  // PROGRAM REFLECTION — the hole that made a 13.9 s cold boot look like 19 ms
  // of blocking GL. A driver is free to defer the real link past linkProgram
  // AND past LINK_STATUS; NVIDIA defers it until something actually needs the
  // program's interface, which is the uniform and attribute queries three makes
  // when it first uses a program. Those are synchronous round trips to the GPU
  // process, so the cost lands here and nowhere else.
  for (const name of [
    'getUniformLocation', 'getActiveUniform', 'getActiveAttrib',
    'getAttribLocation', 'getProgramInfoLog', 'validateProgram',
  ]) {
    timed(name, 'programQueries', 'programQueryMs');
  }

  // ...and for the two other classic hidden stalls: a shader-compile status
  // query, and the pipeline flush a readback or a texture rebind can force.
  const origGetShaderParameter = gl.getShaderParameter;
  gl.getShaderParameter = function (shader, pname) {
    if (pname !== gl.COMPILE_STATUS) return origGetShaderParameter.call(this, shader, pname);
    const t = now();
    const r = origGetShaderParameter.call(this, shader, pname);
    c.compileStatusMs += now() - t;
    c.compileStatusWaits++;
    return r;
  };

  return true;
}
