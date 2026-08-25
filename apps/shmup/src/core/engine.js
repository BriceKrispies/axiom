import * as THREE from 'three';
import { Registry, EventBus } from './registry.js';
import { boot } from './profile.js';
import { Streamer } from './streaming.js';
import { FIXED_DT, MAX_SUBSTEPS } from './config.js';
import { Input } from './input.js';
import { Rng } from './rng.js';

/**
 * The Engine owns the frame loop and the shared context handed to every
 * subsystem. It does NOT know what any subsystem does — it only sequences them.
 *
 * Frame order:
 *   1. input.beginFrame()
 *   2. fixedUpdate(FIXED_DT) xN   — physics, deterministic gameplay
 *   3. update(dt)                 — animation, cameras, AI decisions
 *   4. lateUpdate(dt)             — anything that must observe final transforms
 *   5. render subsystem draws
 *   6. input.endFrame()
 */
export class Engine {
  /**
   * @param opts.bakery  optional worker pool for pure procedural work, handed
   *   in by the composition root. Core does not build it, because the recipes
   *   live in subsystems and core does not import subsystems; the Engine only
   *   publishes it on ctx so any subsystem can use it during init.
   */
  constructor({ canvas, config, bakery = null }) {
    this.canvas = canvas;
    this.config = config;
    this.bakery = bakery;
    this.registry = new Registry();
    this.events = new EventBus();
    this.input = new Input(canvas, config);
    this.rng = new Rng(config.deterministic ? 0x5eed1234 : (Math.random() * 2 ** 32) >>> 0);
    this._traceForks();

    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(config.fov, 1, 0.05, 1200);
    this.camera.rotation.order = 'YXZ';

    /** Separate scene+camera for the first-person viewmodel, drawn with its own
     *  near plane so hands/weapon never clip into world geometry. */
    this.viewScene = new THREE.Scene();
    this.viewCamera = new THREE.PerspectiveCamera(60, 1, 0.005, 12);

    this.time = {
      /** Seconds since start, scaled. */ elapsed: 0,
      /** Unscaled wall-clock seconds since start. */ raw: 0,
      /** Last frame delta, scaled and clamped. */ dt: 0,
      /** Fixed step. */ fixed: FIXED_DT,
      /** Interpolation alpha between the last two physics steps, 0..1. */ alpha: 0,
      scale: 1,
      frame: 0,
    };

    this.ctx = {
      engine: this,
      scene: this.scene,
      camera: this.camera,
      viewScene: this.viewScene,
      viewCamera: this.viewCamera,
      canvas,
      config,
      events: this.events,
      input: this.input,
      time: this.time,
      rng: this.rng,
      bakery,
      get: (id) => this.registry.get(id),
      peek: (id) => this.registry.peek(id),
      has: (id) => this.registry.has(id),
    };

    /** Deferred construction, drained across frames once the loop is live. */
    this.streamer = new Streamer({ budgetMs: config.streamBudgetMs ?? 6 });

    this._accum = 0;
    this._last = 0;
    this._running = false;
    this._onResize = () => this.resize();
  }

  /**
   * `?rngtrace=1` — record a stack for every fork of the ROOT stream.
   *
   * Every procedural thing in this game hangs off this one stream, and each
   * subsystem's seed is decided purely by HOW MANY forks preceded it. So a
   * change that adds, removes or moves a single root fork reseeds everything
   * downstream of it and repaints the game — which the pixel gate reports as
   * "all eleven shots changed", true and useless.
   *
   * This turns that into one line. It found the real case it was written for:
   * `Viewmodel` forked ctx.rng from inside its constructor, so it sat at
   * position 6 only because weapons happened to init before fx — and hoisting
   * the other forks into prepare() silently moved it to position 10, reseeding
   * fx, ai, ui and audio. Read it with `node tools/rngprobe.mjs`.
   *
   * Off unless asked for: it allocates an Error per fork.
   */
  _traceForks() {
    const on = typeof location !== 'undefined' &&
      new URLSearchParams(location.search).get('rngtrace') === '1';
    if (!on) return;
    const trace = [];
    const orig = this.rng.fork.bind(this.rng);
    this.rng.fork = () => {
      trace.push(String(new Error().stack).split(/\r?\n/).slice(2, 5).join(' | '));
      return orig();
    };
    this.rngForkTrace = trace;
  }

  add(SystemClass, opts) {
    this.registry.add(new SystemClass(opts));
    return this;
  }

  async init() {
    const order = this.registry.resolve();

    // PREPARE PASS — claim seeds and start pure precomputation, before any
    // subsystem blocks the main thread.
    //
    // Two things happen here and nothing else may:
    //   1. every subsystem forks its private Rng off ctx.rng;
    //   2. a subsystem with pure, seed-driven precomputation (the procedural
    //      texture bakes) queues it on the bakery and keeps the promise.
    //
    // WHY IT IS A SEPARATE PASS. Those bakes are ~3.5 s of value-noise
    // evaluation, and if each one is queued inside its own subsystem's init
    // there is nothing left to overlap it with — `ai` is second-to-last, so it
    // awaits ~2.4 s with the main thread idle. Started here, every bake is in
    // flight on a worker while `render`, `world` and `weapons` build their
    // object graphs on this thread. Measured with tools/bootprofile.mjs.
    //
    // WHY THE FORK MOVED WITH IT. The fork is a single `u32()` draw from
    // ctx.rng, and its POSITION in that stream decides every seed downstream of
    // it. Hoisting only the baking subsystems would shift everyone else's draw
    // and repaint the whole game. This pass runs in `resolve()` order — the same
    // order the init loop below uses — so the draw sequence is byte-for-byte
    // what it was when the forks lived in init(). The pixel gate is the proof.
    boot.time('engine.prepare', () => {
      for (const sys of order) sys.prepare?.(this.ctx);
    });

    // Sequential BY CONTRACT, not by accident: `resolve()` returns dependency
    // order and a subsystem may read any of its declared deps out of ctx during
    // its own init. Overlapping two of these is only legal between subsystems
    // with no path between them in the dep graph — and, because almost every
    // expensive init here is main-thread GPU work on one shared context, the
    // legal pairs would interleave rather than overlap. Profile before assuming
    // concurrency is the win: `node tools/bootprofile.mjs`.
    for (const sys of order) {
      const id = sys.constructor.id;
      const t0 = performance.now();
      await boot.timeAsync(`init:${id}`, () => sys.init?.(this.ctx));
      const ms = performance.now() - t0;
      if (ms > 50) console.info(`[engine] ${id} init ${ms.toFixed(0)}ms`);
    }
    // Collect the deferred half of every subsystem's construction, in the same
    // dependency order the inits ran in — so the RNG draws and the object-graph
    // mutations happen in exactly the sequence they did when this work was
    // inline. See src/core/streaming.js.
    boot.time('engine.collectStream', () => {
      for (const sys of order) this.streamer.add(sys.constructor.id, sys.stream?.(this.ctx));
    });

    boot.time('engine.attach', () => {
      this.input.attach();
      // Keys pressed from here on are BUFFERED even though no frame has run yet
      // (see Input.beginFrame), so this is where the game stops discarding the
      // player's input — distinct from, and earlier than, when it acts on it.
      boot.milestone('input-armed');
      addEventListener('resize', this._onResize);
      this.resize();
    });
    return this;
  }

  /**
   * Finish every subsystem's deferred construction NOW, rather than across
   * frames. The capture harness calls this before it raises `__READY__`: a shot
   * of a half-streamed world is not a regression, it is a different picture, and
   * the pixel gate cannot tell the difference. Also the right call for any
   * caller that wants "fully loaded" to mean fully loaded.
   */
  drainStream() {
    return boot.timeAsync('engine.drainStream', () => this.streamer.drainAll());
  }

  resize() {
    const w = Math.max(1, this.canvas.clientWidth || innerWidth);
    const h = Math.max(1, this.canvas.clientHeight || innerHeight);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.viewCamera.aspect = w / h;
    this.viewCamera.updateProjectionMatrix();
    for (const sys of this.registry.with('resize')) sys.resize(w, h, this.ctx);
    this.events.emit('resize', { width: w, height: h });
  }

  start() {
    if (this._running) return;
    this._running = true;
    this._last = performance.now();
    this._loop = this._loop.bind(this);
    requestAnimationFrame(this._loop);
  }

  stop() {
    this._running = false;
  }

  _loop(now) {
    if (!this._running) return;
    requestAnimationFrame(this._loop);
    this.step(now);
  }

  /** Advance one frame. Exposed so the capture harness can pump frames by hand. */
  step(now = performance.now()) {
    const t = this.time;
    // Clamp so a tab-switch or a breakpoint doesn't teleport the simulation.
    const rawDt = Math.min(0.1, Math.max(0, (now - this._last) / 1000));
    this._last = now;
    t.raw += rawDt;
    t.dt = rawDt * t.scale;
    t.elapsed += t.dt;
    t.frame++;

    this.input.beginFrame();

    this._accum += t.dt;
    let steps = 0;
    const fixedSystems = this.registry.with('fixedUpdate');
    while (this._accum >= FIXED_DT && steps < MAX_SUBSTEPS) {
      for (const sys of fixedSystems) sys.fixedUpdate(FIXED_DT, this.ctx);
      this._accum -= FIXED_DT;
      steps++;
    }
    if (steps === MAX_SUBSTEPS) this._accum = 0; // shed backlog rather than spiral
    t.alpha = this._accum / FIXED_DT;

    for (const sys of this.registry.with('update')) sys.update(t.dt, this.ctx);
    for (const sys of this.registry.with('lateUpdate')) sys.lateUpdate(t.dt, this.ctx);

    const renderSystem = this.registry.peek('render');
    if (typeof renderSystem?.render === 'function') renderSystem.render(this.ctx);

    // THE number progressive boot exists to move: when the player first sees
    // the game, as opposed to when the last deferred thing finishes loading.
    boot.milestone('first-frame');

    this.input.endFrame();

    // AFTER the frame is drawn, never before: the first frame must pay nothing
    // for streaming, and every frame after it should present what it has before
    // spending anything on what it does not have yet.
    const streaming = this.streamer.step();
    if (!streaming && !this._streamDone) {
      this._streamDone = true;
      this.events.emit('stream:done', this.streamer.stats);
    }
  }

  dispose() {
    this.stop();
    removeEventListener('resize', this._onResize);
    this.input.detach();
    for (const sys of [...this.registry.ordered].reverse()) sys.dispose?.();
    this.bakery?.dispose();
    this.events.clear();
  }
}
