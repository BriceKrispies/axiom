// FIRST import, deliberately: ES modules evaluate a module's imports in
// declaration order, so putting the profiler first makes its time origin the
// earliest point in this app's own evaluation. Everything before that origin
// (fetching and evaluating the module graph itself) is recovered by the tool
// from navigation timing — see tools/bootprofile.mjs.
import { boot } from './core/profile.js';

import { Engine } from './core/engine.js';
import { Bakery } from './core/bakery.js';
import { BootProgress } from './core/bootprogress.js';
import { BAKERS } from './bakers.js';
// `?worker` is Vite's explicit worker import. It has to be a static import at
// the construction site or no worker chunk is emitted — see the `makeWorker`
// note in src/core/bakery.js for the production build this silently broke.
import BakeryWorker from './bakers.worker.js?worker';
import { createConfig } from './core/config.js';

import { RenderSystem } from './render/index.js';
import { MaterialSystem } from './materials/index.js';
import { SkySystem } from './sky/index.js';
import { WorldSystem } from './world/index.js';
import { PhysicsSystem } from './physics/index.js';
import { PlayerSystem } from './player/index.js';
import { WeaponSystem } from './weapons/index.js';
import { FxSystem } from './fx/index.js';
import { AiSystem } from './ai/index.js';
import { UiSystem } from './ui/index.js';
import { AudioSystem } from './audio/index.js';

import { installShotApi } from './dev/shots.js';
import { prewarm, prewarmScene, prewarmRealScene } from './core/prewarm.js';
import { FidelityRamp } from './core/fidelityramp.js';

const params = new URLSearchParams(location.search);
const capture = params.get('capture') === '1';
// Deterministic shutter for the pixel gate: the engine does not schedule its own
// frames, the driver advances exactly N of them through window.__PUMP__. Opt-in,
// because tools that measure real frame pacing (tools/perf.mjs) need the loop to
// free-run. See the long comment in src/dev/shots.js.
const lockstep = capture && params.get('lockstep') === '1';

/**
 * PROGRESSIVE BOOT. Computed here, before the engine exists, because every
 * subsystem's `init()` needs to know it — see `progressiveBoot` in config.js.
 *
 * `?ramp=0` opts out; so does `?prewarm=0`, which opts out of every kind of
 * pre-compilation; and capture mode never uses it.
 */
const useRamp = !capture && params.get('prewarm') !== '0' && params.get('ramp') !== '0';

/**
 * When the surface bakes are allowed to start: as soon as the lighting has been
 * ISSUED to the driver (default), or not until it is READY.
 *
 * The difference is which of two numbers you would rather have: `issued` lets
 * the bakes overlap the lighting, so the level gains detail sooner and its
 * lighting lands later; `ready` gives the driver to the lighting alone.
 */
const bakeRelease = params.get('bakes') === 'ready' ? 'ready' : 'issued';

/** One of progressive boot's three holds, on unless `?hold-<name>=0`. */
const holdOf = (name) => useRamp && params.get(`hold-${name}`) !== '0';

const config = createConfig({
  quality: params.get('q') ?? 'ultra',
  deterministic: capture,
  progressiveBoot: useRamp,
  holdPost: holdOf('post'),
  holdSky: holdOf('sky'),
  holdBakes: holdOf('bakes'),
});

/**
 * THE LOADING BAR.
 *
 * Constructed before anything else this file does, and attached to the boot
 * profiler rather than to hand-placed calls: every phase worth showing a player
 * is already bracketed by a span, and the weight table it paces itself by is
 * generated from a profile of those same spans, so the two cannot drift apart.
 * See src/core/bootprogress.js.
 *
 * `window.__BOOT_UI__` is the inline overlay in index.html, which has been on
 * screen since before this bundle finished downloading.
 */
const ui = typeof window !== 'undefined' ? window.__BOOT_UI__ : null;
const progress = new BootProgress({
  onChange: (frac, label) => ui?.set(frac, label),
});
progress.attach(boot);

boot.mark('module-eval-done', {
  // How long the browser spent fetching + evaluating the module graph before
  // this line. In dev that is ~144 unbundled HTTP requests; in a production
  // build it is one bundle. The gap between the two is real boot time.
  sinceNavigationMs: Math.round(performance.now()),
});

const canvas = document.getElementById('game');

// The worker pool for procedural texture generation. Constructed FIRST and
// before the engine, because spawning workers costs a few milliseconds here and
// buys a head start on ~4 s of value-noise evaluation that would otherwise sit
// on the critical path (see src/core/bakery.js and tools/bootprofile.mjs).
// `?bake=0` forces every bake back onto the main thread — the comparison that
// proves the worker path is worth having, and the escape hatch if it is not.
const bakery = boot.time('bakery.spawn', () => new Bakery({
  makeWorker: () => new BakeryWorker(),
  bakers: BAKERS,
  enabled: params.get('bake') !== '0',
}));
boot.note('workers', bakery.size);

// Wait for the pool to finish loading before anything blocks the main thread.
// A worker's module script is fetched through the parent document's loader, so
// a worker cannot finish starting while this thread is inside a long task —
// see Bakery.ready() for the measurement that made this necessary.
await boot.timeAsync('bakery.ready', () => bakery.ready());

const engine = boot.time('engine.construct', () => new Engine({ canvas, config, bakery }));

// Registration order is irrelevant — Registry topo-sorts on static deps.
engine
  .add(RenderSystem)
  .add(MaterialSystem)
  .add(SkySystem)
  .add(WorldSystem)
  .add(PhysicsSystem)
  .add(PlayerSystem)
  .add(WeaponSystem)
  .add(FxSystem)
  .add(AiSystem)
  .add(UiSystem)
  .add(AudioSystem);

try {
  await boot.timeAsync('engine.init', () => engine.init());
} catch (err) {
  console.error('[boot] init failed', err);
  ui?.fail('boot failed — see console');
  document.body.insertAdjacentHTML(
    'beforeend',
    `<pre style="position:fixed;inset:0;padding:2rem;color:#f66;background:#000;
       font:12px/1.5 ui-monospace,monospace;overflow:auto;z-index:9999;white-space:pre-wrap">
BOOT FAILURE\n\n${err.stack ?? err.message}</pre>`
  );
  throw err;
}

const shotApi = installShotApi(engine, { capture, lockstep });

/**
 * PROGRESSIVE BOOT — the frame loop starts before boot has finished.
 *
 * `engine.init()` now only builds what the first frame genuinely needs: the
 * renderer, the sky, the level and its collision, the player and the HUD. The
 * rest — the two weapon viewmodels nobody is holding, the navigation grid and
 * garrison for enemies that have not engaged, and the shader pre-warm — is
 * declared by each subsystem as a `stream()` generator and drained a few
 * milliseconds per frame by `Engine.step()`, with the game already on screen.
 * See src/core/streaming.js.
 *
 * Pre-warm runs alongside it rather than as stream chunks, because it is mostly
 * an idle main thread waiting on the GPU to link programs in the background —
 * work the frame loop can absorb for free. It is only safe to overlap because
 * it no longer moves the camera; see the note where WARM_POSES used to be.
 *
 * CAPTURE MODE DOES NONE OF THIS. A screenshot of a half-streamed world is not
 * a regression, it is a different picture, and the pixel gate cannot tell the
 * two apart. So `?capture=1` drains every generator and awaits pre-warm before
 * raising `__READY__`, exactly as the old inline boot did — which is why the
 * gate still means something.
 */
const startPrewarm = () =>
  params.get('prewarm') === '0'
    ? Promise.resolve(boot.mark('prewarm:skipped') ?? { ok: false, reason: 'disabled by ?prewarm=0' })
    // Detached: in the live path this runs alongside the frame loop, so it is
    // not a phase the sequential boot spine contains. See profile.js.
    : boot.timeAsync('prewarm', () => prewarm(engine), { detached: !capture });

// LINK THE FIRST FRAME'S PROGRAMS BEFORE DRAWING IT — and before anything
// else compiles, because this is also what settles the visible light count that
// every program's cache key carries.
//
// Not an optimisation of the warm case — it costs the warm case a little — but
// the entire cold one. A driver with an empty shader cache defers each program
// link until the first draw asks for that program's uniform locations, and the
// first frame then pays all of them serially on the main thread: measured at
// 14.4 s on a first-ever visit. `compileAsync` hands the same links to the
// driver's own threads and polls for completion, so they happen in parallel and
// off this thread, and the first frame's reflection is free.
//
// `?prewarm=0` opts out of every kind of pre-compilation, this included.
//
// THE RAMP. `?ramp=0` opts out; capture mode never uses it (a screenshot of a
// flat-lit level is a different picture, and the pixel gate cannot tell that
// from a regression). See src/core/fidelityramp.js for the measurement that
// motivates it — the driver compiles serially, so the 27 scene programs are a
// 10.4 s sum that no scheduling can shrink.
//
// With the ramp on, this phase compiles a handful of unlit stand-ins instead,
// the frame loop starts, and the real programs are linked behind a level the
// player can already see and move through.
/** Resolve once the engine has drawn `n` more frames. */
const afterFrames = (n) =>
  new Promise((resolve) => {
    const target = engine.time.frame + n;
    const tick = () => (engine.time.frame >= target ? resolve() : requestAnimationFrame(tick));
    requestAnimationFrame(tick);
  });
const ramp = useRamp ? new FidelityRamp() : null;
const rampPrograms = ramp ? boot.time('prewarm.ramp:engage', () => ramp.engage(engine.scene, engine.viewScene)) : 0;
if (ramp) console.info(`[boot] fidelity ramp: ${rampPrograms} stand-in programs`);

const sceneWarm =
  params.get('prewarm') === '0'
    ? { ok: false, reason: 'disabled by ?prewarm=0' }
    : await boot.timeAsync('prewarm.scene', () =>
        prewarmScene(engine, {
          // The only phase whose progress is exactly knowable — a count of
          // finished program links — and the one that dominates a first visit.
          // Also where the bar learns what this machine's shader compilation
          // really costs: the reference weight is from a warm run, a cold one is
          // an order of magnitude more, and a handful of completed links is
          // enough to re-price the rest so the bar does not reach 90% and stop.
          onProgress: (frac, done, total, elapsedMs) => {
            progress.advance(frac);
            if (done >= 6 && total > 0) {
              progress.reprice('prewarm.scene', (elapsedMs / done) * total);
            }
          },
        })
      );
console.info('[boot] prewarm.scene', sceneWarm);
window.__PREWARM_SCENE__ = sceneWarm;

// THE REAL MATERIALS, COMPILED BEHIND THE GAME.
//
// Detached on purpose: this is not a phase the sequential boot spine contains,
// it runs alongside the frame loop. The poll inside compileWithProgress yields
// to the event loop between checks, so the driver grinds through the 27
// programs on its own thread while the player is moving around a level that is
// already on screen. When they land, the real materials go back in one swap —
// no per-frame popping, one visible transition from flat to lit.
//
// AFTER THE FIRST PAINTED FRAME, AND NOT ONE MOMENT EARLIER.
//
// `renderer.compile()` is synchronous: it assembles and issues every link in
// one call, and the driver then works through them serially. Started before the
// frame loop, that is 69 links queued in front of the first frame, and the
// first measurement of exactly that mistake was 25.4 s to first paint — WORSE
// than the 16.2 s it was meant to fix, because the ramp had bought a cheap
// first frame and then put the whole expensive compile back in front of it.
//
// So it waits for the `first-frame` milestone. By then the level is on screen,
// the loading screen is down, and the driver can take as long as it likes.
const startRampCompile = () =>
  boot.timeAsync('prewarm.ramp:real', async () => {
    // Say what is happening. The loading screen came down on the first painted
    // frame, so without this the player watches a flat-lit level for several
    // seconds with nothing to say it is still arriving — which reads as the
    // final image, and a broken one.
    // ---- 1. the post chain ------------------------------------------------
    //
    // First, because it is what the FRAME LOOP needs. Everything else here only
    // makes the picture better; until the chain is in, the loop is drawing a
    // pipeline with holes in it.
    //
    // It also has to be completely finished — issued, linked, AND drawn once —
    // before the next tier is handed over. Drawing a program is where the
    // renderer reflects it, reflection blocks until the driver's whole queue
    // drains, and the next tier is exactly what would be in that queue. Sending
    // both at once is the 6 023 ms stall this boot used to have.
    ui?.tail('effects');
    const render = engine.ctx.peek('render');
    const post = await render.warmPostChain();
    render.setPostChainEnabled(true);
    await afterFrames(2);
    boot.milestone('post-chain');
    console.info('[boot] post chain in', post);

    // ---- 2. the lighting --------------------------------------------------
    ui?.tail('lighting');
    // The IBL first: the real materials sample it, and it is one 43 KB shader —
    // 1 747 ms cold — that would otherwise be drawn for the first time on the
    // frame after the swap, freezing a game the player is already playing.
    await engine.ctx.peek('sky')?.releaseEnv();

    // ---- 3. the surface detail --------------------------------------------
    //
    // Released the moment the lighting has been ISSUED, not when it is ready.
    // The driver compiles in submission order, so that is already enough to put
    // 14.2 s of bake shaders behind 1.4 s of lit materials — and holding them
    // any longer than that is actively harmful: the level renders from unpainted
    // textures for as long as the hold lasts, and at 16 s of it the result does
    // not read as "less detail", it reads as broken.
    const real = await prewarmRealScene(engine, ramp, {
      mode: params.get('lighting') === 'block' ? 'block' : 'poll',
      onIssued: () => {
        // The instant the lighting is in the driver's queue. `lit` minus this is
        // how long the driver actually took for it — which is the only way to
        // tell "the lighting is slow" from "the lighting is queued behind
        // something else".
        boot.milestone('lighting-issued');
        bakeRelease === 'issued' && engine.ctx.peek('materials')?.holdBakes(false);
      },
    });
    bakeRelease === 'ready' && engine.ctx.peek('materials')?.holdBakes(false);
    const restored = ramp.release();
    // WHEN THE LEVEL STOPS BEING FLAT. Recorded as a milestone rather than left
    // to the span tree, because the span tree is closed by then: __READY__ now
    // lands before this phase begins, and `begin()` after `finish()` hands back
    // an orphan on purpose. Milestones outlive finish() for exactly this case.
    boot.milestone('lit');
    console.info(`[boot] fidelity ramp released: ${restored} meshes`, real);
    ui?.tail(engine.streamer.done ? null : 'streaming');
    return real;
  }, { detached: true });

/**
 * HOW MANY FRAMES TO LET RUN BEFORE QUEUEING THE REAL MATERIALS.
 *
 * One is not enough, and the reason is the single most expensive thing in this
 * app's cold boot. `renderer.compile()` hands the driver a batch of links and
 * returns; the driver then works through them SERIALLY on its own thread, which
 * is fine — nothing is blocked while it does. But the moment anything on the
 * main thread asks the driver for a program's interface
 * (`getUniformLocation`), that call blocks until the driver has drained the
 * whole queue. It is charged to whichever program happened to ask.
 *
 * The frame loop does exactly that on frame 2, because SSR is skipped on frame
 * 1 (it reprojects through the previous frame, and there is not one yet) and so
 * its program is created one frame late — straight into the queue this compile
 * just filled. MEASURED: `ow-ssr` billed 6 023 ms that way. Its true cost, asked
 * for with an empty queue, is 108 ms. At `?q=medium`, where SSR is off, the same
 * 5 205 ms landed on `csm-depth` instead, at the same instant — it was never
 * about which shader, only about who asked first.
 *
 * THE FIX IS NOT TO GUESS A FRAME NUMBER. An earlier version of this waited
 * five frames, on the reasoning that SSR was the last pass to create a program;
 * it worked, and it was a guess that any new pass could invalidate silently.
 * The real answer is that the frame loop must not be creating programs at all
 * while a batch is in flight — which is what gating the post chain achieves.
 * With the chain out of the frame, frames 2..N create nothing, so the handover
 * can happen as soon as there is a first frame to hand over from, and each tier
 * below waits for the one above to be not just linked but DRAWN.
 */
const rampDone = ramp
  ? new Promise((resolve) => {
      const afterFirstFrame = () => {
        if (boot.milestones['first-frame'] === undefined) {
          requestAnimationFrame(afterFirstFrame);
          return;
        }
        resolve(startRampCompile());
      };
      requestAnimationFrame(afterFirstFrame);
    })
  : Promise.resolve(null);
window.__RAMP__ = rampDone;

if (capture) {
  // No loading screen in a capture: the harness photographs the canvas the
  // instant __READY__ goes up, and an overlay — even a fading one — would be in
  // every reference image.
  ui?.hideNow();
  await engine.drainStream();
  window.__PREWARM__ = await startPrewarm();
  window.__LOADED__ = true;
  console.info('[boot] prewarm', window.__PREWARM__);
} else {
  // ORDER MATTERS HERE, and it is the whole point of progressive boot.
  //
  // Streaming goes first: the weapon in the player's hands and the enemies they
  // will meet. Pre-warm goes last, once those have landed, because it is ~1.7 s
  // of main-thread program creation — in front of the first frame that is 1.7 s
  // of black screen; behind it, it is a few hitches while the player is already
  // playing a level that is already on screen.
  //
  // Nothing here is awaited. The next statement starts the loop.
  engine.events.on('stream:done', (stats) => {
    console.info(
      `[boot] streamed ${stats.chunks} chunks in ${stats.totalMs.toFixed(0)}ms ` +
        `(worst chunk ${stats.worstChunkMs.toFixed(0)}ms: ${stats.worstChunk})`
    );
    ui?.tail('compiling shaders');
    startPrewarm().then((w) => {
      window.__PREWARM__ = w;
      console.info('[boot] prewarm', w);
      // "Fully loaded" is a separate event from "playable" now. Tools that need
      // a settled world — the gameplay profiler, anything measuring steady
      // state — wait for this rather than for __READY__.
      boot.milestone('loaded');
      window.__LOADED__ = true;
      ui?.loaded();
    });
  });
}

boot.note('programs', engine.ctx.peek('render')?.renderer?.info?.programs?.length ?? 0);
console.info('[boot] bakery', bakery.stats, `${bakery.size} workers`);

engine.start();

/**
 * The overlay comes down on the FIRST PAINTED FRAME, not on "fully loaded".
 *
 * That is the whole point of progressive boot: the game is playable while the
 * weapons, the navigation grid and the rest of the pre-warm stream in behind it.
 * Holding a loading screen over a running game to wait for work the player
 * cannot see would be inventing a wait that no longer exists. What is left goes
 * to a corner indicator instead.
 */
if (!capture) {
  const handOver = () => {
    if (boot.milestones['first-frame'] === undefined) {
      requestAnimationFrame(handOver);
      return;
    }
    progress.finish();
    ui?.done(engine.streamer.done ? null : 'streaming');
    // The overlay is a full-screen div with default pointer-events until `.gone`,
    // so until this line a click cannot reach the canvas and mouse look cannot
    // be acquired. Keyboard was live earlier (window listeners); the mouse was
    // not, and a shooter you cannot aim is not one you can play.
    boot.milestone('pointer-ready');
  };
  requestAnimationFrame(handOver);
}

// Capture harness handshake: only flag ready once a frame has actually landed.
//
// BOOT_FRAMES is deliberately a frame COUNT, not a rAF race. In lockstep mode the
// engine has no loop of its own, so we hand-pump exactly this many frames and only
// then raise __READY__; the shot is therefore always applied at engine frame 3, no
// matter how long boot (or pre-warm) took in wall-clock terms.
const BOOT_FRAMES = 3;
// The profiler closes on the same event the harness calls ready, so
// `__BOOTPROFILE__.totalMs` and the harness's own boot stopwatch measure the
// same interval and can be compared without an off-by-one-frame argument.
//
// In the live path this is now the time to the first PLAYABLE frame, not to a
// finished load — which is the number that was worth optimising all along.
const bootFramesSpan = boot.begin('boot-frames');
if (lockstep) {
  await shotApi.pump(BOOT_FRAMES);
  boot.end(bootFramesSpan);
  boot.finish('lockstep');
  window.__READY__ = true;
} else {
  let warm = 0;
  const readyProbe = () => {
    if (++warm >= BOOT_FRAMES) {
      boot.end(bootFramesSpan);
      boot.finish('ready');
      window.__READY__ = true;
      return;
    }
    requestAnimationFrame(readyProbe);
  };
  requestAnimationFrame(readyProbe);
}

window.__ENGINE__ = engine;

if (import.meta.hot) {
  import.meta.hot.dispose(() => engine.dispose());
}
