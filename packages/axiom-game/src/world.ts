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

import type { Component, ComponentKind, Entity, Result, Transform } from "./vocabulary.ts";
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
  /** Whether `entity` names a live node (a stale handle is `false`). */
  readonly alive: (entity: Entity) => boolean;
  /** Whether `entity` carries a component of `kind`. */
  readonly has: (entity: Entity, kind: ComponentKind) => boolean;
  /** Remove a component from `entity` (a stale handle / absent component is a clean no-op). */
  readonly remove: (entity: Entity, kind: ComponentKind) => void;
  /** Re-parent `child` under `parent` (a self-parent / cycle / stale handle is a clean no-op). */
  readonly setParent: (child: Entity, parent: Entity) => void;
  /** `entity`'s parent, or the empty value at a root / on a stale handle. */
  readonly parentOf: (entity: Entity) => Result<Entity>;
  /** `entity`'s resolved (composed) world transform this tick, or the empty value on a stale handle. */
  readonly worldTransform: (entity: Entity) => Result<Transform>;
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

  public alive(entity: Entity): boolean {
    return this.#bridge.worldAlive(entity);
  }

  public has(entity: Entity, kind: ComponentKind): boolean {
    return this.#bridge.worldHas(entity, kind);
  }

  public remove(entity: Entity, kind: ComponentKind): void {
    this.#bridge.worldRemove(entity, kind);
  }

  public setParent(child: Entity, parent: Entity): void {
    this.#bridge.worldSetParent(child, parent);
  }

  public parentOf(entity: Entity): Result<Entity> {
    return this.#bridge.worldParentOf(entity);
  }

  public worldTransform(entity: Entity): Result<Transform> {
    return this.#bridge.worldWorldTransform(entity);
  }
}

/** Build the `World` projection over `bridge` (SPEC-02 §4.2). */
export const makeWorld = (bridge: NativeBridge): World => new BridgeWorld(bridge);
