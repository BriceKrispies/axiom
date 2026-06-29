// A FAKE NativeBridge for the projection tests — the @axiom/game analogue of
// @axiom/client's fake-socket.ts. No wasm: the fake owns a real in-memory ECS
// (so World tests assert genuine spawn/query/subtree behavior), FIFO queues for
// the scripted RNG draws (so Rng tests assert the projection reshapes the core's
// indices), and per-tick maps for the input snapshot. Tests are exempt from the
// Branchless Law, so this file uses ordinary control flow.

import type { NativeBridge, PointerSample, Swipe } from "../src/native-bridge.ts";
import type { Component, ComponentKind, Entity, Result, Ticks, Vec2 } from "../src/vocabulary.ts";
import type { StepBudget } from "../src/step-budget.ts";

const FIRST_ENTITY = 1;

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
}
