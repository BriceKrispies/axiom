#!/usr/bin/env node
/**
 * BOOT PROFILER — reads src/core/profile.js and says where the load went.
 *
 * The question this tool exists to answer is not "how long is boot" (a
 * stopwatch does that) but "which second is which". It reports four things the
 * console lines could not:
 *
 *  1. THE PHASE SPLIT, including the part before the app runs at all. The
 *     module graph is ~150 unbundled requests in dev and one bundle in a
 *     production build; that difference is seconds, and it is invisible to any
 *     instrument that starts when main.js starts.
 *  2. A SPAN TREE with self time. `world init 4600ms` overlaps
 *     `materials bake 3500ms`, so the two cannot be added. Self time removes
 *     the overlap and makes the costs sum to the whole.
 *  3. WHAT KIND OF WORK each span did — shader compiles, the blocking
 *     link-status wait, texture bytes, draw calls — from the GL probe.
 *  4. WHICH SHADER CACHE WAS HOT. Measured on this app: the same boot is 54 s,
 *     11 s, or 10 s depending only on what was already cached, and the cache
 *     that dominates is not the one you would guess.
 *
 *       --icy    delete the GPU DRIVER's on-disk program cache first. This is
 *                the only true cold measurement and the only one that reflects
 *                a player's first ever visit. Measured here: 54 s.
 *       (default) fresh browser profile, driver cache left alone. Measured: 11 s
 *                — i.e. the browser profile is nearly irrelevant.
 *       --warm   reuse a browser profile too. Measured: 10 s.
 *
 *     The driver cache is per MACHINE, not per browser profile, and it is keyed
 *     on shader source: editing a shader evicts exactly the programs you
 *     touched and nothing else. That is why "it takes 30 seconds" is real for a
 *     developer editing shaders and not reproducible for anyone else. Quoting a
 *     boot number without saying which regime it came from is not a measurement.
 *
 *  5. JS vs BLOCKED vs IDLE, per phase. `--samples` runs V8's CPU profiler
 *     alongside the span tree and crosses the two, so every phase reports how
 *     much of its wall time was running JavaScript, how much was blocked inside
 *     WebGL, and how much was an IDLE main thread parked on a compile-completion
 *     poll. That last column is the one that changes what you do: idle time is
 *     not slow code, it is capacity the app declined to use, and it is fixed by
 *     scheduling rather than by optimising anything.
 *
 * A WORD ON THE SAMPLER. The obvious choice is the in-page JS Self-Profiling
 * API, and it is wrong for this job: measured against this app it attributed
 * 240 ms of a 2 510 ms fully-synchronous CPU bake and called the other 2.3 s
 * "not in JS". `--samples` therefore drives V8's own profiler over CDP, which
 * attributes that span correctly and additionally names `(garbage collector)`
 * and `(program)` frames. `--selfprofile` keeps the in-page path for a browser
 * where CDP is not available; it needs the `Document-Policy: js-profiling`
 * header, which vite.config.js sets for dev and preview.
 *
 * USAGE
 *   node tools/bootprofile.mjs --samples             # the one you usually want
 *   node tools/bootprofile.mjs --icy --samples       # a first ever visit
 *   node tools/bootprofile.mjs --warm --repeat=3     # reload cost, median of 3
 *   node tools/bootprofile.mjs --query=prewarm=0     # what pre-warm costs
 *   node tools/bootprofile.mjs --q=low --dpr=2       # a preset, at Retina
 *   node tools/bootprofile.mjs --json --out=boot.json
 *   node tools/bootprofile.mjs --compare=boot.json   # diff against a baseline
 *
 * FLAGS
 *   --port=5173 --w --h --dpr        where and how big
 *   --q=low|medium|high|ultra        quality preset (?q=)
 *   --query=a=1&b=2                  extra URL params
 *   --no-glprobe                     skip the WebGL call probe. It costs ~600 ms
                                    of the boot it measures, so this is how you
                                    see the boot a player actually gets.
   --samples [--interval=200]       V8 CPU profiler, µs sampling interval
 *   --selfprofile                    in-page sampler instead (see above)
 *   --icy | --warm                   shader-cache regime (see above)
 *   --repeat=N                       report the median of N runs, per phase —
                                    the only reliable way to A/B a change here
   --sub                            include sub-phases in the median table
 *   --min=15                         hide spans under N ms in the tree
 *   --headed                         visible browser
 *   --angle=gl|d3d11|metal           force an ANGLE backend. Needed when a
 *                                    headless run falls back to SwiftShader,
 *                                    which the report warns about loudly.
 *   --channel=chrome                 use installed Chrome, not bundled Chromium
 *   --emit-weights                   regenerate src/core/bootweights.js, the
                                    loading bar's phase cost table
   --json [--out=FILE]              machine-readable output
 *   --compare=FILE                   diff phase times against a saved run
 */
import { chromium } from 'playwright';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir, homedir } from 'node:os';
import { join, resolve } from 'node:path';

/**
 * Where each GPU driver keeps its on-disk compiled-program cache. Emptying it
 * is the only way to measure a genuine first-visit boot; nothing the browser
 * exposes can do it, because the cache does not belong to the browser.
 */
const DRIVER_CACHES = [
  process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, 'NVIDIA', 'GLCache'),
  process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, 'AMD', 'GLCache'),
  process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, 'D3DSCache'),
  join(homedir(), '.nv', 'GLCache'),
  join(homedir(), '.cache', 'nvidia', 'GLCache'),
  join(homedir(), '.cache', 'mesa_shader_cache'),
  join(homedir(), '.cache', 'radv_builtin_shaders'),
].filter(Boolean);

function clearDriverCaches() {
  const cleared = [];
  for (const dir of DRIVER_CACHES) {
    if (!existsSync(dir)) continue;
    try {
      rmSync(dir, { recursive: true, force: true });
      cleared.push(dir);
    } catch (err) {
      // A cache in use by another process is normal; report and carry on
      // rather than pretending the run was icy when it was not.
      console.error(`  ! could not clear ${dir}: ${err.message}`);
    }
  }
  return cleared;
}

const args = Object.fromEntries(
  process.argv.slice(2).map((a) => {
    const m = a.match(/^--([^=]+)(?:=(.*))?$/);
    return m ? [m[1], m[2] ?? true] : [a, true];
  })
);

const PORT = Number(args.port ?? 5173);
const W = Number(args.w ?? 1280);
const H = Number(args.h ?? 720);
const DPR = Number(args.dpr ?? 1);
const REPEAT = Number(args.repeat ?? 1);
const WARM = !!args.warm;
const SAMPLES = !!args.samples;
const TIMEOUT = Number(args.timeout ?? 300000);
/** Persistent profile dir for --warm, so run 2 sees run 1's shader cache. */
const WARM_DIR = join(process.cwd(), '.bootprofile-profile');

// ---------------------------------------------------------------- formatting --
const ms = (v) => `${v.toFixed(0).padStart(6)}ms`;
const pct = (v, total) => `${((v / total) * 100).toFixed(1).padStart(5)}%`;
const kb = (b) => (b >= 1 << 20 ? `${(b / (1 << 20)).toFixed(1)}MB` : `${(b / 1024).toFixed(0)}KB`);

const C = process.stdout.isTTY && !args.json
  ? { dim: (s) => `\x1b[2m${s}\x1b[0m`, b: (s) => `\x1b[1m${s}\x1b[0m`,
      r: (s) => `\x1b[31m${s}\x1b[0m`, y: (s) => `\x1b[33m${s}\x1b[0m`,
      g: (s) => `\x1b[32m${s}\x1b[0m`, c: (s) => `\x1b[36m${s}\x1b[0m` }
  : { dim: (s) => s, b: (s) => s, r: (s) => s, y: (s) => s, g: (s) => s, c: (s) => s };

/** Colour a duration by how much of the whole boot it is. */
const heat = (v, total) => (v / total > 0.15 ? C.r : v / total > 0.05 ? C.y : v / total > 0.01 ? C.c : C.dim);

/** The GL counters worth printing next to a span, in the order they matter. */
const GL_LABEL = [
  ['shaderCompiles', (v) => `${v} shd`],
  ['compileStatusMs', (v) => `${v.toFixed(0)}ms compile-wait`],
  ['linkStatusMs', (v) => `${v.toFixed(0)}ms link-wait`],
  ['programLinks', (v) => `${v} link`],
  ['completionMs', (v) => `${v.toFixed(0)}ms poll`],
  ['texUploads', (v) => `${v} tex`],
  ['texBytes', (v) => kb(v)],
  ['texUploadMs', (v) => `${v.toFixed(0)}ms upload`],
  ['readPixelsMs', (v) => `${v.toFixed(0)}ms readback`],
  ['drawCalls', (v) => `${v} draws`],
  ['drawMs', (v) => `${v.toFixed(0)}ms in draws`],
  ['programBindMs', (v) => `${v.toFixed(0)}ms useProgram`],
  ['programQueryMs', (v) => `${v.toFixed(0)}ms program-reflect`],
  ['fenceWaitMs', (v) => `${v.toFixed(0)}ms fence`],
  ['finishMs', (v) => `${v.toFixed(0)}ms finish`],
];

/** Every counter the report reads, so a missing one reads 0 rather than throwing. */
const GL_LABEL_DEFAULTS = {
  shaderCompiles: 0, shaderCompileMs: 0, programLinks: 0, programLinkMs: 0,
  linkStatusWaits: 0, linkStatusMs: 0, completionPolls: 0, completionMs: 0,
  texUploads: 0, texUploadMs: 0, texBytes: 0, bufferUploads: 0, bufferUploadMs: 0,
  bufferBytes: 0, compileStatusWaits: 0, compileStatusMs: 0, readPixels: 0,
  readPixelsMs: 0, drawCalls: 0, drawMs: 0, programBinds: 0, programBindMs: 0,
  programQueries: 0, programQueryMs: 0, distinctPrograms: 0, fenceWaits: 0,
  fenceWaitMs: 0, finishes: 0, finishMs: 0,
};

const glSummary = (gl) => {
  if (!gl) return '';
  const parts = [];
  for (const [k, fmt] of GL_LABEL) {
    const v = gl[k];
    // Sub-millisecond timings are noise at this scale; counts always print.
    if (v === undefined || v === 0) continue;
    if (k.endsWith('Ms') && v < 1) continue;
    parts.push(fmt(v));
  }
  return parts.length ? C.dim(`  [${parts.join(' · ')}]`) : '';
};

const PROGRAM_KEY_PARAMS = [
  'precision', 'outputColorSpace', 'envMapMode', 'envMapCubeUVHeight',
  'mapUv', 'alphaMapUv', 'lightMapUv', 'aoMapUv', 'bumpMapUv', 'normalMapUv',
  'displacementMapUv', 'emissiveMapUv', 'metalnessMapUv', 'roughnessMapUv',
  'anisotropyMapUv', 'clearcoatMapUv', 'clearcoatNormalMapUv',
  'clearcoatRoughnessMapUv', 'iridescenceMapUv', 'iridescenceThicknessMapUv',
  'sheenColorMapUv', 'sheenRoughnessMapUv', 'specularMapUv',
  'specularColorMapUv', 'specularIntensityMapUv', 'transmissionMapUv',
  'thicknessMapUv', 'combine', 'fogExp2', 'sizeAttenuation',
  'morphTargetsCount', 'morphAttributeCount',
  'numDirLights', 'numPointLights', 'numSpotLights', 'numSpotLightMaps',
  'numHemiLights', 'numRectAreaLights', 'numDirLightShadows',
  'numPointLightShadows', 'numSpotLightShadows', 'numSpotLightShadowsWithMaps',
  'numLightProbes', 'shadowMapType', 'toneMapping', 'numClippingPlanes',
  'numClipIntersection', 'depthPacking',
];

const PRECISIONS = new Set(['highp', 'mediump', 'lowp']);

/**
 * Explain the WebGL program population: how many programs exist, which shaders
 * they are permutations OF, and which cache-key field is doing the permuting.
 *
 * WHY THIS BELONGS IN THE PROFILER. Program count is the single biggest lever
 * on a cold boot — a GPU driver with an empty cache spends most of the load
 * linking, and three.js mints one program per distinct combination of ~50
 * parameters. Knowing "206 programs" tells you nothing actionable. Knowing
 * "numPointLights takes the values 20, 21 and 32, which is a 3x multiplier on
 * every lit material in the game" tells you exactly what to go and pin.
 *
 * Finding the parameter block inside a cache key needs care: the key is a
 * comma-join whose head (shaderID, then two tokens per #define) and tail
 * (`customProgramCacheKey`, which in this app embeds whole GLSL chunks, commas
 * and all) are both variable-length. So anchor on the middle instead — the
 * parameter block always opens with a precision token followed by a colour
 * space, which nothing else in the key looks like.
 */
function analyseProgram(keys) {
  const tok = (k) => k.split(',');
  const rows = keys.map(tok);
  const anchor = rows[0].findIndex(
    (t, i) => PRECISIONS.has(t) && /^(srgb|srgb-linear|display-p3|linear-display-p3|)$/.test(rows[0][i + 1] ?? 'x')
  );
  const n = Math.min(...rows.map((r) => r.length));
  const varying = [];
  for (let i = 0; i < n; i++) {
    const vals = new Set(rows.map((r) => r[i]));
    if (vals.size === 1) continue;
    const name = anchor >= 0 && i >= anchor && i - anchor < PROGRAM_KEY_PARAMS.length
      ? PROGRAM_KEY_PARAMS[i - anchor]
      : `key[${i}]`;
    varying.push({ name, values: [...vals].map((v) => (v.length > 24 ? `${v.slice(0, 24)}…` : v)) });
  }
  // A trailing length difference means the custom key itself differs.
  const lens = new Set(rows.map((r) => r.length));
  return { varying, ragged: lens.size > 1 };
}

// -------------------------------------------------------------------- capture --
async function runOnce(index) {
  const userDataDir = WARM ? WARM_DIR : mkdtempSync(join(tmpdir(), 'shmup-boot-'));
  if (WARM) mkdirSync(WARM_DIR, { recursive: true });
  const clearedCaches = args.icy ? clearDriverCaches() : [];

  // A persistent context is the only way to control the profile directory, and
  // the profile directory is the only way to control the GLSL disk cache — the
  // single biggest cold/warm difference in this app.
  const ctx = await chromium.launchPersistentContext(userDataDir, {
    headless: !args.headed,
    viewport: { width: W, height: H },
    deviceScaleFactor: DPR,
    // `channel: chrome` uses the installed browser rather than Playwright's
    // bundled Chromium. Worth knowing about: the bundled build commonly falls
    // back to SwiftShader in headless mode on Windows, and a software-rasterised
    // boot profile is not a measurement of this app.
    ...(args.channel ? { channel: String(args.channel) } : {}),
    args: [
      '--ignore-gpu-blocklist',
      '--mute-audio',
      '--enable-gpu-rasterization',
      // Long-task + self-profiling need a real, non-throttled main thread.
      '--disable-background-timer-throttling',
      '--disable-renderer-backgrounding',
      // ANGLE backend. Default (unset) lets Chrome choose — which is what a
      // player gets. `--angle=gl` is the escape hatch for a headless run that
      // would otherwise land on SwiftShader.
      ...(args.angle ? [`--use-angle=${args.angle}`] : []),
      ...(args.gpuargs ? String(args.gpuargs).split(',') : []),
    ],
  });
  const page = await ctx.newPage();

  const consoleLines = [];
  const errors = [];
  page.on('console', (m) => consoleLines.push(`[${m.type()}] ${m.text()}`));
  page.on('pageerror', (e) => errors.push(e.message ?? String(e)));

  // V8's CPU profiler, over CDP. This is the sampler of record.
  //
  // The in-page JS Self-Profiling API (`--selfprofile`) is the obvious choice
  // and it is the WRONG one here: measured against this app it attributed
  // ~240 ms of a 2 510 ms fully-synchronous CPU bake, reporting the other 2.3 s
  // as "not in JS". V8's profiler attributes the same span correctly AND names
  // the frames the other one cannot — `(garbage collector)`, `(program)` — which
  // is exactly the difference between "this function is slow" and "this
  // function is allocating so hard the GC is eating the boot".
  //
  // It also needs no `Document-Policy` header, so it works against a production
  // build served by anything.
  let cdp = null;
  if (SAMPLES && !args.selfprofile) {
    cdp = await ctx.newCDPSession(page);
    await cdp.send('Profiler.enable');
    // 200 µs. Boot is seconds long, so this is affordable and it resolves
    // individual bakes rather than smearing them together.
    await cdp.send('Profiler.setSamplingInterval', { interval: Number(args.interval ?? 200) });
    await cdp.send('Profiler.start');
  }

  const query = new URLSearchParams();
  if (args.query) for (const [k, v] of new URLSearchParams(String(args.query))) query.set(k, v);
  if (args.q) query.set('q', String(args.q));
  // The WebGL probe distorts the boot badly enough that it does not install
  // itself; ask for it explicitly. See src/core/glprobe.js.
  if (!args['no-glprobe']) query.set('profile', '1');
  if (SAMPLES) query.set('jsprofile', '1');
  if (args.jsinterval) query.set('jsinterval', String(args.jsinterval));
  const url = `http://127.0.0.1:${PORT}/${query.size ? `?${query}` : ''}`;

  const t0 = Date.now();
  await page.goto(url, { waitUntil: 'commit', timeout: TIMEOUT });
  await page.waitForFunction('window.__READY__ === true', null, { timeout: TIMEOUT });
  const wallMs = Date.now() - t0;

  // Progressive boot: __READY__ is the first playable frame, and streaming +
  // pre-warm finish behind it. Wait for __LOADED__ too, so the report can show
  // both — but never block on it forever, since ?prewarm=0 and older builds
  // never raise it.
  await page
    .waitForFunction('window.__LOADED__ === true', null, { timeout: 60000 })
    .catch(() => {});
  const profile = await page.evaluate(() => window.__BOOTPROFILE__ ?? null);
  if (!profile) {
    await ctx.close();
    throw new Error(
      'the page raised __READY__ but published no __BOOTPROFILE__ — is src/core/profile.js imported first in src/main.js?'
    );
  }

  // Navigation + resource timing: the module-graph cost, which lives entirely
  // before the profiler's own time origin.
  const net = await page.evaluate(() => {
    const nav = performance.getEntriesByType('navigation')[0] ?? {};
    const res = performance.getEntriesByType('resource');
    const byExt = {};
    for (const r of res) {
      const ext = (r.name.split('?')[0].match(/\.(\w+)$/)?.[1] ?? 'other').toLowerCase();
      const e = (byExt[ext] ??= { count: 0, ms: 0, bytes: 0 });
      e.count++;
      e.ms += r.duration;
      e.bytes += r.encodedBodySize || 0;
    }
    return {
      responseEnd: +(nav.responseEnd ?? 0).toFixed(1),
      domContentLoaded: +(nav.domContentLoadedEventEnd ?? 0).toFixed(1),
      resourceCount: res.length,
      // Wall time of the request FAN, not the sum of durations: parallel
      // requests overlap, so summing them overstates the cost several times.
      resourceSpanMs: +(Math.max(0, ...res.map((r) => r.responseEnd)) -
        Math.min(Infinity, ...res.map((r) => r.startTime))).toFixed(1),
      byExt: Object.fromEntries(
        Object.entries(byExt).map(([k, v]) => [k, { count: v.count, ms: +v.ms.toFixed(0), bytes: v.bytes }])
      ),
      slowest: res
        .map((r) => ({ n: r.name.replace(/^https?:\/\/[^/]+\//, ''), ms: +r.duration.toFixed(1) }))
        .sort((a, b) => b.ms - a.ms)
        .slice(0, 10),
    };
  });

  const runtime = await page.evaluate(() => {
    const e = window.__ENGINE__;
    const r = e?.ctx?.peek?.('render');
    const gl = r?.renderer?.getContext?.();
    // WHICH GPU. A headless browser that fell back to SwiftShader produces
    // boot numbers several times the real ones, concentrated in exactly the
    // phase you are trying to optimise (shader link, rasterisation). Reporting
    // it is not a nicety — a profile that does not say which renderer produced
    // it is not evidence.
    const dbg = gl?.getExtension?.('WEBGL_debug_renderer_info');
    return {
      renderer: dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : (gl?.getParameter?.(gl.RENDERER) ?? null),
      vendor: dbg ? gl.getParameter(dbg.UNMASKED_VENDOR_WEBGL) : null,
      parallelCompile: !!gl?.getExtension?.('KHR_parallel_shader_compile'),
      quality: e?.config?.quality ?? null,
      pixelRatio: r?.renderer?.getPixelRatio?.() ?? null,
      drawingBuffer: gl ? [gl.drawingBufferWidth, gl.drawingBufferHeight] : null,
      programs: r?.renderer?.info?.programs?.length ?? null,
      geometries: r?.renderer?.info?.memory?.geometries ?? null,
      textures: r?.renderer?.info?.memory?.textures ?? null,
      prewarm: window.__PREWARM__ ?? null,
      bakery: e?.bakery ? { workers: e.bakery.size, ...e.bakery.stats } : null,
      heapMb: performance.memory ? performance.memory.usedJSHeapSize >> 20 : null,
    };
  });

  const programs = args.programs
    ? await page.evaluate(() => {
        const r = window.__ENGINE__?.ctx?.peek?.('render')?.renderer;
        return (r?.info?.programs ?? []).map((p) => ({
          name: p.name || '(unnamed)',
          key: p.cacheKey,
          used: p.usedTimes,
        }));
      })
    : null;

  let samples = null;
  let cpuProfile = null;
  if (cdp) {
    // Stop first, then read the page clock: the two are then separated by one
    // CDP round trip (~1 ms) rather than by a full profile serialisation. That
    // one offset is the entire clock alignment — V8's profiler timestamps and
    // performance.now() tick at the same rate, they just have different epochs.
    const { profile } = await cdp.send('Profiler.stop');
    const pageNowMs = await page.evaluate(() => performance.now());
    cpuProfile = { ...profile, offsetMs: pageNowMs - profile.endTime / 1000 };
  } else if (SAMPLES) {
    await page.waitForFunction('window.__BOOTSAMPLES_DONE__ === true', null, { timeout: 60000 });
    samples = await page.evaluate(() => window.__BOOTSAMPLES__ ?? null);
  }

  await ctx.close();
  return { index, wallMs, profile, net, runtime, samples, cpuProfile, programs, errors, consoleLines,
           regime: args.icy ? 'icy' : WARM ? 'warm' : 'cold-browser', clearedCaches };
}

// ------------------------------------------------------------------ analysis --
/** Flatten the span tree, keeping the parent chain for readable names. */
function flatten(node, out = [], path = []) {
  const here = [...path, node.name];
  out.push({ ...node, path: here, depth: path.length });
  for (const c of node.children ?? []) flatten(c, out, here);
  return out;
}

/**
 * Normalise a V8 CPU profile (CDP `Profiler.stop`) into the flat sample list
 * the rest of this file works with: `{ tMs, frames: [{name, file}] }`, leaf
 * first, on the page's `performance.now()` clock.
 *
 * V8 gives a node tree plus a parallel `samples`/`timeDeltas` pair; a sample is
 * a node id, and the node's parent chain is the stack. Time is microseconds
 * from `profile.startTime`, cumulative over `timeDeltas`.
 *
 * The synthetic frames matter and are kept, not filtered: `(garbage collector)`
 * is the difference between slow code and code that allocates too much, and
 * `(program)` is time inside the engine itself (parse, compile, native calls) —
 * exactly the class of cost the in-page profiler reports as "not in JS".
 */
function normaliseCpuProfile(cp) {
  if (!cp?.nodes?.length || !cp.samples?.length) return null;
  const byId = new Map(cp.nodes.map((n) => [n.id, n]));
  const parent = new Map();
  for (const n of cp.nodes) for (const c of n.children ?? []) parent.set(c, n.id);

  const frameOf = (n) => {
    const f = n.callFrame ?? {};
    const file = (f.url || '').replace(/^https?:\/\/[^/]+\//, '').split('?')[0] || '(native)';
    return { name: f.functionName || '(anonymous)', file };
  };
  const stackCache = new Map();
  const stackOf = (id) => {
    let s = stackCache.get(id);
    if (s) return s;
    s = [];
    let cur = id;
    // Guard against a malformed parent chain rather than spinning forever.
    for (let guard = 0; cur !== undefined && guard < 512; guard++) {
      const n = byId.get(cur);
      if (!n) break;
      s.push(frameOf(n));
      cur = parent.get(cur);
    }
    stackCache.set(id, s);
    return s;
  };

  const out = [];
  let tUs = cp.startTime;
  for (let i = 0; i < cp.samples.length; i++) {
    tUs += cp.timeDeltas[i] ?? 0;
    out.push({
      tMs: tUs / 1000 + cp.offsetMs,
      dtMs: (cp.timeDeltas[i + 1] ?? cp.timeDeltas[i] ?? 0) / 1000,
      frames: stackOf(cp.samples[i]),
    });
  }
  return out;
}

/** Same shape, from the in-page JS Self-Profiling trace (`--selfprofile`). */
function normaliseSelfProfile(trace) {
  if (!trace || trace.error || !trace.samples?.length) return null;
  const { frames, stacks, samples, resources } = trace;
  const frameOf = (fi) => {
    const f = frames[fi];
    const res = f.resourceId !== undefined ? resources[f.resourceId] : null;
    return {
      name: f.name || '(anonymous)',
      file: res ? res.replace(/^https?:\/\/[^/]+\//, '').split('?')[0] : '(native)',
    };
  };
  const gaps = [];
  for (let i = 1; i < samples.length; i++) gaps.push(samples[i].timestamp - samples[i - 1].timestamp);
  gaps.sort((a, b) => a - b);
  const medGap = gaps.length ? gaps[gaps.length >> 1] : 10;
  return samples.map((s, i) => {
    const st = [];
    let si = s.stackId;
    while (si !== undefined) {
      st.push(frameOf(stacks[si].frameId));
      si = stacks[si].parentId;
    }
    return {
      tMs: s.timestamp,
      dtMs: i + 1 < samples.length ? samples[i + 1].timestamp - s.timestamp : medGap,
      // A sample with no stack means the thread was outside JS entirely.
      frames: st.length ? st : [{ name: '(not in JS)', file: '(native)' }],
    };
  });
}

/**
 * THE QUESTION THAT DECIDES WHAT IS FIXABLE. Cross the sampled profile with the
 * span tree to split every span's wall time three ways:
 *
 *   js     the main thread was running JavaScript. Optimise the code, or cache
 *          its result, or move it off the critical path.
 *   gl     the main thread was blocked inside a WebGL call. Same options, but
 *          the work is the driver's.
 *   wait   the main thread was doing NOTHING — parked on an await, a rAF, or a
 *          compile-completion poll while the GPU worked in the background.
 *
 * A phase that is mostly `wait` cannot be made faster by making its code
 * faster. It is a SCHEDULING problem: that time is free main-thread capacity
 * the app could have spent showing the player something. Reading a boot profile
 * without this split is how a team spends a week micro-optimising a loop that
 * was never the bottleneck.
 *
 * Sample timestamps and span timestamps are both `performance.now()`, so they
 * are directly comparable — no clock alignment needed.
 */
function attributeSamples(samples, tree, origin) {
  if (!samples?.length) return null;

  // Sample timestamps and span starts are both on the page's performance.now()
  // clock, so they are directly comparable — no alignment step needed.
  const spans = [];
  const collect = (n) => {
    spans.push({ node: n, t0: origin + n.start, t1: origin + n.start + n.ms, js: 0, gc: 0 });
    for (const c of n.children ?? []) collect(c);
  };
  collect(tree);

  for (const s of samples) {
    const leaf = s.frames[0];
    // `(idle)` and `(not in JS)` are the thread doing nothing; everything else,
    // including `(program)` and `(garbage collector)`, is the thread pinned.
    const idle = leaf.name === '(idle)' || leaf.name === '(not in JS)';
    if (idle) continue;
    const gc = leaf.name === '(garbage collector)';
    for (const sp of spans) {
      if (s.tMs >= sp.t0 && s.tMs < sp.t1) {
        sp.js += s.dtMs;
        if (gc) sp.gc += s.dtMs;
      }
    }
  }
  const map = new Map();
  for (const s of spans) map.set(s.node, { js: s.js, gc: s.gc });
  return map;
}

/** Self / total / per-file rankings from the normalised sample list. */
function foldSamples(samples) {
  if (!samples?.length) return null;
  const self = new Map();
  const total = new Map();
  const byFile = new Map();
  let inJs = 0;
  for (const s of samples) {
    const leaf = s.frames[0];
    if (leaf.name === '(idle)' || leaf.name === '(not in JS)') continue;
    inJs += s.dtMs;
    const key = (f) => `${f.name}  ${C.dim(f.file)}`;
    self.set(key(leaf), (self.get(key(leaf)) ?? 0) + s.dtMs);
    byFile.set(leaf.file, (byFile.get(leaf.file) ?? 0) + s.dtMs);
    const seen = new Set();
    for (const f of s.frames) {
      const k = key(f);
      if (seen.has(k)) continue;
      seen.add(k);
      total.set(k, (total.get(k) ?? 0) + s.dtMs);
    }
  }
  const top = (m, n) => [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, n);
  return {
    sampleCount: samples.length,
    sampledMs: samples.length ? samples[samples.length - 1].tMs - samples[0].tMs : 0,
    inJsMs: inJs,
    topSelf: top(self, 25),
    topTotal: top(total, 20),
    topFiles: top(byFile, 15),
  };
}

/**
 * ms of this span that fell inside a long task.
 *
 * THIS IS THE CORRECTION THAT MAKES THE js/gl/idle SPLIT HONEST. The sampler
 * only produces a stack when the thread is executing JavaScript. A thread stuck
 * inside a native call — an unwrapped canvas op, an image decode, a driver call
 * we did not hook — yields samples with no stack, which naively look exactly
 * like an idle thread. They are the opposite: the thread is pinned and the page
 * is frozen.
 *
 * The long-task observer tells the two apart, because it fires on wall-clock
 * task duration and does not care what the task was doing. Time inside a long
 * task that the sampler could not attribute to JS is native work, not idle.
 */
function longTaskOverlapMs(span, longTasks, origin) {
  const t0 = origin + span.start;
  const t1 = t0 + span.ms;
  let acc = 0;
  for (const lt of longTasks) {
    const a = Math.max(t0, lt.start);
    const b = Math.min(t1, lt.start + lt.ms);
    if (b > a) acc += b - a;
  }
  return acc;
}

/** ms this span spent blocked inside a WebGL call, from its counter delta. */
const glBlockingMs = (gl) => {
  if (!gl) return 0;
  return (gl.shaderCompileMs ?? 0) + (gl.programLinkMs ?? 0) + (gl.linkStatusMs ?? 0) +
    (gl.completionMs ?? 0) + (gl.compileStatusMs ?? 0) + (gl.texUploadMs ?? 0) +
    (gl.bufferUploadMs ?? 0) + (gl.readPixelsMs ?? 0) + (gl.fenceWaitMs ?? 0) +
    (gl.finishMs ?? 0) + (gl.drawMs ?? 0) + (gl.programBindMs ?? 0) +
    (gl.programQueryMs ?? 0);
};


// -------------------------------------------------------------------- report --
function report(run) {
  const { profile, net, runtime, wallMs } = run;
  const pre = profile.origin; // navigation -> first line of app code
  const app = profile.totalMs; // app code -> __READY__
  const total = pre + app;

  console.log('');
  const REGIME = {
    icy: [C.r('ICY'), 'driver shader cache emptied — a first ever visit'],
    'cold-browser': [C.y('COLD BROWSER'), 'fresh browser profile, driver cache left hot'],
    warm: [C.g('WARM'), 'browser profile and driver cache both hot — a reload'],
  }[run.regime];
  console.log(
    C.b(`BOOT PROFILE  ${REGIME[0]}  ${W}x${H}@${DPR} · q=${runtime.quality}`) +
      C.dim(`
${REGIME[1]}`)
  );
  const soft = /swiftshader|llvmpipe|software|basic render/i.test(String(runtime.renderer ?? ''));
  console.log(
    C.dim(`GL: ${runtime.renderer ?? '?'}`) +
      C.dim(` · parallel shader compile: ${runtime.parallelCompile ? 'yes' : C.y('NO')}`)
  );
  if (soft) {
    console.log(
      C.r('  ! SOFTWARE RASTERIZER.') +
        ' Shader link and rasterisation are several times slower than on a real GPU,'
    );
    console.log(
      C.r('    ') +
        'and those are the two phases this profile is about. Re-run with --headed (or on'
    );
    console.log(
      C.r('    ') +
        'a machine with a working GPU) before trusting any absolute number here.'
    );
  }
  console.log(C.dim('─'.repeat(96)));
  const ms0 = profile.milestones ?? {};
  const firstFrame = ms0['first-frame'] !== undefined ? pre + ms0['first-frame'] : null;
  const loaded = ms0.loaded !== undefined ? pre + ms0.loaded : null;

  if (firstFrame !== null) {
    // The number progressive boot exists to move. Everything after it happens
    // with the game already on screen, so it is a different kind of cost.
    console.log(
      `${C.b('first painted frame')}   ${C.b(C.g(`${firstFrame.toFixed(0)}ms`))}` +
        C.dim('   the player can see and move')
    );
  }
  console.log(
    `${C.b('playable (__READY__)')}  ${C.b(`${total.toFixed(0)}ms`)}` +
      C.dim(`   (harness stopwatch ${wallMs}ms — includes browser navigation)`)
  );
  if (loaded !== null) {
    console.log(
      `${C.b('fully loaded')}          ${loaded.toFixed(0)}ms` +
        C.dim(`   +${(loaded - (firstFrame ?? total)).toFixed(0)}ms of streaming and pre-warm behind the game`)
    );
  }
  console.log('');

  // ---- phase split -------------------------------------------------------
  console.log(C.b('PHASES'));
  const phases = [
    [`module graph (${net.resourceCount} requests)`, pre,
      'fetch + evaluate every ES module. One bundle in a production build.'],
  ];
  for (const c of profile.tree.children ?? []) phases.push([c.name, c.ms, '']);
  const named = phases.reduce((a, [, v]) => a + v, 0);
  if (total - named > 1) phases.push(['(unattributed)', total - named, 'gaps between spans — idle, rAF waits, GC']);
  for (const [name, v, note] of phases.sort((a, b) => b[1] - a[1])) {
    const bar = '█'.repeat(Math.max(0, Math.round((v / total) * 40)));
    console.log(`  ${heat(v, total)(ms(v))} ${pct(v, total)}  ${bar.padEnd(40)}  ${name}`);
    if (note && v / total > 0.02) console.log(C.dim(`  ${' '.repeat(56)}${note}`));
  }
  console.log('');

  // ---- js / gl / wait split ----------------------------------------------
  const sampleList = run.cpuProfile
    ? normaliseCpuProfile(run.cpuProfile)
    : normaliseSelfProfile(run.samples);
  const jsBySpan = attributeSamples(sampleList, profile.tree, profile.origin);
  /** The four-way split of a span's wall time. See longTaskOverlapMs(). */
  const split = (node) => {
    const js = jsBySpan?.get(node)?.js ?? 0;
    const gl = glBlockingMs(node.gl);
    const busy = longTaskOverlapMs(node, profile.longTasks ?? [], profile.origin);
    // Anything the thread was pinned for that we could not name.
    const native = Math.max(0, Math.min(node.ms, busy) - js - gl);
    const idle = Math.max(0, node.ms - js - gl - native);
    return { js, gl, native, idle };
  };

  if (jsBySpan) {
    console.log(C.b('WHAT THE MAIN THREAD WAS DOING'));
    console.log(C.dim('  JS and native are work: make it cheaper, cache it, or move it off the critical path.'));
    console.log(C.dim('  IDLE is not work at all — it is main-thread capacity the app declined to use while'));
    console.log(C.dim('  the GPU compiled in the background. Idle time is a SCHEDULING bug, not a slow function.'));
    const rows = [];
    const collect = (n, depth) => {
      if (n.ms >= 120) rows.push([n, depth]);
      for (const c of n.children ?? []) collect(c, depth + 1);
    };
    for (const c of profile.tree.children ?? []) collect(c, 0);
    for (const [n, depth] of rows) {
      const s = split(n);
      const seg = (v, ch) => ch.repeat(Math.max(0, Math.round((v / total) * 44)));
      console.log(
        `  ${heat(n.ms, total)(ms(n.ms))}  ` +
          `${C.y(seg(s.js, '█'))}${C.c(seg(s.gl, '▓'))}${C.r(seg(s.native, '▒'))}${C.dim(seg(s.idle, '░'))}`.padEnd(60) +
          `  ${'  '.repeat(depth)}${n.name}  ` +
          C.dim(
            `js ${s.js.toFixed(0)} · gl ${s.gl.toFixed(0)} · native ${s.native.toFixed(0)} · ` +
              `${s.idle / n.ms > 0.5 ? C.b(`IDLE ${s.idle.toFixed(0)}ms (${((s.idle / n.ms) * 100).toFixed(0)}%)`) : `idle ${s.idle.toFixed(0)}`}`
          )
      );
    }
    console.log(
      C.dim('  ') + C.y('█ JS') + C.dim('  ') + C.c('▓ blocked in WebGL') + C.dim('  ') +
        C.r('▒ native (pinned, not JS)') + C.dim('  ░ idle')
    );
    const t = split(profile.tree);
    console.log(
      C.b('  totals: ') +
        `${t.js.toFixed(0)}ms JS · ${t.gl.toFixed(0)}ms WebGL · ${t.native.toFixed(0)}ms native · ` +
        C.b(`${t.idle.toFixed(0)}ms idle`) +
        C.dim(`  (${((t.idle / total) * 100).toFixed(0)}% of boot was an unoccupied main thread)`)
    );
    console.log('');
  }

  // ---- the tree ----------------------------------------------------------
  const flat = flatten(profile.tree);
  const MIN = Number(args.min ?? 15);
  console.log(C.b(`SPAN TREE`) + C.dim(`  (spans under ${MIN}ms hidden; self = wall minus children)`));
  const show = (node, depth) => {
    if (node.ms < MIN && depth > 0) return;
    const indent = '  '.repeat(depth);
    const selfTag = node.children?.length ? C.dim(` self ${node.selfMs.toFixed(0)}ms`) : '';
    const notes = node.notes
      ? C.dim(`  {${Object.entries(node.notes).map(([k, v]) => `${k}=${v}`).join(' ')}}`)
      : '';
    const js = jsBySpan?.get(node)?.js;
    const jsTag = js !== undefined && node.ms >= 50
      ? C.dim(`  js ${((js / node.ms) * 100).toFixed(0)}%`)
      : '';
    console.log(
      `  ${heat(node.ms, total)(ms(node.ms))} ${pct(node.ms, total)}  ${indent}${node.name}` +
        selfTag + jsTag + glSummary(node.gl) + notes
    );
    for (const c of node.children ?? []) show(c, depth + 1);
  };
  for (const c of profile.tree.children ?? []) show(c, 0);
  console.log('');

  // ---- flat ranking ------------------------------------------------------
  console.log(C.b('TOP COSTS BY SELF TIME') + C.dim('  (these sum to the whole; wall times do not)'));
  const ranked = flat.filter((n) => n.depth > 0).sort((a, b) => b.selfMs - a.selfMs).slice(0, 18);
  for (const n of ranked) {
    if (n.selfMs < 5) break;
    console.log(
      `  ${heat(n.selfMs, total)(ms(n.selfMs))} ${pct(n.selfMs, total)}  ${n.path.slice(1).join(' › ')}` +
        glSummary(n.gl)
    );
  }
  console.log('');

  // ---- GL totals ---------------------------------------------------------
  // A profile from an older build may predate a counter; default them all so
  // the report degrades instead of throwing.
  const c = { ...Object.fromEntries(Object.keys(GL_LABEL_DEFAULTS).map((k) => [k, 0])), ...profile.counters };
  console.log(C.b('GPU WORK DURING BOOT'));
  console.log(
    `  shaders compiled  ${String(c.shaderCompiles).padStart(6)}   ` +
      `${c.shaderCompileMs.toFixed(0)}ms in compileShader()`
  );
  console.log(
    `  programs linked   ${String(c.programLinks).padStart(6)}   ` +
      `${c.programLinkMs.toFixed(0)}ms in linkProgram() + ` +
      C.b(`${c.linkStatusMs.toFixed(0)}ms blocked on LINK_STATUS`) +
      C.dim(`  (${c.completionPolls} parallel-compile polls, ${c.completionMs.toFixed(0)}ms)`)
  );
  console.log(
    `  texture uploads   ${String(c.texUploads).padStart(6)}   ` +
      `${c.texUploadMs.toFixed(0)}ms, ${kb(c.texBytes)}`
  );
  console.log(
    `  buffer uploads    ${String(c.bufferUploads).padStart(6)}   ` +
      `${c.bufferUploadMs.toFixed(0)}ms, ${kb(c.bufferBytes)}`
  );
  console.log(
    `  draw calls        ${String(c.drawCalls).padStart(6)}   ` +
      `${c.drawMs.toFixed(0)}ms inside draw calls` +
      C.dim(`  (a deferred program link lands here, not in linkProgram)`)
  );
  console.log(
    `  program reflect   ${String(c.programQueries).padStart(6)}   ` +
      C.b(`${c.programQueryMs.toFixed(0)}ms`) +
      ` in getUniformLocation/getActiveUniform` +
      C.dim('  (where a deferred link really lands)')
  );
  console.log(
    `  useProgram        ${String(c.programBinds).padStart(6)}   ` +
      `${c.programBindMs.toFixed(0)}ms · readback ${c.readPixelsMs.toFixed(0)}ms · ` +
      `fence ${c.fenceWaitMs.toFixed(0)}ms · finish ${c.finishMs.toFixed(0)}ms`
  );
  const glBlocking = c.shaderCompileMs + c.programLinkMs + c.linkStatusMs + c.completionMs +
    c.compileStatusMs + c.texUploadMs + c.bufferUploadMs + c.readPixelsMs + c.fenceWaitMs +
    c.finishMs + c.drawMs + c.programBindMs + c.programQueryMs;
  console.log(
    C.b(`  ${glBlocking.toFixed(0)}ms`) + ` of boot was spent inside a blocking WebGL call ` +
      C.dim(`(${((glBlocking / total) * 100).toFixed(0)}% of total)`)
  );
  console.log(`  live programs at ready: ${runtime.programs}, geometries ${runtime.geometries}, textures ${runtime.textures}`);
  if (runtime.bakery) {
    const b = runtime.bakery;
    console.log(
      `  bakery: ${b.workers} workers · ${b.jobs} jobs ` +
        `(${b.onWorker} off-thread, ${b.onMainThread} local) · ` +
        `${b.workerMs.toFixed(0)}ms of worker CPU, ${b.mainMs.toFixed(0)}ms on the main thread`
    );
    for (const r of b.workerReady ?? []) {
      console.log(C.dim(`    ${r.id} ready at +${r.atMs}ms (clock offset ${r.offsetMs}ms)`));
    }
    for (const t of b.timeline ?? []) {
      console.log(
        C.dim(`    ${t.kind.padEnd(22)} ${String(t.worker).padEnd(10)} queued +${String(t.queuedAt).padStart(5)}ms · ` +
          `latency ${String(t.latencyMs).padStart(5)}ms · bake ${String(t.bakeMs).padStart(5)}ms · ` +
          `bytes ready +${String(t.readyAt).padStart(5)}ms · ` +
          `collected ${String(t.collectedAfterMs).padStart(5)}ms later`)
      );
    }
  }
  if (c.distinctPrograms) {
    const unused = (runtime.programs ?? 0) - c.distinctPrograms;
    console.log(`  programs ever BOUND during boot: ${c.distinctPrograms}`);
    if (unused > 0) {
      console.log(C.y(`  ${unused} compiled programs were never drawn with during boot.`));
      console.log(C.dim('    Pre-warm is speculative by design, so this is only waste if they are'));
      console.log(C.dim('    never bound in real gameplay either — check with tools/profile.mjs.'));
    }
  }
  console.log('');

  // ---- program population -----------------------------------------------
  if (run.programs) {
    // Group by shader identity. Most programs carry a material name; the ones
    // that do not are full-screen passes and internal materials, and lumping
    // every one of those into a single "(unnamed)" bucket would compare
    // unrelated shaders and report nonsense. Fall back to the cache key's first
    // token, which is three's shaderID / custom shader id.
    const groups = new Map();
    for (const p of run.programs) {
      // Unnamed programs key on BOTH custom shader ids: three writes
      // `[customVertexShaderID, customFragmentShaderID, ...]`, and the
      // full-screen passes all share one vertex shader — grouping on the first
      // token alone would call twenty different post passes permutations of
      // each other.
      const t = p.key.split(',');
      const id = p.name === '(unnamed)' ? `shaderID:${t[0]}/${t[1]}` : p.name;
      if (!groups.has(id)) groups.set(id, []);
      groups.get(id).push(p);
    }
    const permuted = [...groups.entries()]
      .map(([name, ps]) => ({ name, ps }))
      .sort((a, b) => b.ps.length - a.ps.length);
    const dup = permuted.filter((g) => g.ps.length > 1);
    const extra = dup.reduce((a, g) => a + g.ps.length - 1, 0);
    console.log(
      C.b('PROGRAM POPULATION') +
        C.dim(`  ${run.programs.length} programs from ${groups.size} distinct shaders — ` +
          `${extra} of them are permutations`)
    );
    console.log(C.dim('  a cold GPU driver spends most of the load linking these, so the count is the lever'));
    for (const g of dup.slice(0, Number(args.programs === true ? 8 : args.programs))) {
      const { varying, ragged } = analyseProgram(g.ps.map((p) => p.key));
      console.log(
        `  ${String(g.ps.length).padStart(4)}x  ${g.name.length > 46 ? `${g.name.slice(0, 46)}…` : g.name}`
      );
      for (const v of varying) {
        console.log(C.dim(`          ${v.name} = ${v.values.join(' | ')}`));
      }
      if (ragged) console.log(C.dim('          (customProgramCacheKey itself differs)'));
    }
    // The multiplier view: how many programs each varying field is responsible
    // for across the whole population, which is what to attack first.
    const axes = new Map();
    for (const g of dup) {
      for (const v of analyseProgram(g.ps.map((p) => p.key)).varying) {
        axes.set(v.name, (axes.get(v.name) ?? 0) + g.ps.length);
      }
    }
    const ranked = [...axes.entries()].sort((a, b) => b[1] - a[1]).slice(0, 8);
    if (ranked.length) {
      console.log(C.b('  permutation axes, by how many programs they touch:'));
      for (const [name, count] of ranked) console.log(`    ${String(count).padStart(4)}  ${name}`);
    }
    console.log('');
  }

  // ---- long tasks --------------------------------------------------------
  const lt = profile.longTasks ?? [];
  if (lt.length) {
    const ltMs = lt.reduce((a, t) => a + t.ms, 0);
    console.log(
      C.b('LONG TASKS') +
        C.dim(`  ${lt.length} tasks, ${ltMs.toFixed(0)}ms total (${((ltMs / total) * 100).toFixed(0)}% of boot) — the main thread was unresponsive`)
    );
    for (const t of [...lt].sort((a, b) => b.ms - a.ms).slice(0, 8)) {
      console.log(`  ${heat(t.ms, total)(ms(t.ms))}         at +${t.start.toFixed(0)}ms`);
    }
    console.log('');
  }

  // ---- module graph ------------------------------------------------------
  if (pre / total > 0.05) {
    console.log(C.b('MODULE GRAPH') + C.dim(`  ${net.resourceCount} requests, ${pre.toFixed(0)}ms before app code ran`));
    for (const [ext, v] of Object.entries(net.byExt).sort((a, b) => b[1].count - a[1].count).slice(0, 6)) {
      console.log(`  ${String(v.count).padStart(4)} .${ext.padEnd(6)} ${kb(v.bytes).padStart(8)}`);
    }
    console.log('');
  }

  // ---- sampled profile ---------------------------------------------------
  const folded = foldSamples(sampleList);
  if (folded) {
    console.log(
      C.b('SAMPLED JS PROFILE') +
        C.dim(`  ${folded.sampleCount} samples over ${folded.sampledMs.toFixed(0)}ms · ${folded.inJsMs.toFixed(0)}ms in JS`)
    );
    console.log(C.dim('  self time by function — attribution for work no span wraps'));
    for (const [k, v] of folded.topSelf) {
      if (v < 10) break;
      console.log(`  ${heat(v, total)(ms(v))} ${pct(v, total)}  ${k}`);
    }
    console.log(C.dim('  self time by file'));
    for (const [k, v] of folded.topFiles) {
      if (v < 20) break;
      console.log(`  ${heat(v, total)(ms(v))} ${pct(v, total)}  ${k}`);
    }
    console.log('');
  } else if (SAMPLES) {
    console.log(
      C.y('  sampled profile unavailable') +
        C.dim(`  ${profile.samplerError ?? 'no samples returned'} — the dev server must send Document-Policy: js-profiling`)
    );
    console.log('');
  }

  if (run.errors.length) {
    console.log(C.r('PAGE ERRORS'));
    for (const e of run.errors.slice(0, 8)) console.log(`  ${e}`);
    console.log('');
  }
}

function compare(runs, baseline) {
  const cur = summarise(runs);
  const base = summarise(baseline.runs ?? [baseline]);
  console.log(C.b('COMPARED TO BASELINE'));
  const row = (name, a, b) => {
    const d = a - b;
    const tag = Math.abs(d) < 1 ? C.dim('  same') : d < 0 ? C.g(`  ${d.toFixed(0)}ms faster`) : C.r(`  +${d.toFixed(0)}ms slower`);
    console.log(`  ${name.padEnd(28)} ${a.toFixed(0).padStart(7)}ms  vs ${b.toFixed(0).padStart(7)}ms${tag}`);
  };
  row('total to first frame', cur.totalMs, base.totalMs);
  row('module graph', cur.originMs, base.originMs);
  for (const k of new Set([...Object.keys(cur.phases), ...Object.keys(base.phases)])) {
    row(k, cur.phases[k] ?? 0, base.phases[k] ?? 0);
  }
  console.log('');
}

/** Median across repeats — one run of a cold boot is not a measurement. */
function summarise(runs) {
  const med = (xs) => {
    const s = [...xs].sort((a, b) => a - b);
    return s.length ? s[s.length >> 1] : 0;
  };
  const phases = {};
  for (const r of runs) {
    for (const c of r.profile.tree.children ?? []) (phases[c.name] ??= []).push(c.ms);
  }
  // Sub-phases too, so `init:world` can be compared without re-reading a tree.
  const sub = {};
  for (const r of runs) {
    const walk = (n) => {
      for (const c of n.children ?? []) {
        if (c.ms >= 20) (sub[c.name] ??= []).push(c.ms);
        walk(c);
      }
    };
    for (const c of r.profile.tree.children ?? []) walk(c);
  }
  const firstFrames = runs.map((r) => (r.profile.milestones?.['first-frame'] ?? 0) + r.profile.origin)
    .filter((x) => x > 0);

  return {
    runs: runs.length,
    firstFrameMs: firstFrames.length ? med(firstFrames) : null,
    subPhases: Object.fromEntries(Object.entries(sub).map(([k, v]) => [k, med(v)])),
    originMs: med(runs.map((r) => r.profile.origin)),
    appMs: med(runs.map((r) => r.profile.totalMs)),
    totalMs: med(runs.map((r) => r.profile.origin + r.profile.totalMs)),
    wallMs: med(runs.map((r) => r.wallMs)),
    phases: Object.fromEntries(Object.entries(phases).map(([k, v]) => [k, med(v)])),
    counters: runs[runs.length - 1].profile.counters,
  };
}


/**
 * Emit `src/core/bootweights.js` from this run's span tree.
 *
 * The loading bar is only honest if its weights come from a real measurement,
 * and re-measuring must be one command rather than a guess — otherwise the
 * table rots the first time a subsystem gets faster and the bar starts lying in
 * a new way. This closes that loop: the same instrument that measures the boot
 * writes the numbers the bar paces itself by.
 *
 * TWO LEVELS, deliberately. The top level is the phases that partition boot, so
 * their weights sum to the whole. The second is the children of the big phases,
 * used to move the bar smoothly THROUGH a phase rather than in one jump when it
 * ends — `init:world` is over a second, and a bar that sits still for a second
 * is the thing this exists to avoid.
 *
 * Weights are wall time, not self time: a phase's cost from the player's point
 * of view includes everything nested inside it.
 */
function emitWeights(run) {
  const tree = run.profile.tree;
  const kids = (n) => n.children ?? [];
  const byName = (n) => Object.fromEntries(kids(n).map((c) => [c.name, Math.round(c.ms)]));

  // Top level: the direct children of the root, plus engine.init's children in
  // place of engine.init itself — the subsystem inits are what a player is
  // actually waiting through, and naming them lets the bar say so.
  const top = {};
  for (const c of kids(tree)) {
    if (c.name === 'engine.init') {
      for (const sys of kids(c)) top[sys.name] = Math.round(sys.ms);
      // Whatever engine.init spent outside its subsystems.
      const inside = kids(c).reduce((a, x) => a + x.ms, 0);
      if (c.ms - inside > 20) top['engine.wiring'] = Math.round(c.ms - inside);
    } else if (c.ms >= 8 && !c.detached) {
      top[c.name] = Math.round(c.ms);
    }
  }
  // The first painted frame is a phase the player waits through like any other,
  // and on a cold driver it is not small.
  const firstFrame = run.profile.milestones?.['first-frame'];
  const accounted = Object.values(top).reduce((a, b) => a + b, 0);
  if (firstFrame && firstFrame - accounted > 20) {
    top['first-frame'] = Math.round(firstFrame - accounted);
  }

  // Second level, for the phases big enough that the bar would visibly stall.
  // Sub-phases, flattened to LEAVES.
  //
  // The bar can only move on a span boundary, so a container span that is
  // itself 600 ms of silence — `world:buildings`, which wraps eight buildings —
  // has to be descended into rather than reported as one step. Recursing to the
  // leaves gives one flat table per top phase and keeps BootProgress's
  // accounting to a single level.
  //
  // Never `boot-frames`: the spans nested under the first frames are streamed
  // work that merely happened to land there, not sub-phases of drawing a frame,
  // and pacing the bar by them would be pacing it by a coincidence.
  const leaves = (n, out = {}) => {
    for (const c of kids(n)) {
      if (c.ms >= 200 && kids(c).length >= 2) leaves(c, out);
      else out[c.name] = Math.round(c.ms);
    }
    return out;
  };
  const children = {};
  const walk = (n) => {
    for (const c of kids(n)) {
      const eligible = top[c.name] !== undefined && c.name !== 'boot-frames';
      if (eligible && c.ms >= 300 && kids(c).length >= 2) children[c.name] = leaves(c);
      walk(c);
    }
  };
  walk(tree);

  const totalMs = Object.values(top).reduce((a, b) => a + b, 0);
  const lines = [
    '/**',
    ' * BOOT WEIGHTS — generated, do not hand-edit.',
    ' *',
    ' * How long each boot phase takes, measured, so the loading bar can pace',
    ' * itself by real cost instead of counting steps. Regenerate after any change',
    ' * that moves boot around:',
    ' *',
    ' *   node tools/bootprofile.mjs --emit-weights',
    ' *',
    ' * Captured on: ' + (run.runtime.renderer ?? 'unknown GPU'),
    ' * Regime: ' + run.regime + ' (a reload; a first visit is several times this,',
    ' * which BootProgress discovers at runtime and re-prices — see bootprogress.js)',
    ' */',
    '',
    '/** Wall ms per phase. These partition the boot, so they sum to the total. */',
    'export const BOOT_WEIGHTS = ' + JSON.stringify(top, null, 2) + ';',
    '',
    '/** Sub-phases, used to move the bar THROUGH a long phase rather than at its end. */',
    'export const BOOT_CHILDREN = ' + JSON.stringify(children, null, 2) + ';',
    '',
    '/** Sum of BOOT_WEIGHTS, in reference-machine milliseconds. */',
    'export const BOOT_TOTAL_MS = ' + totalMs + ';',
    '',
  ];
  writeFileSync(resolve('src/core/bootweights.js'), lines.join('\n'));
  console.error(`wrote src/core/bootweights.js — ${Object.keys(top).length} phases, ${totalMs}ms total`);
}

// ---------------------------------------------------------------------- main --
const runs = [];
for (let i = 0; i < REPEAT; i++) {
  if (REPEAT > 1) process.stderr.write(C.dim(`run ${i + 1}/${REPEAT}...\n`));
  runs.push(await runOnce(i));
}

if (args['emit-weights']) emitWeights(runs[runs.length - 1]);

if (args.json) {
  const payload = { config: { warm: WARM, w: W, h: H, dpr: DPR, samples: SAMPLES, query: args.query ?? null },
                    summary: summarise(runs), runs: runs.map((r) => ({ ...r, consoleLines: undefined })) };
  const text = JSON.stringify(payload, null, 2);
  if (args.out) {
    writeFileSync(resolve(String(args.out)), text);
    console.error(`wrote ${args.out}`);
  } else {
    console.log(text);
  }
} else {
  report(runs[runs.length - 1]);
  if (REPEAT > 1) {
    const s = summarise(runs);
    console.log(
      C.b(`MEDIAN OF ${REPEAT} RUNS`) +
        `  first frame ${s.firstFrameMs ? s.firstFrameMs.toFixed(0) : '?'}ms · ` +
        `playable ${s.totalMs.toFixed(0)}ms  ` +
        C.dim(`(module graph ${s.originMs.toFixed(0)}ms)`)
    );
    // PER-PHASE medians, because that is what an A/B needs. One run on this
    // machine varies by more than most changes are worth, so a single tree
    // cannot tell you whether a phase got faster — only a median of the phase
    // itself can.
    const shown = { ...s.phases, ...(args.sub ? s.subPhases : {}) };
    for (const [name, ms] of Object.entries(shown).sort((a, b) => b[1] - a[1])) {
      if (ms < 20) continue;
      console.log(`  ${ms.toFixed(0).padStart(6)}ms  ${name}`);
    }
    console.log('');
  }
  if (args.out) {
    writeFileSync(resolve(String(args.out)), JSON.stringify({ summary: summarise(runs), runs }, null, 2));
    console.error(`wrote ${args.out}`);
  }
}

if (args.compare) {
  const p = resolve(String(args.compare));
  if (!existsSync(p)) throw new Error(`--compare: no such file ${p}`);
  compare(runs, JSON.parse(readFileSync(p, 'utf8')));
}
