/*
 * The callback registry behind the free `onFixedUpdate`/`onRender` authoring
 * functions (SPEC-00 §4.2). A `GameRegistry` collects the fixed-update and render
 * callbacks an author registers; the loop core reads them back. The module-level
 * default registry backs the Phaser-style free functions; tests use a fresh
 * `GameRegistry` for isolation. (Module-global registration is a deliberate M0
 * simplification — see SPEC-14 §9.)
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

  /** Drop every registration — used when a fresh game starts over the global registry. */
  public reset(): void {
    this.#fixedUpdates.length = 0;
    this.#renders.length = 0;
  }
}

/** The default registry the free authoring functions target. */
export const defaultRegistry = new GameRegistry();

/** Register a fixed update on the default registry (SPEC-00 §4.2 `onFixedUpdate`). */
export const onFixedUpdate = (callback: FixedUpdate): void => {
  defaultRegistry.onFixedUpdate(callback);
};

/** Register a render on the default registry (SPEC-00 §4.2 `onRender`). */
export const onRender = (callback: Render): void => {
  defaultRegistry.onRender(callback);
};
