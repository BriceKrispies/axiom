/*
 * The callback registry behind the free `onFixedUpdate`/`onRender` authoring
 * functions (SPEC-00 §4.2). A `GameRegistry` collects the fixed-update and render
 * callbacks an author registers; the loop core reads them back.
 *
 * SPEC-14 §9 fix — per-game registry. The free `onFixedUpdate`/`onRender` retain
 * their Phaser-style module-level shape, but they no longer push into one shared
 * singleton that `createGame` *resets*: instead each `createGame` mints its OWN
 * fresh `GameRegistry` and installs it as the ACTIVE registry (`useRegistry`), and
 * the free functions delegate to whichever registry is active. Two games created
 * in sequence therefore get independent registries — the first keeps its
 * registrations instead of being silently cleared — closing the "two live games
 * share the global" debt without changing the author-facing free-function surface.
 * The active pointer lives in one mutable holder here, exactly as the bound host
 * lives in `host-binding.ts`'s `session`.
 */

import type { FixedUpdate, Render } from "./loop-core.ts";

/** Collects the callbacks an author registers for one game. */
export class GameRegistry {
  readonly #fixedUpdates: FixedUpdate[] = [];
  readonly #renders: Render[] = [];

  /** Register a fixed-update callback (run 0..N times per frame at constant dt). */
  public onFixedUpdate(callback: FixedUpdate): void {
    this.#fixedUpdates.push(callback);
  }

  /** Register a render callback (run once per frame, presentation only). */
  public onRender(callback: Render): void {
    this.#renders.push(callback);
  }

  /** The registered fixed-update callbacks, in registration order. */
  public fixedUpdates(): readonly FixedUpdate[] {
    return this.#fixedUpdates;
  }

  /** The registered render callbacks, in registration order. */
  public renders(): readonly Render[] {
    return this.#renders;
  }
}

/*
 * The active registry the free authoring functions target. `createGame` swaps this
 * to each new game's fresh registry (a per-game registry, not a reset singleton).
 * One mutable holder, mirroring `host-binding.ts`'s `session.host`.
 */
const state: { active: GameRegistry } = { active: new GameRegistry() };

/** The registry the free `onFixedUpdate`/`onRender` currently target. */
export const activeRegistry = (): GameRegistry => state.active;

/** Install `registry` as the active one the free authoring functions target (called by `createGame`). */
export const useRegistry = (registry: GameRegistry): void => {
  state.active = registry;
};

/** Register a fixed update on the active registry (SPEC-00 §4.2 `onFixedUpdate`). */
export const onFixedUpdate = (callback: FixedUpdate): void => {
  state.active.onFixedUpdate(callback);
};

/** Register a render on the active registry (SPEC-00 §4.2 `onRender`). */
export const onRender = (callback: Render): void => {
  state.active.onRender(callback);
};
