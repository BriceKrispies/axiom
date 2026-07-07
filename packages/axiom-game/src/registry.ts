/*
 * The callback registry behind the free `onFixedUpdate`/`onRender` authoring
 * functions (SPEC-00 §4.2) AND the KEYED system store the hot runtime reconciles
 * (hot-reload architecture §6.1). A `GameRegistry` collects the systems a manifest
 * declares; the loop core reads them back per phase.
 *
 * The store is a single insertion-ordered `Map<string, SystemDef>` keyed by each
 * system's STABLE id — the diff key. This is what makes a system REPLACEABLE: to
 * hot-patch a system's body, the reconciler `upsert`s the new def under the same id,
 * and `Map.set` on an existing key updates it IN PLACE (position preserved), so the
 * swap takes effect on the next tick with no re-ordering. `remove` drops one system
 * by id; the free `onFixedUpdate`/`onRender` remain, now thin adapters that `upsert`
 * a synthetic-id system so existing side-effect demos keep booting.
 *
 * SPEC-14 §9 per-game registry is unchanged: `createGame` mints a fresh registry and
 * installs it active (`useRegistry`); the free functions target whichever is active.
 * The active pointer lives in one mutable holder here, exactly as the bound host
 * lives in `host-binding.ts`'s `session`.
 */

import type { FixedSystemSpec, RenderSystemSpec, SystemDef } from "./manifest.ts";
import type { FixedUpdate, Render } from "./loop-core.ts";
import { orElse } from "./control-flow.ts";

/** A system def narrowed to the fixed-update phase — the projection target for `fixedUpdates()`. */
interface FixedSystemDef {
  readonly id: string;
  readonly spec: FixedSystemSpec;
}
/** A system def narrowed to the render phase — the projection target for `renders()`. */
interface RenderSystemDef {
  readonly id: string;
  readonly spec: RenderSystemSpec;
}

/** The order key a system without an explicit `order` sorts at — insertion order then breaks ties (stable sort). */
const DEFAULT_ORDER = 0;

/** Collects the ID-keyed systems for one game and projects them per phase for the loop. */
export class GameRegistry {
  readonly #systems = new Map<string, SystemDef>();
  #legacy = 0;

  /** Add or replace a system by its stable id (SPEC hot-reload §6.1). Replacing preserves position. */
  public upsert(def: SystemDef): void {
    this.#systems.set(def.id, def);
  }

  /** Remove a system by id (a stale id is a clean no-op). */
  public remove(id: string): void {
    this.#systems.delete(id);
  }

  /** The system currently mounted under `id`, or the empty value — the reconciler reads its `dispose` hook. */
  public get(id: string): SystemDef | undefined {
    return this.#systems.get(id);
  }

  /** Register a fixed-update callback (legacy free-function path): upserted under a synthetic id. */
  public onFixedUpdate(callback: FixedUpdate): void {
    this.#legacy += 1;
    this.upsert({ id: `legacy:fixed:${this.#legacy}`, spec: { phase: "fixedUpdate", run: callback } });
  }

  /** Register a render callback (legacy free-function path): upserted under a synthetic id. */
  public onRender(callback: Render): void {
    this.#legacy += 1;
    this.upsert({ id: `legacy:render:${this.#legacy}`, spec: { phase: "render", run: callback } });
  }

  /** The registered fixed-update callbacks, ordered by `order` then registration. */
  public fixedUpdates(): readonly FixedUpdate[] {
    return this.#ordered()
      .filter((def): def is FixedSystemDef => def.spec.phase === "fixedUpdate")
      .map((def) => def.spec.run);
  }

  /** The registered render callbacks, ordered by `order` then registration. */
  public renders(): readonly Render[] {
    return this.#ordered()
      .filter((def): def is RenderSystemDef => def.spec.phase === "render")
      .map((def) => def.spec.run);
  }

  /** All systems in insertion order, stably re-sorted by the optional `order` key. */
  #ordered(): readonly SystemDef[] {
    return [...this.#systems.values()].toSorted(
      (lhs, rhs) => orElse(lhs.spec.order, DEFAULT_ORDER) - orElse(rhs.spec.order, DEFAULT_ORDER),
    );
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
