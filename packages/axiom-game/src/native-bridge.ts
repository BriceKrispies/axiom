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

import type {
  Component,
  ComponentKind,
  Entity,
  Handle,
  Result,
  Ticks,
  Transform,
  Vec2,
  Vec3,
} from "./vocabulary.ts";
import type { StepBudget } from "./step-budget.ts";

/** A rigid-body kind the native physics core integrates (SPEC-10). */
export type BodyKind = "dynamic" | "kinematic" | "static";

/** A tween's native curve: endpoints, whole-tick duration, and dense ease index (SPEC-09). */
export interface TweenCurve {
  readonly from: number;
  readonly to: number;
  readonly durationTicks: Ticks;
  readonly easeIndex: number;
}

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
  /** Whether `entity` names a live node (a stale handle is `false`). */
  readonly worldAlive: (entity: Entity) => boolean;
  /** Whether `entity` carries a component of `kind` (a dead entity / unknown kind is `false`). */
  readonly worldHas: (entity: Entity, kind: ComponentKind) => boolean;
  /** Remove `entity`'s component of `kind` (a stale handle / absent component is a clean no-op). */
  readonly worldRemove: (entity: Entity, kind: ComponentKind) => void;
  /** Re-parent `child` under `parent`, or detach it to the root when `parent` is omitted (a self-parent / cycle / stale handle is a clean no-op). */
  readonly worldSetParent: (child: Entity, parent?: Entity) => void;
  /** `entity`'s parent, or the empty value at a root / on a stale handle. */
  readonly worldParentOf: (entity: Entity) => Result<Entity>;
  /** `entity`'s resolved (composed) world transform for this tick, or the empty value on a stale handle. */
  readonly worldWorldTransform: (entity: Entity) => Result<Transform>;

  // Input snapshot (SPEC-05): every read is scoped to a tick's snapshot.
  /** Whether `action` is held at `tick`. */
  readonly inputIsDown: (tick: Ticks, action: string) => boolean;
  /** Whether `action` went down on `tick` (edge). */
  readonly inputPressed: (tick: Ticks, action: string) => boolean;
  /** Whether `action` went up on `tick` (edge). */
  readonly inputReleased: (tick: Ticks, action: string) => boolean;
  /** This tick's relative look (mouse / pointer-lock) as a raw-pixel `(dx, dy)` delta — `(0, 0)` when none. */
  readonly inputLookDelta: (tick: Ticks) => Vec2;
  /** The pointer sample at `tick`, or `null` when there is no pointer. */
  readonly inputPointer: (tick: Ticks) => Result<PointerSample>;
  /** The position a pointer-press began at on `tick`, or `null`. */
  readonly inputPointerPressed: (tick: Ticks) => Result<Vec2>;
  /** The flick gesture committed on `tick`, or `null`. */
  readonly inputSwipe: (tick: Ticks) => Result<Swipe>;
  /** The tick `action` was most recently pressed at, or `null` if never. */
  readonly inputPressedAtTick: (tick: Ticks, action: string) => Result<Ticks>;

  // Tick-scheduled callbacks (SPEC-07): the native TickApi owns the schedule and reports the due ids each tick; the TS pump holds the author closures.
  /** Schedule a one-shot timer registered at `tick`, due `delay` ticks later; return its id. */
  readonly timerAfter: (tick: Ticks, delay: Ticks) => Handle;
  /** Schedule a repeating timer registered at `tick`, firing every `interval` ticks; return its id. */
  readonly timerEvery: (tick: Ticks, interval: Ticks) => Handle;
  /** Cancel a timer so it never fires again (a stale id is a clean no-op). */
  readonly timerCancel: (id: Handle) => void;
  /** The timer ids due to fire on `tick`, in stable schedule order. */
  readonly timersDue: (tick: Ticks) => readonly Handle[];

  // Tick-driven state machine (SPEC-07): dense state indices, entry-tick tracked native-side.
  /** Create a machine of `stateCount` states starting in `initial`, entered at `tick`; return its id. */
  readonly machineCreate: (tick: Ticks, stateCount: number, initial: number) => Handle;
  /** The current dense state index of machine `id`. */
  readonly machineCurrent: (id: Handle) => number;
  /** Move machine `id` to state `to`, recording `tick` as the new entry tick. */
  readonly machineTransition: (id: Handle, tick: Ticks, to: number) => void;
  /** How many ticks machine `id` has been in its current state as of `tick`. */
  readonly machineTicksInState: (id: Handle, tick: Ticks) => Ticks;

  // Tick-sampled tweens (SPEC-09): the native TweenApi owns the eased curve.
  /** Add a tween from its curve, registered at `tick`; return its id. */
  readonly tweenAdd: (tick: Ticks, curve: TweenCurve) => Handle;
  /** Cancel a tween so it stops sampling (a stale id is a clean no-op). */
  readonly tweenCancel: (id: Handle) => void;
  /** The tween ids to sample on `tick`, in stable schedule order. */
  readonly tweenActive: (tick: Ticks) => readonly Handle[];
  /** The eased value of tween `id` at `tick`. */
  readonly tweenValue: (id: Handle, tick: Ticks) => number;
  /** The tween ids that reach their end on `tick`, in stable schedule order. */
  readonly tweenCompleted: (tick: Ticks) => readonly Handle[];

  // Physics bodies (SPEC-10): a body wraps an entity; impulses/forces are native-side.
  /** Set the physics world config: gravity vector plus linear/angular damping ratios. */
  readonly physicsSetConfig: (gravity: Vec3, linearDamping: number, angularDamping: number) => void;
  /** Attach a `kind` body to `entity`; return the body handle. */
  readonly physicsAddBody: (entity: Entity, kind: BodyKind) => Handle;
  /** Apply an instantaneous impulse to `body`. */
  readonly physicsApplyImpulse: (body: Handle, impulse: Vec3) => void;
  /** Apply a continuous force to `body`. */
  readonly physicsApplyForce: (body: Handle, force: Vec3) => void;
  /** Apply a torque to `body` (SPEC-10 angular). */
  readonly physicsApplyTorque: (body: Handle, torque: Vec3) => void;
  /** Set `body`'s linear velocity. */
  readonly physicsSetVelocity: (body: Handle, velocity: Vec3) => void;
  /** Set `body`'s angular velocity. */
  readonly physicsSetAngularVelocity: (body: Handle, velocity: Vec3) => void;
}
