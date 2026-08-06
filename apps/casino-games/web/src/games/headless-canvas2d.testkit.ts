/*
 * headless-canvas2d.testkit.ts — the headless browser surface a bare
 * `node --test` process needs in order to run a REAL casino game on the engine's
 * REAL Canvas2D software rasterizer, one frame at a time, under a clock the test
 * owns.
 *
 * Why this exists. Every other test in this app is pure: it folds ticks and
 * asserts on values, and never touches the engine. Nothing tested the thing a
 * player actually experiences on the software path — how long a frame takes,
 * end to end, from `foldRoundTick` through `view` → `reconcile` → the store →
 * `backend-canvas2d`'s scanline rasterizer → the present. The only tools for
 * that were the Playwright benchmarks in `web/browser/`, which need a browser,
 * a served build, and a human reading a number.
 *
 * What is REAL here (i.e. what the frame time measures):
 *   - the game: `TREASURE_CHEST_PICK.mount` → `mountCasinoGame` → `runGame`.
 *     The fold, the phase machine, the result source, the scene author, the
 *     reconciler, the retained store — all of it, unmodified.
 *   - the renderer: `initRenderer(canvas, "canvas2d", quality)` builds the
 *     genuine `backend-canvas2d` rasterizer. Every frame transforms, lights,
 *     near-plane-clips and rasterizes the real triangles into a real
 *     `Uint32Array` framebuffer with a real depth buffer, at the real backing
 *     size resolved from the CSS box + quality.
 *   - the loop: `startLoop`'s genuine `FixedStepper`, driven one animation frame
 *     per `frame()` call.
 *   - input: the game's own `InputState`, fed the way the capture agent's
 *     `__casino.pointer` feeds it (logical 960x600 coordinates).
 *
 * What is a STUB, and therefore NOT in the measured cost:
 *   - `ctx.putImageData` — the browser's blit of the finished framebuffer to the
 *     compositor. Counted (`counters.blits`), not performed.
 *   - the 2D VECTOR ops of the water overlay layer (`fill`, `stroke`, `clip`,
 *     gradients). The overlay's geometry — projecting the pool rim, the nine
 *     chest silhouettes, generating the ripple lattice — runs for real; only the
 *     final path rasterization is absent. Counted as `counters.overlayOps`.
 *   - WebAudio. A muted mount emits no tones at all (see `casino-mount.ts`'s
 *     `scaled`), so the stub is never reached rather than being faked.
 *
 * So a frame time here is "everything Axiom computes, minus what the browser's
 * compositor and 2D vector rasterizer would add." That is the right thing to
 * regression-test: it is the part this repo can break.
 *
 * The stubs are STRICT. Every DOM object is wrapped in a Proxy that throws the
 * moment the engine touches a member the stub does not implement, because the
 * failure mode of a permissive stub is a silently cheaper frame — a performance
 * test that quietly stops measuring the work it was written to measure.
 *
 * Test-only, by the `*.testkit.ts` convention this repo already uses for
 * fakes (`packages/axiom-game/src/fake-host.testkit.ts`). The app's `tsconfig.json`
 * excludes it from the browser build.
 */

const MS_PER_SECOND = 1000;

/** Default CSS box: the geometry `definition.ts` quotes its measured frame rates
 * at, so the numbers this harness prints are comparable to that note. */
const DEFAULT_CSS_WIDTH = 936;
const DEFAULT_CSS_HEIGHT = 585;

/** What the harness observed the engine do, so a test can assert the work
 * actually happened instead of trusting a fast number. */
export interface HeadlessCounters {
  /** Animation frames pumped. */
  frames: number;
  /** Framebuffer presents (`putImageData`) — one per rendered frame. */
  blits: number;
  /** Total framebuffer pixels presented, across all frames. */
  blitPixels: number;
  /** Canvas2D vector calls the overlay layer made (proof it drew at all). */
  overlayOps: number;
}

export interface HeadlessCanvas2dOptions {
  /** The canvas's CSS box, which (with the quality) decides the backing size. */
  readonly cssWidth?: number;
  readonly cssHeight?: number;
  /** `window.devicePixelRatio`. 1 by default — the chest game pins `fixed-1x`. */
  readonly devicePixelRatio?: number;
  /** The virtual clock's step per animation frame; match the game's `fixedHz`
   * so exactly one fixed simulation step is due per frame. */
  readonly fixedHz?: number;
}

export interface HeadlessCanvas2dBrowser {
  /** The canvas to mount a game on. */
  readonly canvas: HTMLCanvasElement;
  /**
   * Advance exactly one animation frame and return the REAL wall-clock
   * milliseconds it cost.
   *
   * The engine's clock (`performance.now`) is virtual and advances by exactly
   * one fixed step per call, so the simulation is deterministic no matter how
   * fast or slow the machine is: the same frame count always walks the same
   * ticks and draws the same scene. The measurement uses `process.hrtime` and
   * is therefore independent of that virtual clock. Determinism in what is
   * measured; honesty in the measurement.
   */
  readonly frame: () => number;
  readonly counters: HeadlessCounters;
  /** The canvas's resolved backing store — the rasterizer's sample grid. */
  readonly backingSize: () => { readonly width: number; readonly height: number };
  /** Restore every global this installed. */
  readonly teardown: () => void;
}

/**
 * Wrap a stub so an unimplemented member is a loud failure, never a silently
 * cheaper frame. `key in target` walks the prototype chain, so class methods and
 * `Object.prototype` members (`toString`, used by node's inspector on failure)
 * stay reachable.
 *
 * Methods come back BOUND to the real target, and the bindings are cached. Bound
 * because a method invoked through the proxy would otherwise receive the proxy as
 * `this` and be unable to read the stub's own `#private` fields; cached because
 * the overlay path calls these thousands of times per frame and allocating a
 * fresh bound function per call would put the harness into the measurement.
 */
const strict = <T extends object>(target: T, label: string): T => {
  const bound = new Map<PropertyKey, unknown>();
  return new Proxy(target, {
    get: (object, key) => {
      if (typeof key === "string" && !(key in object)) {
        throw new Error(`${label}: the engine read '${key}', which this headless stub does not implement`);
      }
      const cached = bound.get(key);
      if (cached !== undefined) {
        return cached;
      }
      const value = Reflect.get(object, key);
      if (typeof value !== "function") {
        return value;
      }
      const fn = (value as (...args: unknown[]) => unknown).bind(object);
      bound.set(key, fn);
      return fn;
    },
    set: (object, key, value) => {
      if (typeof key === "string" && !(key in object)) {
        throw new Error(`${label}: the engine wrote '${key}', which this headless stub does not implement`);
      }
      return Reflect.set(object, key, value);
    },
  });
};

/** A gradient that accepts stops and paints nothing (the overlay's fill styles
 * are never rasterized here — see the file header). */
const flatGradient = (): CanvasGradient => strict({ addColorStop: (): void => {} }, "CanvasGradient") as CanvasGradient;

/**
 * The 2D context stub, serving BOTH of the app's Canvas2D roles:
 *
 *  - the 3D backend's framebuffer target (`createImageData` / `putImageData`),
 *    where the `ImageData` is genuine typed-array memory because the rasterizer
 *    writes millions of pixels into it and that write is the cost being measured;
 *  - the overlay layer's vector target, whose calls are counted rather than
 *    rasterized.
 */
const createContext2d = (canvas: unknown, counters: HeadlessCounters): CanvasRenderingContext2D => {
  const op = (): void => {
    counters.overlayOps += 1;
  };
  const context = {
    canvas,
    // Vector state the overlay writes (`casino-mount.ts` + `canvas-water.ts`).
    fillStyle: "" as string | CanvasGradient,
    filter: "none",
    globalAlpha: 1,
    globalCompositeOperation: "source-over",
    lineCap: "round",
    lineJoin: "round",
    lineWidth: 1,
    miterLimit: 10,
    strokeStyle: "" as string | CanvasGradient,
    // Vector ops: counted, not rasterized.
    arc: op,
    beginPath: op,
    clearRect: op,
    clip: op,
    closePath: op,
    createRadialGradient: (): CanvasGradient => {
      op();
      return flatGradient();
    },
    ellipse: op,
    fill: op,
    fillRect: op,
    lineTo: op,
    moveTo: op,
    restore: op,
    save: op,
    setTransform: op,
    stroke: op,
    translate: op,
    // The software rasterizer's framebuffer: real memory, really written to.
    createImageData: (width: number, height: number): ImageData =>
      ({ colorSpace: "srgb", data: new Uint8ClampedArray(width * height * 4), height, width }) as ImageData,
    // The present. The one cost a browser would add that this harness does not pay.
    putImageData: (image: ImageData): void => {
      counters.blits += 1;
      counters.blitPixels += image.width * image.height;
    },
  };
  return strict(context, "CanvasRenderingContext2D") as unknown as CanvasRenderingContext2D;
};

/** The minimal element surface `renderer.ts`, `dom-input.ts` and
 * `casino-mount.ts`'s overlay actually touch. */
class HeadlessElement {
  public width = 300;
  public height = 150;
  public parentElement: HeadlessElement | null = null;
  public nextSibling: HeadlessElement | null = null;
  public readonly style: Record<string, string> = { cssText: "" };
  public readonly children: HeadlessElement[] = [];
  readonly #rect: DOMRect;
  readonly #counters: HeadlessCounters;
  #context: CanvasRenderingContext2D | null = null;

  public constructor(rect: DOMRect, counters: HeadlessCounters) {
    this.#rect = rect;
    this.#counters = counters;
  }

  public get parentNode(): HeadlessElement | null {
    return this.parentElement;
  }

  public getContext(kind: string): CanvasRenderingContext2D | null {
    if (kind !== "2d") {
      // The whole point of this harness: only the software path exists here, so
      // an accidental `?backend=webgl2` must fail loudly rather than fall back.
      return null;
    }
    this.#context ??= createContext2d(this, this.#counters);
    return this.#context;
  }

  /** The CSS box. Fixed: there is no layout here, and `renderer.ts` reads this
   * to resolve the backing store. */
  public getBoundingClientRect(): DOMRect {
    return this.#rect;
  }

  public append(child: HeadlessElement): void {
    child.parentElement = this;
    this.children.push(child);
  }

  public insertBefore(child: HeadlessElement): void {
    this.append(child);
  }

  public remove(): void {
    this.parentElement = null;
  }

  public setAttribute(): void {}
  public addEventListener(): void {}
  public removeEventListener(): void {}
}

/**
 * Install the headless browser globals and return a canvas plus a one-frame
 * pump. Call `teardown()` when the test is done; every global is restored.
 */
export const installHeadlessCanvas2dBrowser = (options: HeadlessCanvas2dOptions = {}): HeadlessCanvas2dBrowser => {
  const cssWidth = options.cssWidth ?? DEFAULT_CSS_WIDTH;
  const cssHeight = options.cssHeight ?? DEFAULT_CSS_HEIGHT;
  const stepMs = MS_PER_SECOND / (options.fixedHz ?? 60);

  const counters: HeadlessCounters = { blitPixels: 0, blits: 0, frames: 0, overlayOps: 0 };
  const rect = {
    bottom: cssHeight,
    height: cssHeight,
    left: 0,
    right: cssWidth,
    toJSON: (): unknown => ({}),
    top: 0,
    width: cssWidth,
    x: 0,
    y: 0,
  } as DOMRect;

  const host = strict(new HeadlessElement(rect, counters), "HTMLElement");
  const canvas = strict(new HeadlessElement(rect, counters), "HTMLCanvasElement");
  host.append(canvas);

  // The engine's clock. Virtual, so the simulation is identical on every machine.
  let virtualMs = 0;
  let queue: (() => void)[] = [];
  let nextFrameId = 1;

  const saved = new Map<string, PropertyDescriptor | undefined>();
  const define = (name: string, value: unknown): void => {
    saved.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, value, writable: true });
  };

  // Inherit the real Performance object so anything else in the process keeps
  // working, and override only `now` with the virtual clock.
  const virtualPerformance = Object.create(globalThis.performance) as Performance;
  Object.defineProperty(virtualPerformance, "now", { configurable: true, value: (): number => virtualMs });
  define("performance", virtualPerformance);

  define("requestAnimationFrame", (callback: () => void): number => {
    queue.push(callback);
    nextFrameId += 1;
    return nextFrameId;
  });
  define("cancelAnimationFrame", (): void => {
    queue = [];
  });
  define("window", strict({ devicePixelRatio: options.devicePixelRatio ?? 1 }, "window"));
  define(
    "document",
    strict({ createElement: (): HeadlessElement => strict(new HeadlessElement(rect, counters), "HTMLCanvasElement") }, "document"),
  );
  // `renderer.ts` and `casino-mount.ts` both observe the canvas for resizes. There
  // is no layout here and the CSS box never changes, so observing is a no-op —
  // both call sites run their sync callback once directly after constructing it.
  define(
    "ResizeObserver",
    class {
      public observe(): void {}
      public unobserve(): void {}
      public disconnect(): void {}
    },
  );
  // `attachDomInput` registers key/blur listeners on `globalThis`, which is not an
  // EventTarget under Node. The test drives input through `InputState` directly
  // (as the capture agent does), so these only need to exist.
  define("addEventListener", (): void => {});
  define("removeEventListener", (): void => {});

  return {
    backingSize: (): { readonly width: number; readonly height: number } => ({ height: canvas.height, width: canvas.width }),
    canvas: canvas as unknown as HTMLCanvasElement,
    counters,
    frame: (): number => {
      const due = queue;
      queue = [];
      if (due.length === 0) {
        throw new Error("headless: no animation frame is pending — is the game's loop running (or already stopped)?");
      }
      // Advance the ENGINE's clock by exactly one fixed step, then measure the
      // REAL time the frame costs. The two clocks are deliberately unrelated.
      virtualMs += stepMs;
      const startedNs = process.hrtime.bigint();
      for (const callback of due) {
        callback();
      }
      const elapsedNs = process.hrtime.bigint() - startedNs;
      counters.frames += 1;
      return Number(elapsedNs) / 1e6;
    },
    teardown: (): void => {
      for (const [name, descriptor] of saved) {
        if (descriptor === undefined) {
          Reflect.deleteProperty(globalThis, name);
        } else {
          Object.defineProperty(globalThis, name, descriptor);
        }
      }
      saved.clear();
      queue = [];
    },
  };
};
