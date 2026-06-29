// A FAKE NativeBridge for the projection tests — the @axiom/game analogue of
// @axiom/client's fake-socket.ts. No wasm: the fake owns a real in-memory ECS
// (so World tests assert genuine spawn/query/subtree behavior), FIFO queues for
// the scripted RNG draws (so Rng tests assert the projection reshapes the core's
// indices), and per-tick maps for the input snapshot. Tests are exempt from the
// Branchless Law, so this file uses ordinary control flow.

import type { BodyKind, NativeBridge, PointerSample, Swipe, TweenCurve } from "../src/native-bridge.ts";
import type {
  Component,
  ComponentKind,
  Entity,
  Handle,
  Result,
  Ticks,
  Vec2,
  Vec3,
} from "../src/vocabulary.ts";
import type { StepBudget } from "../src/step-budget.ts";

const FIRST_ENTITY = 1;

// A scripted in-memory timer schedule: one-shot fires on its due tick, repeating
// fires every interval after its start tick. Cancellation drops it from due.
type TimerEntry =
  | { kind: "after"; due: Ticks; cancelled: boolean }
  | { kind: "every"; start: Ticks; interval: Ticks; cancelled: boolean };

// A tween's schedule: linear progress over [start, start+durationTicks], eased by
// the index the projection chose. The easing table mirrors src/tweens.ts EASES.
interface TweenEntry {
  start: Ticks;
  from: number;
  to: number;
  durationTicks: Ticks;
  easeIndex: number;
  cancelled: boolean;
}

interface MachineEntry {
  current: number;
  enterTick: Ticks;
}

// The easing curves in the same dense order as src/tweens.ts EASES. Test-only, so
// these are simple closed forms (no ternary) — only `linear` is value-asserted in
// the tween test; the rest just need to exist at their index and be monotone.
const EASE_FNS: readonly ((p: number) => number)[] = [
  (p) => p,
  (p) => p * p,
  (p) => 1 - (1 - p) * (1 - p),
  (p) => p * p * (3 - 2 * p),
  (p) => 1 - (1 - p) ** 3,
  (p) => 1 - 2 ** (-10 * p),
  (p) => {
    const c = 1.701_58;
    return 1 + (c + 1) * (p - 1) ** 3 + c * (p - 1) ** 2;
  },
];

export class FakeBridge implements NativeBridge {
  // --- advance / snapshot ---
  public budgets: StepBudget[] = [];
  public snap: Uint8Array = Uint8Array.of();
  private budgetIndex = 0;

  // --- scripted RNG ---
  public units: number[] = [];
  public belows: number[] = [];
  public weightedIndices: number[] = [];
  public permutations: number[][] = [];
  public streamIds = new Map<string, number>();
  public streamCalls: (readonly [number, string])[] = [];
  public lastUnitStream: number | undefined = undefined;
  public lastBelow: { stream: number; maxExclusive: number } | undefined = undefined;
  public lastWeights: readonly number[] | undefined = undefined;
  private nextStreamId = 1000;

  // --- in-memory ECS ---
  private readonly alive = new Set<Entity>();
  private readonly columns = new Map<Entity, Map<string, Component>>();
  private readonly parents = new Map<Entity, Entity>();
  private readonly order: Entity[] = [];
  public lastSpawn: readonly Component[] | undefined = undefined;
  private nextEntity = FIRST_ENTITY;

  // --- input snapshot, keyed by tick ---
  public down = new Set<string>();
  public pressedEdges = new Set<string>();
  public releasedEdges = new Set<string>();
  public pointers = new Map<Ticks, PointerSample>();
  public pressedStarts = new Map<Ticks, Vec2>();
  public swipes = new Map<Ticks, Swipe>();
  public pressedAt = new Map<string, Ticks>();

  // --- tick-scheduled timers ---
  private readonly timers = new Map<Handle, TimerEntry>();
  private nextTimer = 1;

  // --- tick-driven state machines ---
  private readonly machines = new Map<Handle, MachineEntry>();
  private nextMachine = 1;

  // --- tick-sampled tweens ---
  private readonly tweens = new Map<Handle, TweenEntry>();
  private nextTween = 1;

  // --- physics call log ---
  public bodies: (readonly [Entity, BodyKind])[] = [];
  public config: readonly number[] | undefined = undefined;
  public impulses: (readonly [Handle, number, number, number])[] = [];
  public forces: (readonly [Handle, number, number, number])[] = [];
  public torques: (readonly [Handle, number, number, number])[] = [];
  public velocities: (readonly [Handle, number, number, number])[] = [];
  public angularVelocities: (readonly [Handle, number, number, number])[] = [];
  private nextBody = 1;

  public advance(): StepBudget {
    const fallback: StepBudget = { fixedStepNanos: 1, remainderNanos: 0, steps: 0 };
    const budget = this.budgets[this.budgetIndex] ?? fallback;
    this.budgetIndex += 1;
    return budget;
  }

  public snapshot(): Uint8Array {
    return this.snap;
  }

  public rngUnit(stream: number): number {
    this.lastUnitStream = stream;
    return this.units.shift() ?? 0;
  }

  public rngBelow(stream: number, maxExclusive: number): number {
    this.lastBelow = { maxExclusive, stream };
    return this.belows.shift() ?? 0;
  }

  public rngWeighted(_stream: number, weights: readonly number[]): number {
    this.lastWeights = weights;
    return this.weightedIndices.shift() ?? 0;
  }

  public rngPermutation(_stream: number, length: number): readonly number[] {
    return this.permutations.shift() ?? Array.from({ length }, (_unused, index) => index);
  }

  public rngStream(parent: number, name: string): number {
    this.streamCalls.push([parent, name]);
    const existing = this.streamIds.get(name);
    if (existing !== undefined) {
      return existing;
    }
    const minted = this.nextStreamId;
    this.nextStreamId += 1;
    this.streamIds.set(name, minted);
    return minted;
  }

  public worldSpawn(components: readonly Component[]): Entity {
    this.lastSpawn = components;
    const entity = this.nextEntity;
    this.nextEntity += 1;
    this.alive.add(entity);
    this.order.push(entity);
    const column = new Map<string, Component>();
    for (const component of components) {
      column.set(component.kind, component);
    }
    this.columns.set(entity, column);
    return entity;
  }

  public worldDespawn(entity: Entity): void {
    this.alive.delete(entity);
    this.columns.delete(entity);
    this.parents.delete(entity);
  }

  public worldDespawnSubtree(entity: Entity): void {
    const doomed = this.descendants(entity);
    doomed.push(entity);
    for (const target of doomed) {
      this.worldDespawn(target);
    }
  }

  public worldGet(entity: Entity, kind: ComponentKind): Result<Component> {
    const column = this.columns.get(entity);
    if (!this.alive.has(entity) || column === undefined) {
      return undefined;
    }
    return column.get(kind);
  }

  public worldSet(entity: Entity, value: Component): void {
    const column = this.columns.get(entity);
    if (this.alive.has(entity) && column !== undefined) {
      column.set(value.kind, value);
    }
  }

  public worldQuery(kinds: readonly ComponentKind[]): readonly Entity[] {
    return this.order.filter((entity) => {
      const column = this.columns.get(entity);
      return (
        this.alive.has(entity) && column !== undefined && kinds.every((kind) => column.has(kind))
      );
    });
  }

  public worldChildrenOf(entity: Entity): readonly Entity[] {
    return this.order.filter((candidate) => this.parents.get(candidate) === entity);
  }

  // Test helper: wire `child` under `parent` (the projected surface omits
  // `setParent`, so the fake establishes hierarchy for childrenOf/subtree tests).
  public link(child: Entity, parent: Entity): void {
    this.parents.set(child, parent);
  }

  private descendants(entity: Entity): Entity[] {
    const result: Entity[] = [];
    for (const candidate of this.order) {
      if (this.parents.get(candidate) === entity) {
        result.push(candidate);
        for (const deeper of this.descendants(candidate)) {
          result.push(deeper);
        }
      }
    }
    return result;
  }

  public inputIsDown(tick: Ticks, action: string): boolean {
    return this.down.has(`${tick}|${action}`);
  }

  public inputPressed(tick: Ticks, action: string): boolean {
    return this.pressedEdges.has(`${tick}|${action}`);
  }

  public inputReleased(tick: Ticks, action: string): boolean {
    return this.releasedEdges.has(`${tick}|${action}`);
  }

  public inputPointer(tick: Ticks): Result<PointerSample> {
    return this.pointers.get(tick);
  }

  public inputPointerPressed(tick: Ticks): Result<Vec2> {
    return this.pressedStarts.get(tick);
  }

  public inputSwipe(tick: Ticks): Result<Swipe> {
    return this.swipes.get(tick);
  }

  public inputPressedAtTick(tick: Ticks, action: string): Result<Ticks> {
    return this.pressedAt.get(`${tick}|${action}`);
  }

  // --- timers ---
  public timerAfter(tick: Ticks, delay: Ticks): Handle {
    const id = this.nextTimer;
    this.nextTimer += 1;
    this.timers.set(id, { cancelled: false, due: tick + delay, kind: "after" });
    return id;
  }

  public timerEvery(tick: Ticks, interval: Ticks): Handle {
    const id = this.nextTimer;
    this.nextTimer += 1;
    this.timers.set(id, { cancelled: false, interval, kind: "every", start: tick });
    return id;
  }

  public timerCancel(id: Handle): void {
    const entry = this.timers.get(id);
    if (entry !== undefined) {
      entry.cancelled = true;
    }
  }

  public timersDue(tick: Ticks): readonly Handle[] {
    const due: Handle[] = [];
    for (const [id, entry] of this.timers) {
      if (!entry.cancelled && entry.kind === "after" && entry.due === tick) {
        due.push(id);
      }
      if (
        !entry.cancelled &&
        entry.kind === "every" &&
        tick > entry.start &&
        (tick - entry.start) % entry.interval === 0
      ) {
        due.push(id);
      }
    }
    return due;
  }

  // --- state machines ---
  public machineCreate(tick: Ticks, _stateCount: number, initial: number): Handle {
    const id = this.nextMachine;
    this.nextMachine += 1;
    this.machines.set(id, { current: initial, enterTick: tick });
    return id;
  }

  public machineCurrent(id: Handle): number {
    return this.machines.get(id)!.current;
  }

  public machineTransition(id: Handle, tick: Ticks, to: number): void {
    const entry = this.machines.get(id)!;
    entry.current = to;
    entry.enterTick = tick;
  }

  public machineTicksInState(id: Handle, tick: Ticks): Ticks {
    return tick - this.machines.get(id)!.enterTick;
  }

  // --- tweens ---
  public tweenAdd(tick: Ticks, curve: TweenCurve): Handle {
    const id = this.nextTween;
    this.nextTween += 1;
    this.tweens.set(id, {
      cancelled: false,
      durationTicks: curve.durationTicks,
      easeIndex: curve.easeIndex,
      from: curve.from,
      start: tick,
      to: curve.to,
    });
    return id;
  }

  public tweenCancel(id: Handle): void {
    const entry = this.tweens.get(id);
    if (entry !== undefined) {
      entry.cancelled = true;
    }
  }

  public tweenActive(tick: Ticks): readonly Handle[] {
    const active: Handle[] = [];
    for (const [id, entry] of this.tweens) {
      if (!entry.cancelled && tick > entry.start && tick <= entry.start + entry.durationTicks) {
        active.push(id);
      }
    }
    return active;
  }

  public tweenValue(id: Handle, tick: Ticks): number {
    const entry = this.tweens.get(id)!;
    const raw = (tick - entry.start) / entry.durationTicks;
    const progress = Math.max(0, Math.min(1, raw));
    const eased = EASE_FNS[entry.easeIndex]!(progress);
    return entry.from + (entry.to - entry.from) * eased;
  }

  public tweenCompleted(tick: Ticks): readonly Handle[] {
    const completed: Handle[] = [];
    for (const [id, entry] of this.tweens) {
      if (!entry.cancelled && tick === entry.start + entry.durationTicks) {
        completed.push(id);
      }
    }
    return completed;
  }

  // --- physics ---
  public physicsSetConfig(gravity: Vec3, linearDamping: number, angularDamping: number): void {
    this.config = [gravity.x, gravity.y, gravity.z, linearDamping, angularDamping];
  }

  public physicsAddBody(entity: Entity, kind: BodyKind): Handle {
    const id = this.nextBody;
    this.nextBody += 1;
    this.bodies.push([entity, kind]);
    return id;
  }

  public physicsApplyImpulse(body: Handle, impulse: Vec3): void {
    this.impulses.push([body, impulse.x, impulse.y, impulse.z]);
  }

  public physicsApplyForce(body: Handle, force: Vec3): void {
    this.forces.push([body, force.x, force.y, force.z]);
  }

  public physicsApplyTorque(body: Handle, torque: Vec3): void {
    this.torques.push([body, torque.x, torque.y, torque.z]);
  }

  public physicsSetVelocity(body: Handle, velocity: Vec3): void {
    this.velocities.push([body, velocity.x, velocity.y, velocity.z]);
  }

  public physicsSetAngularVelocity(body: Handle, velocity: Vec3): void {
    this.angularVelocities.push([body, velocity.x, velocity.y, velocity.z]);
  }
}
