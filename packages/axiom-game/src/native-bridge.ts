/*
 * The shape of the native (wasm) runtime the loop and the per-tick `Sim`
 * projections drive — the deterministic core's only contact with
 * `apps/axiom-game-runtime`'s `WasmGame` exports. The pure spine depends on this
 * interface, never on a live wasm module, so every projection's tests inject a
 * FAKE bridge (no wasm needed) and stay fully covered — exactly as @axiom/client
 * tests its codec against a fake socket. The platform edge (`raf-loop.ts`)
 * adapts the real `WasmGame` to this interface.
 *
 * The methods below are the seam the runtime app must implement in wasm. The
 * deterministic *decisions* live native-side: the RNG draw sequence, the ECS
 * columns and their stable iteration order, and each tick's input snapshot. The
 * TS projections (`Rng`, `World`, `Input`) only reshape those primitive results
 * into the authoring surface (SPEC-01/02/05 §4.2) — they reorder the author's
 * own arrays using the *indices* the native core chose, never re-deciding them.
 */

import type { Component, ComponentKind, Entity, Result, Ticks, Vec2 } from "./vocabulary.ts";
import type { StepBudget } from "./step-budget.ts";

/** A pointer sample for one tick: position plus pressed state (SPEC-05). */
export interface PointerSample {
  readonly pos: Vec2;
  readonly down: boolean;
}

/** A flick gesture direction over the input snapshot (SPEC-05). */
export type Swipe = "up" | "down" | "left" | "right";

/** The native fixed-step runtime, as the loop core and the `Sim` projections see it. */
export interface NativeBridge {
  /** Bank `elapsedNanos` of real time and report the resulting integer step budget. */
  readonly advance: (elapsedNanos: number) => StepBudget;
  /** The durable simulation state as opaque bytes (for checkpoint / determinism checks). */
  readonly snapshot: () => Uint8Array;

  // Deterministic RNG (SPEC-01): the native core owns the draw sequence and the projection turns these primitives into the author surface.
  /** A uniform float in `[0, 1)` from `stream`. */
  readonly rngUnit: (stream: number) => number;
  /** A uniform integer in `[0, maxExclusive)` from `stream`. */
  readonly rngBelow: (stream: number, maxExclusive: number) => number;
  /** The index `weights` selects, drawn proportionally to the weights, from `stream`. */
  readonly rngWeighted: (stream: number, weights: readonly number[]) => number;
  /** A Fisher-Yates permutation of `[0, length)` the core drew from `stream`. */
  readonly rngPermutation: (stream: number, length: number) => readonly number[];
  /** Resolve the deterministic id of the named sub-stream of `parent`. */
  readonly rngStream: (parent: number, name: string) => number;

  // Retained ECS world (SPEC-02): entities/components/queries/hierarchy.
  /** Spawn an entity carrying `components`, returning its handle. */
  readonly worldSpawn: (components: readonly Component[]) => Entity;
  /** Despawn one entity (a stale handle is a clean no-op). */
  readonly worldDespawn: (entity: Entity) => void;
  /** Despawn an entity and its whole subtree (scene `despawn_subtree`). */
  readonly worldDespawnSubtree: (entity: Entity) => void;
  /** Read a component, or the empty value on a miss / dead entity. */
  readonly worldGet: (entity: Entity, kind: ComponentKind) => Result<Component>;
  /** Add or replace a component on `entity` (a stale handle is a clean no-op). */
  readonly worldSet: (entity: Entity, value: Component) => void;
  /** Entities having every named kind, in stable ascending-id order. */
  readonly worldQuery: (kinds: readonly ComponentKind[]) => readonly Entity[];
  /** The direct children of `entity`, in stable order (scene `children_of`). */
  readonly worldChildrenOf: (entity: Entity) => readonly Entity[];

  // Input snapshot (SPEC-05): every read is scoped to a tick's snapshot.
  /** Whether `action` is held at `tick`. */
  readonly inputIsDown: (tick: Ticks, action: string) => boolean;
  /** Whether `action` went down on `tick` (edge). */
  readonly inputPressed: (tick: Ticks, action: string) => boolean;
  /** Whether `action` went up on `tick` (edge). */
  readonly inputReleased: (tick: Ticks, action: string) => boolean;
  /** The pointer sample at `tick`, or `null` when there is no pointer. */
  readonly inputPointer: (tick: Ticks) => Result<PointerSample>;
  /** The position a pointer-press began at on `tick`, or `null`. */
  readonly inputPointerPressed: (tick: Ticks) => Result<Vec2>;
  /** The flick gesture committed on `tick`, or `null`. */
  readonly inputSwipe: (tick: Ticks) => Result<Swipe>;
  /** The tick `action` was most recently pressed at, or `null` if never. */
  readonly inputPressedAtTick: (tick: Ticks, action: string) => Result<Ticks>;
}
