// A FAKE NativeBridge for the projection tests — the @axiom/game analogue of
// @axiom/client's fake-socket.ts. No wasm: the fake owns a real in-memory ECS
// (so World tests assert genuine spawn/query/subtree behavior), FIFO queues for
// the scripted RNG draws (so Rng tests assert the projection reshapes the core's
// indices), and per-tick maps for the input snapshot. Tests are exempt from the
// Branchless Law, so this file uses ordinary control flow.

import type { BodyKind, NativeBridge, PointerSample, Swipe, TweenCurve } from "./native-bridge.ts";
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

// The identity world transform the fake returns for a live node with no scripted
// override — the obvious neutral pose for a hierarchy/transform read test.
const IDENTITY_TRANSFORM: Transform = {
  position: { x: 0, y: 0, z: 0 },
  rotation: [0, 0, 0, 1],
  scale: { x: 1, y: 1, z: 1 },
};

// A `Transform`-bearing component (the built-in `Transform` the World tests `set`
// onto a node). The fake reads these fields back when composing a node's world
// transform, so `worldTransform` is a genuine function of the parent chain — not a
// scripted constant. Test-only, so a structural type guard is fine.
interface TransformComponent extends Component {
  readonly position: Vec3;
  readonly rotation: Transform["rotation"];
  readonly scale: Vec3;
}

const isTransformComponent = (component: Component): component is TransformComponent =>
  "position" in component && "rotation" in component && "scale" in component;

// Hamilton product of two quaternions `[x, y, z, w]` — the rotation half of a TRS
// compose. With identity rotations it is identity, so a translate/scale-only test
// reads cleanly while a rotated hierarchy still composes faithfully.
const quatMul = (a: Transform["rotation"], b: Transform["rotation"]): Transform["rotation"] => [
  a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
  a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
  a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
  a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
];

// Compose a child's LOCAL transform under its PARENT's resolved world transform:
// translation accumulates with the parent's scale applied to the child offset,
// scale multiplies componentwise, and rotation is the quaternion product — the
// same TRS hierarchy the native scene resolves.
const composeTransform = (parent: Transform, child: Transform): Transform => ({
  position: {
    x: parent.position.x + parent.scale.x * child.position.x,
    y: parent.position.y + parent.scale.y * child.position.y,
    z: parent.position.z + parent.scale.z * child.position.z,
  },
  rotation: quatMul(parent.rotation, child.rotation),
  scale: {
    x: parent.scale.x * child.scale.x,
    y: parent.scale.y * child.scale.y,
    z: parent.scale.z * child.scale.z,
  },
});

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
  // Scriptable world-transform returns: a live entity with no override reads the
  // identity pose; a dead entity is the empty Result.
  public readonly transforms = new Map<Entity, Transform>();
  private nextEntity = FIRST_ENTITY;

  // --- input snapshot, keyed by tick ---
  public down = new Set<string>();
  public pressedEdges = new Set<string>();
  public releasedEdges = new Set<string>();
  public pointers = new Map<Ticks, PointerSample>();
  public pressedStarts = new Map<Ticks, Vec2>();
  public swipes = new Map<Ticks, Swipe>();
  public looks = new Map<Ticks, Vec2>();
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
    const retro_fpsed = this.descendants(entity);
    retro_fpsed.push(entity);
    for (const target of retro_fpsed) {
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

  public worldAlive(entity: Entity): boolean {
    return this.alive.has(entity);
  }

  public worldHas(entity: Entity, kind: ComponentKind): boolean {
    const column = this.columns.get(entity);
    if (!this.alive.has(entity) || column === undefined) {
      return false;
    }
    return column.has(kind);
  }

  public worldRemove(entity: Entity, kind: ComponentKind): void {
    const column = this.columns.get(entity);
    if (this.alive.has(entity) && column !== undefined) {
      column.delete(kind);
    }
  }

  public worldSetParent(child: Entity, parent: Entity): void {
    // Self-parenting is rejected (mirrors the native scene), like `link` otherwise.
    if (child !== parent) {
      this.parents.set(child, parent);
    }
  }

  public worldParentOf(entity: Entity): Result<Entity> {
    return this.parents.get(entity);
  }

  public worldWorldTransform(entity: Entity): Result<Transform> {
    if (!this.alive.has(entity)) {
      return undefined;
    }
    return this.resolveWorld(entity);
  }

  // A node's LOCAL transform: a scripted override wins (the existing transform
  // test), else its `Transform` component if one was `set`, else identity.
  private localTransform(entity: Entity): Transform {
    const scripted = this.transforms.get(entity);
    if (scripted !== undefined) {
      return scripted;
    }
    const column = this.columns.get(entity);
    if (column !== undefined) {
      const component = column.get("transform");
      if (component !== undefined && isTransformComponent(component)) {
        return { position: component.position, rotation: component.rotation, scale: component.scale };
      }
    }
    return IDENTITY_TRANSFORM;
  }

  // Resolve a node's WORLD transform by composing its local transform under its
  // (live) parent chain — so a child reads its parent's pose, the composed value.
  private resolveWorld(entity: Entity): Transform {
    const local = this.localTransform(entity);
    const parent = this.parents.get(entity);
    if (parent === undefined || !this.alive.has(parent)) {
      return local;
    }
    return composeTransform(this.resolveWorld(parent), local);
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

  public inputLookDelta(tick: Ticks): Vec2 {
    return this.looks.get(tick) ?? { x: 0, y: 0 };
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
