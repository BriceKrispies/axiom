/*
 * The retained-ECS world projection (SPEC-02 §4.2). `World` is the author
 * surface over the native entity/component store: spawning, component get/set,
 * queries, and the scene hierarchy. The native core owns the columns and their
 * stable ascending-id iteration order (`entities_with*`, `children_of`,
 * `despawn_subtree`); this projection adapts the variadic author calls
 * (`spawn(...components)`, `query(...kinds)`) onto the bridge's array seam and
 * forwards the rest.
 *
 * `World` is reached only through `Sim.world` (SPEC-00 §4.2) — there is no free
 * `World` constructor for an author; the engine owns the world. A stale handle
 * passed to `get`/`set`/`despawn` is a clean no-op / `null`, never a throw
 * (SPEC-02 §5).
 */

import type { Component, ComponentKind, Entity, Result } from "./vocabulary.ts";
import type { NativeBridge } from "./native-bridge.ts";

/** The entity/component/query/hierarchy surface for the running tick (SPEC-02 §4.2). */
export interface World {
  /** Spawn an entity carrying `components`, returning its handle. */
  readonly spawn: (...components: Component[]) => Entity;
  /** Despawn one entity (a stale handle is a clean no-op). */
  readonly despawn: (entity: Entity) => void;
  /** Despawn an entity and its whole subtree. */
  readonly despawnSubtree: (entity: Entity) => void;
  /** Read a component (author narrows on `kind`), or the empty value on a miss. */
  readonly get: (entity: Entity, kind: ComponentKind) => Result<Component>;
  /** Add or replace a component (a stale handle is a clean no-op). */
  readonly set: (entity: Entity, value: Component) => void;
  /** Entities having every `kind`, in stable ascending-id order. */
  readonly query: (...kinds: ComponentKind[]) => readonly Entity[];
  /** The direct children of `entity`, in stable order. */
  readonly childrenOf: (entity: Entity) => readonly Entity[];
}

/** The `World` projection bound to the native store. */
export class BridgeWorld implements World {
  readonly #bridge: NativeBridge;

  public constructor(bridge: NativeBridge) {
    this.#bridge = bridge;
  }

  public spawn(...components: Component[]): Entity {
    return this.#bridge.worldSpawn(components);
  }

  public despawn(entity: Entity): void {
    this.#bridge.worldDespawn(entity);
  }

  public despawnSubtree(entity: Entity): void {
    this.#bridge.worldDespawnSubtree(entity);
  }

  public get(entity: Entity, kind: ComponentKind): Result<Component> {
    return this.#bridge.worldGet(entity, kind);
  }

  public set(entity: Entity, value: Component): void {
    this.#bridge.worldSet(entity, value);
  }

  public query(...kinds: ComponentKind[]): readonly Entity[] {
    return this.#bridge.worldQuery(kinds);
  }

  public childrenOf(entity: Entity): readonly Entity[] {
    return this.#bridge.worldChildrenOf(entity);
  }
}

/** Build the `World` projection over `bridge` (SPEC-02 §4.2). */
export const makeWorld = (bridge: NativeBridge): World => new BridgeWorld(bridge);
