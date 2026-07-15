/*
 * run-game.ts — the imperative SHELL that runs a pure `Game`. This is the platform
 * edge for the functional-authoring layer: it owns every effect the game does NOT
 * — it resolves a render backend (`initRenderer`), binds DOM input, creates the
 * declared resources ONCE, drives the fixed-step loop, and per tick/frame calls the
 * game's pure functions and applies their results. The author's `init`/`update`/
 * `view`/`sound` never run here; they are called FROM here with plain values.
 *
 * Per fixed tick: snapshot input into an immutable `InputFrame`, fold it through
 * `update`, and play whatever tones `sound` returns. Per rendered frame: call
 * `view` to get a `Scene` value, `reconcile` it against the last applied scene, and
 * execute the minimal spawn/re-pose/despawn (+ light) plan against the retained
 * store before `renderScene`. Immediate-mode authoring, retained-mode execution.
 *
 * Like the engine's other browser boundaries this file sits OUTSIDE the branchless
 * / 100%-coverage spine laws (see the `.oxlintrc.json` override and
 * `test-exempt.json`); its correctness is proven by the live browser path, and the
 * pure pieces it drives (`reconcile`, `sampleInput`, `FixedStepper`) are covered.
 */

import type { Handle, ToneSpec } from "./api.ts";
import { type Game, type MeshRef, type ReconcilePlan, type SceneMemory, type TickContext, type ViewContext, emptyMemory, reconcile } from "./game.ts";
import { InputState, sampleInput } from "./input.ts";
import { attachDomInput } from "./dom-input.ts";
import { type BackendChoice, initRenderer } from "./renderer.ts";
import { startLoop } from "./raf-loop.ts";
import { playTone } from "./audio.ts";
import {
  addLight,
  createMaterial,
  createMesh,
  createMeshData,
  despawnRenderable,
  removeLight,
  renderScene,
  setCamera3D,
  setClearColor,
  setLight,
  setNodeTransform,
  spawnRenderable,
} from "./store.ts";

/** Tuning + hooks for `runGame`. All optional; the defaults suit a 60 Hz game. */
export interface RunGameOptions<State> {
  /** Which drawing backend to use (default "auto": WebGL2, else Canvas2D). */
  readonly backend?: BackendChoice;
  /** Whether clicking the canvas captures the pointer for mouse look (default
   * true). A cursor-driven game (clickable objects, menus) sets this false so
   * the cursor stays visible and clicks stay clicks. */
  readonly pointerLock?: boolean;
  /** Fixed simulation rate (default 60). `update` runs this many times per second. */
  readonly fixedHz?: number;
  /** Cap on catch-up steps a single slow frame may run (default 8). */
  readonly maxCatchUpSteps?: number;
  /** Seed passed to `init` (default 1). */
  readonly seed?: number;
  /** Wall clock for `view`'s `nowMs` (default `performance.now`). Pin it for
   * deterministic captures. */
  readonly now?: () => number;
  /** Stop advancing the simulation after this tick — `view` still renders, so a
   * screenshot at a fixed tick is stable (default: never freeze). */
  readonly freezeAtTick?: number;
  /** Inject synthetic input each tick BEFORE the snapshot (scripted autoplay, an
   * on-screen touch pad): called with the tick number and the live `InputState`. */
  readonly script?: (tick: number, input: InputState) => void;
  /** Observe the new state after each fixed `update` (e.g. accumulate a DOM HUD's
   * per-tick events). Read-only — the shell never uses the return value. */
  readonly onTick?: (state: State, ctx: TickContext) => void;
  /** Observe the state each rendered frame, after the scene is applied (e.g. flush
   * the DOM HUD). Read-only. */
  readonly onFrame?: (state: State, ctx: ViewContext) => void;
}

/** A running game: stop it, read its current state (for a DOM HUD), or reach the
 * live `InputState` (to feed an on-screen control pad). */
export interface RunningGame<State> {
  readonly stop: () => void;
  readonly getState: () => State;
  readonly input: InputState;
}

const DEFAULT_FIXED_HZ = 60;
const DEFAULT_MAX_CATCHUP = 8;
const DEFAULT_SEED = 1;

/** Resolve one declared mesh resource to a store handle (primitive kind or custom
 * geometry). */
const uploadMeshRef = (ref: MeshRef): Handle => ("kind" in ref ? createMesh(ref.kind) : createMeshData(ref.data));

/**
 * Run a pure `Game` on `canvas`. Resolves a backend, binds input, creates the
 * declared resources once, and starts the fixed-step loop that drives the game's
 * pure functions. Returns handles to stop it and observe its state.
 */
export const runGame = <State>(canvas: HTMLCanvasElement, game: Game<State>, opts: RunGameOptions<State> = {}): RunningGame<State> => {
  const fixedHz = opts.fixedHz ?? DEFAULT_FIXED_HZ;
  const dt = 1 / fixedHz;
  const now = opts.now ?? ((): number => performance.now());
  const freezeAtTick = opts.freezeAtTick ?? Number.POSITIVE_INFINITY;

  initRenderer(canvas, opts.backend ?? "auto");

  // Declared resources → store handles, created exactly once.
  const meshHandles = new Map<string, Handle>(
    Object.entries(game.resources.meshes).map(([name, ref]) => [name, uploadMeshRef(ref)] as const),
  );
  const materialHandles = new Map<string, Handle>(
    Object.entries(game.resources.materials).map(([name, spec]) => [name, createMaterial(spec)] as const),
  );

  const input = new InputState();
  const detachInput = attachDomInput(input, canvas, { pointerLock: opts.pointerLock ?? true });
  const actionNames = Object.keys(game.actions);
  for (const [action, codes] of Object.entries(game.actions)) {
    input.bindAction(action, codes);
  }

  // Retained identity: instance/light key → store entity.
  const instanceEntities = new Map<string, number>();
  const lightEntities = new Map<string, number>();
  let memory: SceneMemory = emptyMemory();

  const applyPlan = (plan: ReconcilePlan): void => {
    for (const key of plan.despawns) {
      const entity = instanceEntities.get(key);
      if (entity !== undefined) {
        despawnRenderable(entity);
        instanceEntities.delete(key);
      }
    }
    for (const instance of plan.spawns) {
      const mesh = meshHandles.get(instance.mesh);
      const material = materialHandles.get(instance.material);
      if (mesh !== undefined && material !== undefined) {
        instanceEntities.set(instance.key, spawnRenderable(mesh, material, instance.transform));
      }
    }
    for (const repose of plan.reposes) {
      const entity = instanceEntities.get(repose.key);
      if (entity !== undefined) {
        setNodeTransform(entity, repose.transform);
      }
    }
    for (const key of plan.removeLights) {
      const entity = lightEntities.get(key);
      if (entity !== undefined) {
        removeLight(entity);
        lightEntities.delete(key);
      }
    }
    for (const entry of plan.addLights) {
      lightEntities.set(entry.key, addLight(entry.light));
    }
    for (const entry of plan.setLights) {
      const entity = lightEntities.get(entry.key);
      if (entity !== undefined) {
        setLight(entity, entry.light);
      }
    }
  };

  let state = game.init(opts.seed ?? DEFAULT_SEED);

  const stopLoop = startLoop({
    fixedHz,
    maxCatchUpSteps: opts.maxCatchUpSteps ?? DEFAULT_MAX_CATCHUP,
    render: (): void => {
      const ctx: ViewContext = { nowMs: now() };
      const scene = game.view(state, ctx);
      const result = reconcile(memory, scene);
      memory = result.memory;
      applyPlan(result.plan);
      setCamera3D(scene.camera);
      if (scene.clearColor !== undefined) {
        setClearColor(scene.clearColor);
      }
      renderScene();
      opts.onFrame?.(state, ctx);
    },
    update: (tick: number): void => {
      opts.script?.(tick, input);
      input.beginTick();
      if (tick <= freezeAtTick) {
        const previous = state;
        const ctx: TickContext = { dt, tick };
        state = game.update(state, sampleInput(input, actionNames), ctx);
        const tones: readonly ToneSpec[] = game.sound ? game.sound(previous, state) : [];
        for (const tone of tones) {
          playTone(tone);
        }
        opts.onTick?.(state, ctx);
      }
    },
  });

  return {
    getState: (): State => state,
    input,
    stop: (): void => {
      stopLoop();
      detachInput();
    },
  };
};
