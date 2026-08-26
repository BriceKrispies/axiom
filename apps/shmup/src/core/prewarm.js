/**
 * Shader pre-warm.
 *
 * WHY THIS EXISTS — measured, not guessed. Profiling actual gameplay at Retina
 * DPR showed 86 WebGL programs compiling lazily *during play*, with up to 30
 * landing on a single frame. Each of those frames took 3.1-3.9 SECONDS. That is
 * the "freezing" players report: not a low frame rate, but multi-second stalls
 * whenever geometry with an uncompiled material/light/shadow permutation first
 * enters the frame.
 *
 * Three.js compiles a program the first time a given (material, lights, shadow,
 * skinning, fog, ...) permutation is actually drawn. The fix is to force every
 * permutation to compile up front, while a loading state is on screen, so the
 * steady-state frame loop never compiles anything.
 *
 * This must not change a single rendered pixel. It only moves *when* compilation
 * happens, so it touches no material parameters, no camera, no lighting state.
 * The pixel-diff gate (tools/imagediff.mjs) enforces that.
 *
 * Two mechanisms, because neither alone is sufficient:
 *
 *  1. renderer.compileAsync() — uses KHR_parallel_shader_compile where available,
 *     so it compiles off the main thread and does not block. Covers the forward
 *     lit pass for everything currently in a scene graph.
 *  2. Real frames from representative poses — compileAsync does NOT cover the
 *     depth/shadow-map variant of a material, nor the post-processing chain,
 *     nor permutations that only exist once a subsystem has spawned its transient
 *     objects (particles, decals, ragdolls, muzzle flash). Actually drawing a
 *     handful of frames is the only way to reach those.
 */

/**
 * THERE ARE NO WARM-UP POSES ANY MORE, and the reason is worth keeping.
 *
 * This used to walk the camera through four poses spanning the level's lighting
 * and material variety, compiling at each. It was necessary for one reason
 * only: the visible LIGHT COUNT is part of three's program cache key, the count
 * depends on which lights survive the camera's distance cull, and so a pose was
 * a proxy for a light count. It was never about what the camera could see —
 * `compileAsync` walks the whole scene graph and ignores the frustum entirely.
 *
 * Now that `render.settleLights()` pins the count before anything compiles, the
 * poses compile nothing the first call did not. Measured: 108 programs with the
 * four poses, 108 without.
 *
 * Deleting them is what lets pre-warm run WHILE THE GAME IS ON SCREEN. A
 * pre-warm that moves the camera cannot overlap a live frame loop without the
 * player seeing the level flick through four viewpoints; one that never touches
 * the camera can. That is the whole progressive-boot path in src/main.js.
 */

/**
 * Force every shader permutation to compile before gameplay starts.
 * Resolves once warm. Never throws — a failed pre-warm must not block boot,
 * it just means the old stutter comes back.
 */
/**
 * @param opts.transients  Stage each subsystem's spawned objects (enemies, impact
 *   bursts, muzzle flash) so their programs compile too. MEASURED TO BE UNSAFE and
 *   therefore off by default: the pixel-diff gate showed up-to-254/255 channel
 *   deltas afterwards, because decals live in a persistent ring buffer and spawned
 *   actors are not despawned by any hook reachable from here. Reaching the
 *   remaining permutations safely needs a `prewarmMaterials()` on each subsystem
 *   that builds and compiles its materials WITHOUT spawning gameplay objects —
 *   which is owned by those subsystems, not by core.
 */
import * as THREE from 'three';
import { boot } from './profile.js';

/**
 * Subsystems whose `prewarmMaterials()` must NOT be driven from here.
 *
 * `fx` self-schedules its own pre-warm on the second rendered frame, and that is
 * not a workaround it can drop: the program cache key carries the number of
 * VISIBLE lights, and the visible set is only settled inside the renderer's
 * first frame (`render._cullLights`) plus `world._stabiliseLightCount`, both of
 * which run after this function has returned. Calling fx from here would compile
 * a permutation the frame loop never asks for AND latch fx's `_warmed` flag, so
 * the real programs would go back to compiling on the first shot fired. Measured
 * by src/fx: that is 12 programs / 142-159 ms on the frame the trigger is pulled.
 */
const SELF_WARMING = new Set(['fx']);

/**
 * Whether to let `render.prewarmMaterials()` run its CSM-depth + MRT-prepass step.
 *
 * OFF, and it is the one thing in this file that was MEASURED not to be
 * pixel-neutral. Unlike every other step here, that one does not compile — it
 * actually *runs* the two depth passes, writing the shadow array and the gbuffer.
 * `render` reports it as clean when invoked standalone at frame 0; driven from
 * here (after every subsystem has init'd, with the camera restored to the real
 * spawn pose) it is not. Bisected against shots/perf-base with everything else in
 * place, one variable at a time:
 *
 *   render-only tree, no hooks .................. identical, 0 px
 *   + ragdoll sleep skip ........................ identical, 0 px
 *   + all hooks, shadow:false ................... identical, 0 px
 *   + all hooks, shadow:true .... detail/impacts/muzzle/night/weapon changed,
 *                                 0.005-0.017% of pixels, maxDelta 1
 *
 * Run-to-run noise was verified at exactly zero first (two captures of the same
 * tree were bit-identical), so those deltas are the change, not the harness.
 *
 * Little is lost: the override-material variants are reached anyway, without
 * drawing, by `world.prewarmMaterials()` (which compiles the level under
 * `csm.depthMaterial` and `gbuffer.material` via `scene.overrideMaterial`) and by
 * `ai.prewarmMaterials()` (which borrows render's depth override for the
 * characters). The gate outranks the last few programs.
 */
const RENDER_SHADOW_WARM = false;

/**
 * PHASE ONE — link the programs the FIRST FRAME will need, in parallel.
 *
 * This is the whole cold-boot story, and it is not about compiling early for
 * its own sake. A GPU driver is free to defer a program link past
 * `linkProgram` and past `LINK_STATUS`; NVIDIA defers it until something asks
 * for the program's interface, which is the uniform and attribute reflection
 * three does the first time it draws with a program. Those queries are
 * synchronous round trips, so on a driver with an empty shader cache the first
 * frame pays every link SERIALLY, on the main thread. Measured on a first-ever
 * visit: 7054 reflection queries, 14 435 ms, 88% of the entire boot.
 *
 * `compileAsync` turns that inside out. With KHR_parallel_shader_compile the
 * links run on the driver's own threads and three polls a completion flag, so
 * the same work happens off the main thread and in parallel with itself. By the
 * time the first frame draws, the reflection it does is free — the links are
 * already finished.
 *
 * It is deliberately only the scene compile, not the whole pre-warm: the hooks
 * (render's post chain, world's depth variants, ai's characters) reach
 * permutations the first frame never draws, so they stay deferred to
 * `prewarm()` behind the streaming. This phase is the part the first frame
 * cannot do without.
 *
 * A RENDER TARGET IS BOUND while compiling — three folds the bound target's
 * colour space and tone mapping into the program cache key, and the world and
 * viewmodel are both drawn into HDR targets. Compiling against the canvas
 * produces the `srgb` variants, which the frame loop then never uses.
 */

/**
 * `renderer.compileAsync()`, reimplemented so it can report progress.
 *
 * Identical in behaviour: compile the scene, then poll each material's program
 * until every one reports ready. The only addition is that the caller is told
 * how many are done, which is what lets a loading bar move truthfully through
 * the longest phase of a cold boot instead of sitting at one number for seven
 * seconds.
 *
 * `onProgress(fraction, done, total)` is also where the phase gets RE-PRICED:
 * the reference weight for this phase comes from a warm machine, and on a first
 * visit it is an order of magnitude larger. A few completed links are enough to
 * measure the real per-program cost, so the bar can re-pace the rest of itself
 * around the truth rather than reaching 90% and waiting.
 *
 * Falls back to the stock call where KHR_parallel_shader_compile is missing:
 * without it `isReady()` cannot answer without blocking, so there is no
 * progress to report and no point pretending otherwise.
 */
async function compileWithProgress(renderer, scenes, onProgress) {
  const parallel = renderer.getContext().getExtension('KHR_parallel_shader_compile');
  if (!parallel || !renderer.properties) {
    // eslint-disable-next-line no-restricted-syntax -- fallback path, see catch below
    for (const { scene, camera } of scenes) await renderer.compileAsync(scene, camera);
    onProgress?.(1, 0, 0);
    return;
  }

  // ISSUE EVERY LINK BEFORE WAITING ON ANY OF THEM.
  //
  // This used to be called once per scene and awaited in sequence, so the
  // viewmodel's programs were not handed to the driver until the last world
  // program had finished linking. That is the one thing a parallel-compile path
  // must not do: the driver has idle threads for the whole world phase and
  // nothing queued for them. `compile()` is synchronous and only creates and
  // links, so calling it for both scenes first costs nothing and lets the whole
  // set overlap.
  const pending = new Set();
  scenes.forEach(({ scene, camera }) => {
    renderer.compile(scene, camera).forEach((m) => pending.add(m));
  });
  const total = pending.size;
  if (total === 0) {
    onProgress?.(1, 0, 0);
    return;
  }

  // THE COMPLETION CURVE. `done/total` over time says whether the driver is
  // really compiling in parallel or feeding one thread: links that land in a
  // burst mean parallelism, links spaced evenly across the phase mean the wall
  // time is a sum and the only lever is fewer or cheaper programs. Recorded
  // rather than reasoned about — 27 programs in 11 s could be either.
  const curve = [];
  boot.compileCurve = curve;

  const started = performance.now();
  for (;;) {
    for (const material of [...pending]) {
      const program = renderer.properties.get(material)?.currentProgram;
      // A material with no program cannot be waited on; treat it as done rather
      // than looping forever on it.
      if (!program || program.isReady()) pending.delete(material);
    }
    const done = total - pending.size;
    const at = performance.now() - started;
    curve.length && curve[curve.length - 1].done === done ? null : curve.push({ ms: Math.round(at), done });
    onProgress?.(done / total, done, total, at);
    if (pending.size === 0) return;
    await new Promise((r) => setTimeout(r, 10));
  }
}

/**
 * Link the real scene programs while the FIDELITY RAMP is holding the screen.
 *
 * The ramp has replaced every scene material with an unlit stand-in so the
 * first frames could be drawn cheaply (see core/fidelityramp.js). The real
 * materials are therefore NOT in the scene, and `renderer.compile()` compiles
 * what it finds — so this hands them back for exactly the duration of the
 * `compile()` call, inside one synchronous window, and takes them away again
 * before the next frame renders.
 *
 * From then on it is the ordinary poll: the driver grinds through the links on
 * its own thread while the game runs at full rate in front of it. Only when
 * every program answers `isReady()` does the caller swap the real materials in
 * for good, so the frame that shows them never waits on a link.
 *
 * @param {*} engine
 * @param {{withRealMaterials: (fn: () => any) => any}} ramp
 */
/**
 * @param opts.onIssued Called the instant the real materials' links have been
 *   HANDED to the driver, which is long before they are ready. That distinction
 *   is the whole point of the hook: the driver compiles serially in submission
 *   order, so anything released here queues BEHIND the lighting without having
 *   to wait for it. Waiting for readiness instead left the surface bakes held
 *   for the full 16 s the lighting took, and a level rendering that long from
 *   unpainted textures does not look like a level with less detail — it looks
 *   broken.
 */
export async function prewarmRealScene(engine, ramp, { onIssued = null, mode = 'poll', chunk = 6 } = {}) {
  const render = engine.ctx.peek('render');
  const renderer = render?.renderer;
  if (!renderer?.properties) return { ok: false, reason: 'no renderer' };

  const t0 = performance.now();
  const before = renderer.info.programs?.length ?? 0;
  const scratchRt = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false, stencilBuffer: false });

  /**
   * A FEW MESHES AT A TIME, WITH A FRAME BETWEEN EACH.
   *
   * `renderer.compile()` is synchronous, and over all 169 meshes at once it is
   * 7256 ms of it — MEASURED, and measured as a 6720 ms hole in the frame
   * record at the moment it runs. The player has had control since 2.7 s by
   * then, so that is not a loading pause, it is the game freezing mid-play.
   *
   * Chunking does not make the work smaller. It makes it interruptible: the
   * same translation and linking happens, spread over as many frames as there
   * are chunks, and the loop keeps drawing between them.
   *
   * Each chunk is still ONE synchronous window — restore, compile, stand back
   * in — because a frame that renders with a real material whose program is not
   * linked is the stall this whole path exists to avoid. The await is strictly
   * between chunks.
   */
  const meshes = ramp.meshes ?? [];
  const chunkSize = Math.max(1, chunk);
  const pending = new Set();
  const compileChunk = (subset) => {
    const prevRt = renderer.getRenderTarget();
    renderer.setRenderTarget(scratchRt);
    try {
      ramp.withRealMaterials(() => {
        [
          { scene: engine.scene, camera: engine.camera },
          { scene: engine.viewScene, camera: engine.viewCamera },
        ].forEach(({ scene, camera }) => {
          renderer.compile(scene, camera).forEach((m) => pending.add(m));
        });
      }, subset);
    } finally {
      renderer.setRenderTarget(prevRt);
    }
  };

  const chunkMs = [];
  for (let i = 0; i < meshes.length; i += chunkSize) {
    const at = performance.now();
    compileChunk(meshes.slice(i, i + chunkSize));
    chunkMs.push(Math.round(performance.now() - at));
    // Let the game have a frame. rAF rather than setTimeout so this paces off
    // presentation rather than off a timer the compile has already blown past.
    await new Promise((r) => requestAnimationFrame(r));
  }
  // The viewmodel scene has meshes the ramp never engaged; one final pass with
  // no subset picks up anything left, and costs nothing if there is nothing.
  compileChunk([]);
  scratchRt.dispose();
  try { window.__RAMPCHUNKS__ = chunkMs; } catch { /* non-browser */ }
  onIssued?.();

  // FORCE THE DRIVER TO FINISH NOW, at the cost of freezing the frame loop.
  //
  // Asking a program for its uniform locations blocks the main thread until the
  // driver has drained its queue — which is exactly the stall progressive boot
  // exists to avoid, and also, measurably, the only thing that makes the driver
  // treat this work as urgent. Polling `isReady()` instead lets it dawdle: the
  // same material set is ~7 s of wall time when something is blocked on it and
  // ~15 s when nothing is. `?lighting=block` is here to keep that measurable
  // rather than asserted.
  if (mode === 'block') {
    pending.forEach((material) => {
      renderer.properties.get(material)?.currentProgram?.getUniforms?.();
    });
    pending.clear();
  }

  const total = pending.size;

  /**
   * PER-PROGRAM DRIVER TIME, which is the one thing the WebGL probe cannot see.
   *
   * `glprobe` charges a program the time something spent BLOCKED on it, and this
   * phase blocks on nothing — the driver links on its own thread while this loop
   * polls a non-blocking flag. So the probe reports these programs as nearly
   * free while the phase as a whole is the most expensive thing in the session
   * (measured: 26 programs, 14 245 ms). Without per-program times, "cut the
   * expensive permutations" has no way to say WHICH.
   *
   * Recording when each material's program first reports ready gives the
   * completion curve, and the gaps in that curve are the per-program costs — the
   * driver compiles serially, so a material that lands 900 ms after the one
   * before it cost 900 ms.
   */
  const curve = [];
  const started = performance.now();
  for (;;) {
    for (const material of [...pending]) {
      const program = renderer.properties.get(material)?.currentProgram;
      if (!program || program.isReady()) {
        pending.delete(material);
        curve.push({
          at: Math.round(performance.now() - started),
          name: material.name || '(unnamed)',
          // three's own cache key, so a permutation can be identified rather
          // than guessed at from the material name.
          key: String(program?.cacheKey ?? ''),
          // A material with no program at all was never really compiled; it is
          // deleted from `pending` so the loop can finish, and this says so
          // instead of letting it look like a free program.
          noProgram: !program,
        });
      }
    }
    if (pending.size === 0) break;
    await new Promise((r) => setTimeout(r, 16));
  }
  try { window.__RAMPCURVE__ = curve; } catch { /* non-browser */ }

  return {
    ok: true,
    ms: Math.round(performance.now() - t0),
    materials: total,
    compiled: (renderer.info.programs?.length ?? 0) - before,
  };
}

export async function prewarmScene(engine, { onProgress = null } = {}) {
  const t0 = performance.now();
  const render = engine.ctx.peek('render');
  const renderer = render?.renderer;
  if (!renderer) return { ok: false, reason: 'no renderer' };

  // The visible light count is part of every lit program's cache key, so it has
  // to be final before anything compiles. See `RenderSystem.settleLights()`.
  const settle = typeof location === 'undefined' ||
    new URLSearchParams(location.search).get('settle') !== '0';
  boot.time('prewarmScene:settleLights', () => {
    settle && render.settleLights?.(engine.ctx);
    settle && engine.ctx.peek('world')?.settleLights?.(engine.ctx);
  });

  const scratchRt = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false, stencilBuffer: false });
  const prevRt = renderer.getRenderTarget();
  const prevFace = renderer.getActiveCubeFace?.() ?? 0;
  const prevMip = renderer.getActiveMipmapLevel?.() ?? 0;
  const before = renderer.info.programs?.length ?? 0;

  renderer.setRenderTarget(scratchRt);
  try {
    await boot.timeAsync('prewarmScene:compile', async () => {
      // `compileAsync` would do exactly this, but it resolves once and tells
      // nobody anything in between — and on a first visit this is a seven-second
      // phase. So drive the same loop by hand and report as programs land.
      //
      // `compile()` hands back the set of materials it kicked off and each
      // program answers `isReady()`, so this is a COUNT of finished links, not a
      // timer pretending to be one. It is the only part of boot whose progress
      // can be known exactly, and it is the part that needs it most.
      // Split the reported fraction between the two scenes rather than letting
      // the second run silently: the world is the bulk of the programs, but a
      // viewmodel compile that reports nothing leaves the bar parked at the
      // world's last number and then jumping when the phase ends.
      // Both scenes go in together — see compileWithProgress. The old split
      // reported a 0.9/0.1 world/viewmodel share because the two phases ran one
      // after the other; now there is one phase and one honest fraction.
      await compileWithProgress(
        renderer,
        [
          { scene: engine.scene, camera: engine.camera },
          { scene: engine.viewScene, camera: engine.viewCamera },
        ],
        (f, d, t, ms) => onProgress?.(f, d, t, ms)
      );
    });
  } catch {
    // Older three, or a driver without the extension. Falling back to the
    // synchronous compile is still better than letting the first frame do it.
    try {
      renderer.compile(engine.scene, engine.camera);
      renderer.compile(engine.viewScene, engine.viewCamera);
    } catch { /* nothing more we can do; boot must still proceed */ }
  } finally {
    renderer.setRenderTarget(prevRt, prevFace, prevMip);
    scratchRt.dispose();
  }

  return {
    ok: true,
    ms: Math.round(performance.now() - t0),
    compiled: (renderer.info.programs?.length ?? 0) - before,
    parallel: !!renderer.getContext().getExtension('KHR_parallel_shader_compile'),
  };
}

export async function prewarm(engine, { onProgress = () => {}, transients = false, drawFrames = false } = {}) {
  const t0 = performance.now();
  const render = engine.ctx.peek('render');
  const renderer = render?.renderer;
  if (!renderer) return { ok: false, reason: 'no renderer' };

  const programsBefore = renderer.info.programs?.length ?? 0;
  const cam = engine.camera;
  const saved = { pos: cam.position.clone(), quat: cam.quaternion.clone(), fov: cam.fov };

  // Pre-warm has to be *simulation-transparent*, not just visually transparent.
  // It steps the engine, which advances the clock and the RNG stream; if that
  // residue survived, every downstream capture would drift and the pixel-diff
  // gate would report phantom regressions. Snapshot and restore both.
  const t = engine.time;
  const savedTime = { elapsed: t.elapsed, raw: t.raw, dt: t.dt, alpha: t.alpha, frame: t.frame };
  const r = engine.rng;
  const savedRng = { s0: r.s0, s1: r.s1, s2: r.s2, s3: r.s3, spare: r._spare };
  const savedAccum = engine._accum;

  // Subsystems whose materials only exist once they have spawned something.
  // These are the public debug hooks ARCHITECTURE.md already defines for the
  // capture harness; using them here costs nothing and reaches the transient
  // material permutations (particles, decals, ragdolls, flash, HUD layers).
  // Only kinds the subsystems actually implement — verified by reading their
  // sources, not guessed. fx.debugBurst understands 'explosion' | 'muzzle' |
  // 'combat' and a default wall burst; anything else falls through to the same
  // default, so enumerating surface names buys nothing. weapons.debugPose
  // understands 'idle' | 'ads' | 'fire'.
  const transientStages = [
    () => engine.ctx.peek('ai')?.debugStage?.('firefight'),
    () => engine.ctx.peek('fx')?.debugBurst?.('wall'),
    () => engine.ctx.peek('fx')?.debugBurst?.('explosion'),
    () => engine.ctx.peek('fx')?.debugBurst?.('muzzle'),
    () => engine.ctx.peek('fx')?.debugBurst?.('combat'),
    () => engine.ctx.peek('weapons')?.debugPose?.('fire'),
    () => engine.ctx.peek('weapons')?.debugPose?.('ads'),
    () => engine.ctx.peek('ui')?.debugState?.('combat'),
  ];

  // A RENDER TARGET MUST BE BOUND WHILE COMPILING. three folds `outputColorSpace`
  // and `toneMapping` into the program cache key and reads BOTH off the currently
  // bound target. With the canvas bound (the default here) every program compiled
  // is the `srgb` + tone-mapped variant — but the world and the viewmodel are both
  // drawn into HDR targets, which need `srgb-linear` + NoToneMapping. Measured by
  // src/materials and src/fx independently: 25 of 47 pre-warmed programs were the
  // unused canvas variant, and the real ones still compiled during the first
  // frames of play. A 1x1 target is enough to get the right key; nothing is ever
  // rendered into it. Restored in the caller's `finally`.
  const scratchRt = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false, stencilBuffer: false });
  const prevRt = renderer.getRenderTarget();
  const prevFace = renderer.getActiveCubeFace?.() ?? 0;
  const prevMip = renderer.getActiveMipmapLevel?.() ?? 0;

  const compile = async () => {
    // compileAsync is non-blocking where KHR_parallel_shader_compile exists.
    renderer.setRenderTarget(scratchRt);
    try {
      await renderer.compileAsync(engine.scene, engine.camera);
      await renderer.compileAsync(engine.viewScene, engine.viewCamera);
    } catch {
      // Older three or a driver without the extension — fall back to sync.
      try {
        renderer.compile(engine.scene, engine.camera);
        renderer.compile(engine.viewScene, engine.viewCamera);
      } catch { /* nothing more we can do; boot must still proceed */ }
    } finally {
      renderer.setRenderTarget(prevRt, prevFace, prevMip);
    }
  };

  const yieldFrame = () => new Promise((r) => requestAnimationFrame(r));

  try {
    // Light settling and the scene compile both happened in `prewarmScene()`,
    // which runs before the first frame. What is left here is everything the
    // first frame does NOT need.
    let step = 0;
    const totalSteps = 2 + (transients ? transientStages.length : 0) + 1;
    const tick = () => onProgress(Math.min(1, ++step / totalSteps));

    // Pass 1: compile the static world from each pose, with the depth/shadow
    // variants reached by drawing a real frame at that pose.
    {
      // The scene itself is already compiled; re-running it is one cheap
      // no-op pass that also picks up anything the streamed subsystems added
      // to the scene graph after prewarmScene() ran.
      await boot.timeAsync('prewarm:recompile', compile);
      tick();
      // Drawing real frames here would reach the depth/shadow and post-processing
      // variants too, but engine.step() advances every subsystem's internal state
      // (AI transforms, exposure adaptation, particle cursors) and NONE of that is
      // restorable from core. The pixel gate measured up-to-180/255 deltas from it.
      // So this is opt-in and off: compileAsync only, which mutates nothing.
      if (drawFrames) {
        engine.step();
        await yieldFrame();
        engine.step();
        await yieldFrame();
      }
      tick();
    }

    // Pass 1b: THE SUBSYSTEM HOOKS. This is the `prewarmMaterials()` contract the
    // doc comment above says is missing — "a prewarmMaterials() on each subsystem
    // that builds and compiles its materials WITHOUT spawning gameplay objects".
    // It is now implemented by render, world and ai, and it reaches exactly what
    // `compileAsync(scene, camera)` provably cannot:
    //
    //   render  the CSM depth pass, the MRT prepass and the ~13 full-screen post
    //           materials (blitted into a 4x4 scratch). +34-40 programs.
    //   world   the CSM-depth and prepass override variants of the level geometry,
    //           in their plain / instanced / instanced+instanceColor flavours,
    //           compiled at the stabilised light count. +35 programs.
    //   ai      the 26 character materials and their skinned + depth variants,
    //           against a dummy SkinnedMesh on the real skeleton. +7 programs.
    //           (ai also calls this itself at the end of init(); it is idempotent.)
    //
    // None of them draws a gameplay frame, steps the engine, touches the clock or
    // the RNG, so none of the restore machinery above applies to them — which is
    // why this replaces the `drawFrames` option rather than extending it.
    //
    // The camera goes back to its real pose FIRST: render's hook runs the shadow
    // and prepass passes for real (at frame 0, where it is pixel-clean), and there
    // is no reason to fit the cascades to a warm-up pose the game never uses.
    cam.position.copy(saved.pos);
    cam.quaternion.copy(saved.quat);
    cam.fov = saved.fov;
    cam.updateProjectionMatrix();
    cam.updateMatrixWorld(true);

    // render goes first, deliberately: it patches every lit material with the
    // CSM/AO/SSR injection, and a program compiled off an UNPATCHED material is
    // thrown away by the first frame that walks the scene.
    const hooks = [];
    const renderSys = engine.registry.peek?.('render');
    if (renderSys && typeof renderSys.prewarmMaterials === 'function') hooks.push(renderSys);
    for (const sys of engine.registry.ordered ?? []) {
      if (sys === renderSys) continue;
      if (SELF_WARMING.has(sys.constructor?.id)) continue;
      if (typeof sys.prewarmMaterials === 'function') hooks.push(sys);
    }
    // BIND AN HDR TARGET AROUND THE HOOKS, for the same reason `compile()` does.
    //
    // three folds the CURRENTLY BOUND target's colour space and tone mapping
    // into the program cache key. With the canvas bound — which is the default
    // out here — every program a hook compiles is the `srgb` + tone-mapped
    // variant, while the world and the viewmodel are both drawn into HDR
    // targets and want `srgb-linear` + NoToneMapping. The `srgb` copies are then
    // dead weight and the real programs still compile during the first frames of
    // play. `compile()` above already learned this the hard way (25 of 47
    // pre-warmed programs were the wrong variant); the hooks needed the same
    // treatment, and `outputColorSpace` was still the top permutation axis in
    // `node tools/bootprofile.mjs --programs` until they got it.
    renderer.setRenderTarget(scratchRt);
    const hookResults = {};
    for (const sys of hooks) {
      const id = sys.constructor?.id ?? '?';
      try {
        const arg = sys === renderSys ? { post: true, shadow: RENDER_SHADOW_WARM } : engine.ctx;
        hookResults[id] =
          (await boot.timeAsync(`prewarm:hook:${id}`, () => sys.prewarmMaterials(arg))) ?? { ok: true };
      } catch (err) {
        // An optional hook must never be able to block boot.
        hookResults[id] = { ok: false, reason: String(err?.message ?? err) };
      }
    }
    renderer.setRenderTarget(prevRt, prevFace, prevMip);
    engine.__prewarmHooks = hookResults;

    // Pass 2: spawn each subsystem's transient objects and compile those too.
    // Gated: see the `transients` option doc — this pass is not pixel-transparent.
    for (const spawn of (transients ? transientStages : [])) {
      try { spawn(); } catch { /* subsystem may not implement the hook */ }
      engine.step();
      await yieldFrame();
      await compile();
      engine.step();
      await yieldFrame();
      tick();
    }
    tick();
  } finally {
    // Restore exactly what we found. Any residue here would be a visual change.
    for (const reset of (transients ? [
      () => engine.ctx.peek('fx')?.debugBurst?.('none'),
      () => engine.ctx.peek('weapons')?.debugPose?.('idle'),
      () => engine.ctx.peek('ui')?.debugState?.('clean'),
      () => engine.ctx.peek('ai')?.debugStage?.('none'),
    ] : [])) {
      try { reset(); } catch { /* optional hook */ }
    }
    cam.position.copy(saved.pos);
    cam.quaternion.copy(saved.quat);
    cam.fov = saved.fov;
    cam.updateProjectionMatrix();
    cam.updateMatrixWorld(true);

    Object.assign(engine.time, savedTime);
    r.s0 = savedRng.s0;
    r.s1 = savedRng.s1;
    r.s2 = savedRng.s2;
    r.s3 = savedRng.s3;
    r._spare = savedRng.spare;
    engine._accum = savedAccum;
    engine._last = performance.now();
    renderer.setRenderTarget(prevRt, prevFace, prevMip);
    scratchRt.dispose();
  }

  const programsAfter = renderer.info.programs?.length ?? 0;
  return {
    ok: true,
    hooks: engine.__prewarmHooks,
    ms: Math.round(performance.now() - t0),
    programsBefore,
    programsAfter,
    compiled: programsAfter - programsBefore,
    parallel: !!renderer.getContext().getExtension('KHR_parallel_shader_compile'),
  };
}
