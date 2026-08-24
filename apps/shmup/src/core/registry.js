/**
 * Subsystem registry + shared context.
 *
 * CONTRACT — every subsystem is a class with:
 *   static id      : string, unique. Other systems fetch it via ctx.get(id).
 *   static deps    : string[] of subsystem ids that must init first.
 *   prepare(ctx)   : optional. Runs for EVERY subsystem, in dependency order,
 *                    before ANY init(). Fork your Rng here and queue pure
 *                    seed-to-bytes work on ctx.bakery here — nothing else: no
 *                    GPU, no scene graph, no other subsystem. Make it
 *                    idempotent and call it from your own init() too, so a
 *                    preview page without an Engine still works.
 *                    See ARCHITECTURE.md and src/core/engine.js for why the
 *                    POSITION of a root-stream fork is load-bearing.
 *   async init(ctx): build resources. May await asset loads. Build only what
 *                    frame 1 needs; defer the rest to stream().
 *   *stream(ctx)   : optional generator. Deferred construction, drained a few
 *                    ms per frame AFTER the frame loop is live — see
 *                    ARCHITECTURE.md and src/core/streaming.js. Yield at every
 *                    point it is safe to stop. The frames before a chunk lands
 *                    must render correctly without it.
 *   update(dt,ctx) : variable-rate, once per frame, before render.
 *   fixedUpdate(h,ctx): fixed-rate (PHYSICS_HZ), 0..N times per frame. Optional.
 *   lateUpdate(dt,ctx): after all update(), before render. Optional.
 *   resize(w,h,ctx): viewport changed. Optional.
 *   dispose()      : free GPU/CPU resources. Optional.
 *
 * Subsystems MUST NOT import each other directly — go through ctx.get(id).
 * That keeps the dependency graph explicit and lets agents own files in isolation.
 */

export class Registry {
  #systems = new Map();
  #order = [];

  add(system) {
    const id = system.constructor.id;
    if (!id) throw new Error(`${system.constructor.name} is missing a static id`);
    if (this.#systems.has(id)) throw new Error(`duplicate subsystem id "${id}"`);
    this.#systems.set(id, system);
    return this;
  }

  get(id) {
    const s = this.#systems.get(id);
    if (!s) throw new Error(`subsystem "${id}" not registered`);
    return s;
  }

  /** Non-throwing lookup for optional dependencies. */
  peek(id) {
    return this.#systems.get(id) ?? null;
  }

  has(id) {
    return this.#systems.has(id);
  }

  /** Topological sort over static deps; throws on cycles or missing deps. */
  resolve() {
    const seen = new Map(); // id -> 0 visiting, 1 done
    const out = [];
    const visit = (id, from) => {
      const state = seen.get(id);
      if (state === 1) return;
      if (state === 0) throw new Error(`dependency cycle at "${id}" (via ${from})`);
      const sys = this.#systems.get(id);
      if (!sys) throw new Error(`"${from}" depends on unregistered subsystem "${id}"`);
      seen.set(id, 0);
      for (const d of sys.constructor.deps ?? []) visit(d, id);
      seen.set(id, 1);
      out.push(sys);
    };
    for (const id of this.#systems.keys()) visit(id, '<root>');
    this.#order = out;
    return out;
  }

  get ordered() {
    return this.#order.length ? this.#order : this.resolve();
  }

  /** Systems that implement `method`, in dependency order. Cached per method. */
  #cache = new Map();
  with(method) {
    let list = this.#cache.get(method);
    if (!list) {
      list = this.ordered.filter((s) => typeof s[method] === 'function');
      this.#cache.set(method, list);
    }
    return list;
  }

  invalidate() {
    this.#cache.clear();
  }
}

/** Minimal typed event bus. Handlers are called synchronously. */
export class EventBus {
  #map = new Map();

  on(type, fn) {
    (this.#map.get(type) ?? this.#map.set(type, new Set()).get(type)).add(fn);
    return () => this.off(type, fn);
  }

  once(type, fn) {
    const off = this.on(type, (e) => {
      off();
      fn(e);
    });
    return off;
  }

  off(type, fn) {
    this.#map.get(type)?.delete(fn);
  }

  emit(type, payload) {
    const set = this.#map.get(type);
    if (!set) return;
    // Copy so handlers may unsubscribe during dispatch.
    for (const fn of [...set]) {
      try {
        fn(payload);
      } catch (err) {
        console.error(`[events] handler for "${type}" threw:`, err);
      }
    }
  }

  clear() {
    this.#map.clear();
  }
}
