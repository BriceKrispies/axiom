/*
 * The platform edge: the requestAnimationFrame + performance.now() impure driver
 * and the real wasm bridge load. This is @axiom/game's analogue of @axiom/client's
 * webtransport.ts and the Rust spine's host/windowing layers — it binds browser
 * APIs and the live wasm module, so a documented subset of rules (the branch ban,
 * async/await, optional chaining, no-unsafe-*) is scoped off here and it is
 * coverage-exempt (browser-only; verified via the Playwright path) — see its
 * .oxlintrc.json override and the --test-coverage-exclude in package.json.
 *
 * Everything deterministic lives behind this edge: the GameLoop + stepFrame core
 * is pure and fully covered; here we only measure real elapsed time and load wasm.
 * Because the branch ban is off here, this file uses ordinary `if` control flow.
 */

import type { Component, Result } from "./vocabulary.ts";
import { each, pick } from "./control-flow.ts";
import type { GameLoop } from "./game-loop.ts";
import type { NativeBridge } from "./native-bridge.ts";
import type { StepBudget } from "./step-budget.ts";

/*
 * The raw `WasmGame` exports `apps/axiom-game-runtime` produces. `advance` adapts
 * its snake_case bigint `StepReport`; the retained-world methods (SPEC-02) speak
 * the wasm boundary's `(kind, fields-bytes)` convention rather than the
 * `NativeBridge`'s `Component` objects, so this edge runs the component codec
 * below over them (entities cross as plain numbers). The rng / input / timer /
 * tween / physics methods already match the `NativeBridge` shape, so they forward
 * unchanged.
 */
export interface WasmGameExport
  extends Omit<
    NativeBridge,
    | "advance"
    | "worldSpawn"
    | "worldDespawn"
    | "worldDespawnSubtree"
    | "worldGet"
    | "worldSet"
    | "worldQuery"
    | "worldChildrenOf"
  > {
  readonly advance: (elapsedNanos: bigint) => {
    readonly fixed_step_nanos: bigint;
    readonly remainder_nanos: bigint;
    readonly steps: number;
  };
  readonly worldSpawn: () => number;
  readonly worldDespawn: (entity: number) => void;
  readonly worldDespawnSubtree: (entity: number) => void;
  readonly worldGet: (entity: number, kind: string) => Uint8Array;
  readonly worldSet: (entity: number, kind: string, fields: Uint8Array) => void;
  readonly worldQuery: (kinds: readonly string[]) => readonly number[];
  readonly worldChildrenOf: (entity: number) => readonly number[];
}

/*
 * The component marshalling convention (the load-bearing decision every later
 * subsystem reuses), TS half. A `Component` crosses the wasm boundary as a
 * `(kind: string, fields: Uint8Array)` pair: `kind` is the discriminant the
 * native `world.rs` routes on (== the `Reflect` schema name), and `fields` are
 * exactly the kernel `Reflect` wire bytes for that kind — little-endian scalars,
 * a string as a u32-LE length prefix then UTF-8. This table mirrors the closed
 * vocabulary in `apps/axiom-game-runtime/src/world.rs`; the field ORDER must match
 * each Rust `reflect_write`. `worldGet`'s empty buffer (a miss) maps to the empty
 * `Result`. This codec lives at the platform edge because it is browser-bound
 * glue, exactly like @axiom/client's transport codecs.
 */
type FieldType = "f32" | "u32" | "string";
interface FieldSpec {
  readonly key: string;
  readonly type: FieldType;
}
const COMPONENT_FIELDS: Readonly<Record<string, readonly FieldSpec[]>> = {
  Image: [{ key: "texture", type: "string" }],
  Rectangle: [
    { key: "width", type: "f32" },
    { key: "height", type: "f32" },
    { key: "color", type: "u32" },
  ],
  Sprite: [{ key: "texture", type: "string" }],
  Text: [{ key: "value", type: "string" }],
  Transform: [
    { key: "x", type: "f32" },
    { key: "y", type: "f32" },
    { key: "rotation", type: "f32" },
    { key: "scaleX", type: "f32" },
    { key: "scaleY", type: "f32" },
  ],
  Velocity: [
    { key: "x", type: "f32" },
    { key: "y", type: "f32" },
  ],
};

/** The byte width of an f32 / u32 scalar in the wire format. */
const SCALAR_BYTES = 4;

/** The absent `Result` value, materialized without the lint-banned `undefined` literal. */
const absent = <Value>(slot?: Value): Value | undefined => slot;

/** A scalar's little-endian bytes, produced by `write` into a fresh 4-byte view. */
const scalarBytes = (write: (view: DataView) => void): readonly number[] => {
  const view = new DataView(new ArrayBuffer(SCALAR_BYTES));
  write(view);
  return [...new Uint8Array(view.buffer)];
};

/** A field value's wire bytes, by field type. */
const ENCODE: Readonly<Record<FieldType, (value: unknown) => readonly number[]>> = {
  f32: (value): readonly number[] =>
    scalarBytes((view): void => {
      view.setFloat32(0, value as number, true);
    }),
  string: (value): readonly number[] => {
    const utf8 = [...new TextEncoder().encode(value as string)];
    const prefix = scalarBytes((view): void => {
      view.setUint32(0, utf8.length, true);
    });
    return [...prefix, ...utf8];
  },
  u32: (value): readonly number[] =>
    scalarBytes((view): void => {
      view.setUint32(0, value as number, true);
    }),
};

/** Encode a `Component` to its field bytes (an unknown kind yields empty bytes). */
const encodeComponent = (component: Component): Uint8Array => {
  const record = component as unknown as Readonly<Record<string, unknown>>;
  const fields = COMPONENT_FIELDS[component.kind] ?? [];
  return Uint8Array.from(fields.flatMap((field): readonly number[] => ENCODE[field.type](record[field.key])));
};

/** Read a scalar from `bytes` at `offset`, returning `[value, nextOffset]`. */
const readScalar = (
  bytes: Uint8Array,
  offset: number,
  read: (view: DataView) => number,
): readonly [number, number] => {
  const view = new DataView(bytes.buffer, bytes.byteOffset + offset, SCALAR_BYTES);
  return [read(view), offset + SCALAR_BYTES];
};

/** Read one field value (by type) from `bytes` at `offset`, returning `[value, nextOffset]`. */
const DECODE: Readonly<
  Record<FieldType, (bytes: Uint8Array, offset: number) => readonly [unknown, number]>
> = {
  f32: (bytes, offset): readonly [unknown, number] =>
    readScalar(bytes, offset, (view): number => view.getFloat32(0, true)),
  string: (bytes, offset): readonly [unknown, number] => {
    const [length, afterLen] = readScalar(bytes, offset, (view): number => view.getUint32(0, true));
    const end = afterLen + length;
    return [new TextDecoder().decode(bytes.subarray(afterLen, end)), end];
  },
  u32: (bytes, offset): readonly [unknown, number] =>
    readScalar(bytes, offset, (view): number => view.getUint32(0, true)),
};

/*
 * Decode the field specs of `kind` over `bytes` into the `Component`'s entries. A
 * single mutable `cursor` walks the buffer as `.map` visits each field (strings
 * are variable-length, so offsets cannot be precomputed) — a local scan, not
 * control flow.
 */
const decodeFields = (fields: readonly FieldSpec[], kind: string, bytes: Uint8Array): Component => {
  const cursor = { offset: 0 };
  const entries = fields.map((field): readonly [string, unknown] => {
    const [value, next] = DECODE[field.type](bytes, cursor.offset);
    cursor.offset = next;
    return [field.key, value];
  });
  return Object.fromEntries([["kind", kind], ...entries]) as unknown as Component;
};

/*
 * Decode `(kind, bytes)` back to a `Component`; an empty buffer (a miss / dead
 * entity) or an unknown kind is the empty `Result`. Selection is the branchless
 * `pick` over two thunks so the decode body never runs on empty bytes.
 */
const decodeComponent = (kind: string, bytes: Uint8Array): Result<Component> => {
  const fields = COMPONENT_FIELDS[kind] ?? [];
  const present = Number(bytes.byteLength > 0 && fields.length > 0);
  return pick<() => Result<Component>>(
    [(): Result<Component> => absent<Component>(), (): Result<Component> => decodeFields(fields, kind, bytes)],
    present,
  )();
};

/** Adapt the snake_case wasm `advance` (snake_case bigint `StepReport`) to the camelCase `StepBudget`. */
const adaptAdvance =
  (game: WasmGameExport) =>
  (elapsedNanos: number): StepBudget => {
    const report = game.advance(BigInt(Math.round(elapsedNanos)));
    return {
      fixedStepNanos: Number(report.fixed_step_nanos),
      remainderNanos: Number(report.remainder_nanos),
      steps: report.steps,
    };
  };

/** Adapt `worldGet`: decode the wasm `(kind, bytes)` read back to a `Result<Component>`. */
const adaptWorldGet =
  (game: WasmGameExport) =>
  (entity: number, kind: string): Result<Component> =>
    decodeComponent(kind, game.worldGet(entity, kind));

/** Adapt `worldSet`: encode the `Component` to `(kind, fields-bytes)` for the wasm store. */
const adaptWorldSet =
  (game: WasmGameExport) =>
  (entity: number, value: Component): void => {
    game.worldSet(entity, value.kind, encodeComponent(value));
  };

/** Adapt `worldSpawn`: spawn an empty entity, then `worldSet` each component onto it. */
const adaptWorldSpawn =
  (game: WasmGameExport) =>
  (components: readonly Component[]): number => {
    const entity = game.worldSpawn();
    each(components, (component): void => {
      game.worldSet(entity, component.kind, encodeComponent(component));
    });
    return entity;
  };

/** Adapt the snake_case wasm `WasmGame` to the loop core's camelCase NativeBridge. */
export const bridgeFromWasm = (game: WasmGameExport): NativeBridge => ({
  advance: adaptAdvance(game),
  inputIsDown: game.inputIsDown,
  inputPointer: game.inputPointer,
  inputPointerPressed: game.inputPointerPressed,
  inputPressed: game.inputPressed,
  inputPressedAtTick: game.inputPressedAtTick,
  inputReleased: game.inputReleased,
  inputSwipe: game.inputSwipe,
  machineCreate: game.machineCreate,
  machineCurrent: game.machineCurrent,
  machineTicksInState: game.machineTicksInState,
  machineTransition: game.machineTransition,
  physicsAddBody: game.physicsAddBody,
  physicsApplyForce: game.physicsApplyForce,
  physicsApplyImpulse: game.physicsApplyImpulse,
  physicsApplyTorque: game.physicsApplyTorque,
  physicsSetAngularVelocity: game.physicsSetAngularVelocity,
  physicsSetConfig: game.physicsSetConfig,
  physicsSetVelocity: game.physicsSetVelocity,
  rngBelow: game.rngBelow,
  rngPermutation: game.rngPermutation,
  rngStream: game.rngStream,
  rngUnit: game.rngUnit,
  rngWeighted: game.rngWeighted,
  snapshot: game.snapshot,
  timerAfter: game.timerAfter,
  timerCancel: game.timerCancel,
  timerEvery: game.timerEvery,
  timersDue: game.timersDue,
  tweenActive: game.tweenActive,
  tweenAdd: game.tweenAdd,
  tweenCancel: game.tweenCancel,
  tweenCompleted: game.tweenCompleted,
  tweenValue: game.tweenValue,
  worldChildrenOf: game.worldChildrenOf,
  worldDespawn: game.worldDespawn,
  worldDespawnSubtree: game.worldDespawnSubtree,
  worldGet: adaptWorldGet(game),
  worldQuery: game.worldQuery,
  worldSet: adaptWorldSet(game),
  worldSpawn: adaptWorldSpawn(game),
});

const NANOS_PER_MILLI = 1_000_000;

/*
 * Drive `loop.advance` from requestAnimationFrame, measuring each frame's elapsed
 * time with performance.now() and converting to nanoseconds. `isRunning` gates
 * whether a frame steps the sim (pause/stop freeze the accumulator). Returns a
 * stop function that halts the RAF chain.
 */
export const driveRaf = (loop: GameLoop, isRunning: () => boolean): (() => void) => {
  let last = performance.now();
  let active = true;
  const frame = (now: number): void => {
    const elapsedNanos = (now - last) * NANOS_PER_MILLI;
    last = now;
    if (isRunning()) {
      loop.advance(elapsedNanos);
    }
    if (active) {
      requestAnimationFrame(frame);
    }
  };
  requestAnimationFrame(frame);
  return (): void => {
    active = false;
  };
};
