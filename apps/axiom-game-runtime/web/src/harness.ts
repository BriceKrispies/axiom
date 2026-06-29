/*
 * The browser boot harness for the @axiom/game hot-reload spike.
 *
 * This is the "host"/platform edge the SDK is meant to be driven by — it is NOT
 * engine spine, so it lives in an app `web/` dir, outside the branchless + 100%
 * coverage gates, and uses ordinary `if`/`?.` control flow.
 *
 * What it proves about the dev UX:
 *   1. The WASM *engine* (apps/axiom-game-runtime) is loaded ONCE and stays
 *      alive for the whole session — the heavy, stateful runtime (deterministic
 *      fixed-step accumulator + seeded RNG).
 *   2. The game *author's* code (./game.ts) is plain JS layered on top, hot-
 *      swapped via a fresh dynamic `import()` whenever the dev server signals a
 *      change over Server-Sent Events. No page reload, no WASM rebuild.
 *   3. The swap goes through the real SDK seam: `defaultRegistry.reset()` then
 *      re-running the author module's `onFixedUpdate` registrations.
 *
 * The render path here (a canvas2d painter calling the author's exported `draw`)
 * is a deliberate spike stand-in for the not-yet-wired 2D surface. The dev-UX
 * machinery is identical regardless of what the author eventually draws into.
 */

import {
  type Frame,
  type NativeBridge,
  type SimContext,
  type StepBudget,
  GameRegistry,
  TickPump,
  makeFrame,
  makeSim,
  stepFrame,
  useRegistry,
} from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";

/** The author module shape the harness consumes: a presentation hook. */
interface AuthorModule {
  readonly draw?: (ctx: CanvasRenderingContext2D, width: number, height: number, frame: Frame) => void;
}

const FIXED_HZ = 60;
const NANOS_PER_SECOND = 1_000_000_000;
const NANOS_PER_MILLI = 1_000_000;
const MAX_STEPS_PER_FRAME = 8;

/*
 * Adapt the wasm `WasmGame` to the `NativeBridge` the SDK projects its `Sim` over.
 * Today the runtime implements the deterministic accumulator (`advance`),
 * `snapshot`, and the full RNG seam (SPEC-01); the world / input / physics /
 * timer / tween methods are not wired through wasm yet, so they throw if an
 * author reaches for them. Completing them is the game-side bridge work this
 * spike deliberately sidesteps — the dev-UX loop only needs the live seam.
 */
const makeBridge = (game: WasmGame): NativeBridge => {
  const notWired =
    (name: string) =>
    (): never => {
      throw new Error(`@axiom/game bridge.${name} is not wired through wasm yet (spike: RNG + advance only)`);
    };
  return {
    advance: (elapsedNanos: number): StepBudget => {
      const report = game.advance(BigInt(Math.round(elapsedNanos)));
      return {
        fixedStepNanos: Number(report.fixed_step_nanos),
        remainderNanos: Number(report.remainder_nanos),
        steps: report.steps,
      };
    },
    snapshot: (): Uint8Array => game.snapshot(),

    // Deterministic RNG (SPEC-01) — genuinely served by the live wasm engine.
    rngUnit: (stream: number): number => game.rngUnit(stream),
    rngBelow: (stream: number, maxExclusive: number): number => game.rngBelow(stream, maxExclusive),
    rngWeighted: (stream: number, weights: readonly number[]): number =>
      game.rngWeighted(stream, Float64Array.from(weights)),
    rngPermutation: (stream: number, length: number): readonly number[] => game.rngPermutation(stream, length),
    rngStream: (parent: number, name: string): number => game.rngStream(parent, name),

    // Not wired through wasm in this spike (game-side bridge completion work).
    worldSpawn: notWired("worldSpawn"),
    worldDespawn: notWired("worldDespawn"),
    worldDespawnSubtree: notWired("worldDespawnSubtree"),
    worldGet: notWired("worldGet"),
    worldSet: notWired("worldSet"),
    worldQuery: notWired("worldQuery"),
    worldChildrenOf: notWired("worldChildrenOf"),
    worldAlive: notWired("worldAlive"),
    worldHas: notWired("worldHas"),
    worldParentOf: notWired("worldParentOf"),
    worldRemove: notWired("worldRemove"),
    worldSetParent: notWired("worldSetParent"),
    worldWorldTransform: notWired("worldWorldTransform"),
    inputIsDown: notWired("inputIsDown"),
    inputPressed: notWired("inputPressed"),
    inputReleased: notWired("inputReleased"),
    inputPointer: notWired("inputPointer"),
    inputPointerPressed: notWired("inputPointerPressed"),
    inputSwipe: notWired("inputSwipe"),
    inputPressedAtTick: notWired("inputPressedAtTick"),
    timerAfter: notWired("timerAfter"),
    timerEvery: notWired("timerEvery"),
    timerCancel: notWired("timerCancel"),
    timersDue: notWired("timersDue"),
    machineCreate: notWired("machineCreate"),
    machineCurrent: notWired("machineCurrent"),
    machineTransition: notWired("machineTransition"),
    machineTicksInState: notWired("machineTicksInState"),
    tweenAdd: notWired("tweenAdd"),
    tweenCancel: notWired("tweenCancel"),
    tweenActive: notWired("tweenActive"),
    tweenValue: notWired("tweenValue"),
    tweenCompleted: notWired("tweenCompleted"),
    physicsSetConfig: notWired("physicsSetConfig"),
    physicsAddBody: notWired("physicsAddBody"),
    physicsApplyImpulse: notWired("physicsApplyImpulse"),
    physicsApplyForce: notWired("physicsApplyForce"),
    physicsApplyTorque: notWired("physicsApplyTorque"),
    physicsSetVelocity: notWired("physicsSetVelocity"),
    physicsSetAngularVelocity: notWired("physicsSetAngularVelocity"),
  };
};

const boot = async (): Promise<void> => {
  const canvas = document.getElementById("c") as HTMLCanvasElement;
  const ctx = canvas.getContext("2d") as CanvasRenderingContext2D;
  const status = document.getElementById("status") as HTMLSpanElement;

  const resize = (): void => {
    const dpr = Math.min(globalThis.devicePixelRatio || 1, 2);
    canvas.width = Math.floor(globalThis.innerWidth * dpr);
    canvas.height = Math.floor(globalThis.innerHeight * dpr);
  };
  globalThis.addEventListener("resize", resize);
  resize();

  // 1) Load the heavy, stateful WASM engine — ONCE. It survives every hot reload.
  await initWasm();
  const fixedStepNanos = BigInt(Math.round(NANOS_PER_SECOND / FIXED_HZ));
  const game = new WasmGame(fixedStepNanos, MAX_STEPS_PER_FRAME);
  const bridge = makeBridge(game);
  const context: SimContext = { bridge, fixedHz: FIXED_HZ, pump: new TickPump(bridge, FIXED_HZ) };

  // 2) The hot-swappable author layer. `useRegistry` is the real SDK seam: mint a
  //    FRESH per-game registry and make the free `onFixedUpdate`/`onRender` target
  //    it, then re-import so the author module's top-level registrations land in
  //    the new registry. The old one is simply discarded — no reset, no leak.
  let registry = new GameRegistry();
  let author: AuthorModule = {};
  const loadAuthor = async (version: number): Promise<void> => {
    registry = new GameRegistry();
    useRegistry(registry);
    author = (await import(`/dist/game.js?v=${version}`)) as AuthorModule;
  };
  await loadAuthor(0);

  // 3) Subscribe to the dev server's reload stream. Each save → fresh author
  //    module, engine untouched.
  const events = new EventSource("/events");
  events.addEventListener("reload", (event: MessageEvent<string>): void => {
    void loadAuthor(Number(event.data)).then((): void => {
      status.textContent = `hot-reloaded @ engine tick ${Number(game.current_tick)}`;
      status.classList.add("flash");
      globalThis.setTimeout((): void => status.classList.remove("flash"), 400);
    });
  });

  // 4) The frame loop: real wasm `advance` → registered fixed updates → author draw.
  let startTick = 0;
  let last = performance.now();
  const frame = (now: number): void => {
    const elapsedNanos = (now - last) * NANOS_PER_MILLI;
    last = now;
    const budget = bridge.advance(elapsedNanos);
    startTick = stepFrame({
      budget,
      fixedUpdates: registry.fixedUpdates(),
      makeFrame,
      makeSim: (tick: number) => makeSim(context, tick),
      renders: registry.renders(),
      startTick,
    });
    author.draw?.(ctx, canvas.width, canvas.height, makeFrame(startTick));
    if (!status.classList.contains("flash")) {
      status.textContent = `live · engine tick ${Number(game.current_tick)} · seed ${game.seed}`;
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
};

void boot();
