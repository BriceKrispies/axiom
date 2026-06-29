/*
 * The scene runtime (SPEC-14 §9): the mount that drives a `Scene`'s lifecycle from
 * the loop and scopes its `this.*` factories to the running tick. It is the missing
 * wiring between the `Scene` authoring shell (`scene.ts`) and the deterministic
 * loop (`game-loop.ts`): the loop owns *when* `preload`/`create`/`update` run, this
 * owns *what* `this.add`/`this.input`/… resolve to each phase.
 *
 * A scene's factories are built from the per-tick `Sim` the loop already mints
 * (`makeSim`): `this.add`/`this.input`/`this.physics`/`this.time`/`this.tweens` are
 * exactly the `Sim`'s bridge/pump-backed projections, so the scene and a free
 * `onFixedUpdate` see one identical world. `this.sound` is the free audio surface
 * (`sound.ts`) grouped into a namespace, and `this.cameras` is the documented 2D
 * camera stub (no projection yet). Before each lifecycle hook the mount calls
 * `scene.bindFactories(...)`, so a timer/tween/spawn an author issues in `create`
 * or `update` is scoped to the tick it runs at — deterministic and replayable.
 *
 * `create` runs once at start (over a tick-0 `Sim`) and `update` runs as a per-tick
 * `FixedUpdate` the loop schedules; `preload`'s declared asset keys are surfaced
 * through `assets()` for the loader. Modules-stay-isolated framing: this composition
 * (Sim projections + free sound + the scene shell) lives in the runtime tier, never
 * inside a single projection.
 */

import type { Cameras, Scene, SceneFactories, Sound } from "./scene.ts";
import { type Sim, type SimContext, makeSim } from "./sim.ts";
import {
  loadSound,
  playMusic,
  playSound,
  playTone,
  scheduleSound,
  setMasterVolume,
  setMuted,
  stopVoice,
} from "./sound.ts";
import type { FixedUpdate } from "./loop-core.ts";

/** The tick `create` runs at — the start of the game. */
const FIRST_TICK = 0;

/** The audio factory `this.sound` exposes: the free `sound.ts` functions grouped into the SPEC-08 namespace. */
const SOUND: Sound = {
  load: loadSound,
  music: playMusic,
  play: playSound,
  schedule: scheduleSound,
  setMasterVolume,
  setMuted,
  stop: stopVoice,
  tone: playTone,
};

/** The deferred `this.cameras` stub — no 2D `camera2D` projection exists yet (SPEC-04). */
const CAMERAS: Cameras = { subsystem: "cameras" };

/** Build the factory set for `sim`'s tick: the `Sim`'s bridge/pump-backed projections plus the free sound surface + the camera stub. */
const factoriesFromSim = (sim: Sim): SceneFactories => ({
  add: sim.add,
  cameras: CAMERAS,
  input: sim.input,
  physics: sim.physics,
  sound: SOUND,
  time: sim.time,
  tweens: sim.tweens,
});

/** A scene mounted onto a loop: the start hook, the per-tick update, and the preloaded asset keys. */
export interface MountedScene {
  /** Bind the start factories and run `preload` + `create` once (the loop calls this on the first frame). */
  readonly start: () => void;
  /** The per-tick `FixedUpdate` the loop schedules: bind the tick's factories, then run `update`. */
  readonly tick: FixedUpdate;
  /** The asset keys `preload` declared (empty until `start` has run). */
  readonly assets: () => readonly string[];
}

/*
 * Mount `scene` against the loop's `context`. `start` scopes the factories to a
 * tick-0 `Sim`, records the `preload` keys, and runs `create`; `tick` re-scopes the
 * factories to each running tick's `Sim` and runs `update`. Binding before every
 * hook is what makes a tick-scoped projection (input/time/tweens) read the correct
 * tick inside the author's `create`/`update`.
 */
export const mountScene = (scene: Scene, context: SimContext): MountedScene => {
  const loaded: { assets: readonly string[] } = { assets: [] };
  return {
    assets: (): readonly string[] => loaded.assets,
    start: (): void => {
      scene.bindFactories(factoriesFromSim(makeSim(context, FIRST_TICK)));
      loaded.assets = scene.preload();
      scene.create();
    },
    tick: (sim: Sim): void => {
      scene.bindFactories(factoriesFromSim(sim));
      scene.update(sim.tick, sim.dt);
    },
  };
};
