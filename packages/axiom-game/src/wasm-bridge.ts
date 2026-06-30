/*
 * The wasm↔bridge ADAPTER: it builds a `NativeBridge` from the raw `WasmGame`
 * exports `apps/axiom-game-runtime` produces. This is the platform edge's
 * marshalling half — the analogue of @axiom/client's transport codecs — extracted
 * from `raf-loop.ts` (which now owns only the requestAnimationFrame + clock
 * driver) so each per-subsystem binding has room to land. Like `raf-loop.ts` it is
 * the wasm-binding edge: it is coverage-exempt (its correctness is the exact byte
 * layout of the live wasm boundary, verified via the Playwright path, not a fake)
 * — see the `--test-coverage-exclude` in package.json and the `.oxlintrc.json`
 * override. Unlike `raf-loop.ts` it keeps the Branchless Law ON: every selection
 * here is a `pick`/`orElse`/`each` combinator from `control-flow.ts`, never an
 * `if`/`?:`/`??`. The two rules it cannot meet are scoped off and documented in
 * `.oxlintrc.json`:
 *   - `no-unsafe-type-assertion`: the byte codec must assert `Component`↔record
 *     and the swipe `string`→union (the wasm boundary is untyped bytes/strings);
 *   - `max-lines`: the one adapter file carries the whole boundary (codec + world
 *     + input + physics), past the 300-line budget a single subsystem assumes.
 *
 * ## Boundary conventions this adapter reshapes (the Rust half is the matching
 * `apps/axiom-game-runtime/src/{world,input,physics}.rs`)
 *   - retained world (SPEC-02): a `Component` ↔ `(kind: string, fields: bytes)`
 *     pair, via the component codec below;
 *   - input (SPEC-05): an optional read crosses as a `number[]` that is EMPTY
 *     when absent (`inputPointer`/`inputPointerPressed`/`inputPressedAtTick`) or a
 *     `string` that is `""` when absent (`inputSwipe`); this edge maps those to
 *     the `Result` empty value. The boolean reads already match `NativeBridge`;
 *   - physics (SPEC-10): a `Vec3` crosses as scalar `(x, y, z)` args, so the
 *     vector verbs and `setConfig` destructure here.
 * The rng / timer / tween / machine reads already match the `NativeBridge` shape,
 * so they forward unchanged.
 */

import type { Component, Handle, Quat, Result, Ticks, Transform, Vec2, Vec3 } from "./vocabulary.ts";
import type { NativeBridge, PointerSample, Swipe, TweenCurve } from "./native-bridge.ts";
import { each, orElse, pick } from "./control-flow.ts";
import type { StepBudget } from "./step-budget.ts";

/*
 * The raw `WasmGame` exports. `advance` adapts its snake_case f64 `StepReport`;
 * the retained-world methods speak `(kind, fields-bytes)`; the input reads carry
 * the `NativeBridge` `tick` arg but return boundary primitives (`number[]` empty =
 * absent, `string` `""` = absent); the physics vector verbs take scalar `(x, y,
 * z)`. The injection surface (`inputKey`/`inputPointerEvent`/`inputPointerClear`/
 * `inputSetSurface`/`bindAction`) is the browser-event feed + host `bindAction`,
 * not a `NativeBridge` read, so it is declared here but not part of the built
 * bridge.
 */
export interface WasmGameExport
  extends Omit<
    NativeBridge,
    | "advance"
    | "inputIsDown"
    | "inputLookDelta"
    | "inputPressed"
    | "inputReleased"
    | "inputPointer"
    | "inputPointerPressed"
    | "inputSwipe"
    | "inputPressedAtTick"
    | "physicsSetConfig"
    | "physicsApplyImpulse"
    | "physicsApplyForce"
    | "physicsApplyTorque"
    | "physicsSetVelocity"
    | "physicsSetAngularVelocity"
    | "tweenAdd"
    | "worldSpawn"
    | "worldDespawn"
    | "worldDespawnSubtree"
    | "worldGet"
    | "worldSet"
    | "worldQuery"
    | "worldChildrenOf"
    | "worldParentOf"
    | "worldWorldTransform"
  > {
  readonly advance: (elapsedNanos: number) => {
    readonly fixed_step_nanos: number;
    readonly remainder_nanos: number;
    readonly steps: number;
  };
  // Input reads (SPEC-05): booleans match; optional reads return empty `number[]` / `""` = absent.
  readonly inputIsDown: (tick: Ticks, action: string) => boolean;
  readonly inputPressed: (tick: Ticks, action: string) => boolean;
  readonly inputReleased: (tick: Ticks, action: string) => boolean;
  readonly inputLookDelta: (tick: Ticks) => Float64Array;
  readonly inputPointer: (tick: Ticks) => Float64Array;
  readonly inputPointerPressed: (tick: Ticks) => Float64Array;
  readonly inputSwipe: (tick: Ticks) => string;
  readonly inputPressedAtTick: (tick: Ticks, action: string) => Float64Array;
  // Input injection (SPEC-05): the browser-event feed + host `bindAction`.
  readonly inputKey: (token: string, down: boolean) => void;
  readonly inputPointerEvent: (x: number, y: number, down: boolean) => void;
  readonly inputPointerClear: () => void;
  readonly inputSetSurface: (width: number, height: number) => void;
  readonly inputLook: (dx: number, dy: number) => void;
  readonly bindAction: (action: string, keys: readonly string[]) => void;
  // Physics (SPEC-10): a `Vec3` crosses as scalar `(x, y, z)`.
  readonly physicsSetConfig: (
    gravityX: number,
    gravityY: number,
    gravityZ: number,
    linearDamping: number,
    angularDamping: number,
  ) => void;
  readonly physicsApplyImpulse: (body: Handle, x: number, y: number, z: number) => void;
  readonly physicsApplyForce: (body: Handle, x: number, y: number, z: number) => void;
  readonly physicsApplyTorque: (body: Handle, x: number, y: number, z: number) => void;
  readonly physicsSetVelocity: (body: Handle, x: number, y: number, z: number) => void;
  readonly physicsSetAngularVelocity: (body: Handle, x: number, y: number, z: number) => void;
  // Tweens (SPEC-09): the `TweenCurve` struct crosses as scalar `(from, to, durationTicks, easeIndex)`.
  readonly tweenAdd: (
    tick: Ticks,
    from: number,
    to: number,
    durationTicks: Ticks,
    easeIndex: number,
  ) => Handle;
  // Retained world (SPEC-02): `(kind, fields-bytes)`; entities cross as numbers.
  readonly worldSpawn: () => number;
  readonly worldDespawn: (entity: number) => void;
  readonly worldDespawnSubtree: (entity: number) => void;
  readonly worldGet: (entity: number, kind: string) => Uint8Array;
  readonly worldSet: (entity: number, kind: string, fields: Uint8Array) => void;
  readonly worldQuery: (kinds: readonly string[]) => readonly number[];
  readonly worldChildrenOf: (entity: number) => readonly number[];
  /*
   * Hierarchy + liveness (SPEC-02): `worldParentOf` returns `[]` / `[parent]` and
   * `worldWorldTransform` returns `[]` / the flat 10-tuple — the same
   * empty-is-absent shape the input reads use, mapped to the `Result` empty value
   * here.
   */
  readonly worldParentOf: (entity: number) => Float64Array;
  readonly worldWorldTransform: (entity: number) => Float64Array;
}

/*
 * The component marshalling convention, TS half. A `Component` crosses the wasm
 * boundary as a `(kind: string, fields: Uint8Array)` pair: `kind` is the
 * discriminant the native `world.rs` routes on (== the `Reflect` schema name), and
 * `fields` are exactly the kernel `Reflect` wire bytes for that kind —
 * little-endian scalars, a string as a u32-LE length prefix then UTF-8. This table
 * mirrors the closed vocabulary in `apps/axiom-game-runtime/src/world.rs`; the
 * field ORDER must match each Rust `reflect_write`. `worldGet`'s empty buffer (a
 * miss) maps to the empty `Result`.
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

/** No field specs — the fallback for an unknown kind (kept branchless via `orElse`). */
const NO_FIELDS: readonly FieldSpec[] = [];

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
  const fields = orElse(COMPONENT_FIELDS[component.kind], NO_FIELDS);
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
  const fields = orElse(COMPONENT_FIELDS[kind], NO_FIELDS);
  // Present iff BOTH bytes and field specs are non-empty (`min` collapses the two guards: either zero -> absent).
  const present = Number(Math.min(bytes.byteLength, fields.length) > 0);
  return pick<() => Result<Component>>(
    [(): Result<Component> => absent<Component>(), (): Result<Component> => decodeFields(fields, kind, bytes)],
    present,
  )();
};

/*
 * Map an optional input read's boundary `Float64Array` to a `Result`: an empty
 * array is absent, otherwise `build` shapes the present values. Branchless `pick`
 * over two thunks, so `build` never runs on the empty case.
 */
const fromScalars = <Value>(raw: Float64Array, build: (values: readonly number[]) => Value): Result<Value> => {
  const values = [...raw];
  return pick<() => Result<Value>>(
    [(): Result<Value> => absent<Value>(), (): Result<Value> => build(values)],
    Number(values.length > 0),
  )();
};

/** The `down` axis index in a pointer sample's `[x, y, down]` boundary array. */
const POINTER_DOWN = 2;

/*
 * Adapt the snake_case wasm `advance` to the camelCase `StepBudget`. The nanos
 * fields cross as f64 `number`s (not BigInt i64) so the Binaryen `wasm2js`
 * fallback — which legalizes i64 into i32 pairs and has no BigInt ABI — can run;
 * the elapsed delta is passed as a rounded integer `number`, no `BigInt(...)` wrap.
 */
const adaptAdvance =
  (game: WasmGameExport) =>
  (elapsedNanos: number): StepBudget => {
    const report = game.advance(Math.round(elapsedNanos));
    return {
      fixedStepNanos: report.fixed_step_nanos,
      remainderNanos: report.remainder_nanos,
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

/** The flat-world-transform indices: `[tx,ty,tz, qx,qy,qz,qw, sx,sy,sz]` (SPEC-02). */
const TRANSFORM_PZ = 2;
const TRANSFORM_RX = 3;
const TRANSFORM_RY = 4;
const TRANSFORM_RZ = 5;
const TRANSFORM_RW = 6;
const TRANSFORM_SX = 7;
const TRANSFORM_SY = 8;
const TRANSFORM_SZ = 9;

/** Adapt `worldParentOf`: the boundary `[]` / `[parent]` (empty = root / absent) to a `Result<Entity>`. */
const adaptWorldParentOf =
  (game: WasmGameExport) =>
  (entity: number): Result<number> =>
    fromScalars(game.worldParentOf(entity), (values): number => pick(values, 0));

/** Adapt `worldWorldTransform`: the boundary `[]` / flat 10-tuple to a `Result<Transform>`. */
const adaptWorldWorldTransform =
  (game: WasmGameExport) =>
  (entity: number): Result<Transform> =>
    fromScalars(
      game.worldWorldTransform(entity),
      (values): Transform => ({
        position: { x: pick(values, 0), y: pick(values, 1), z: pick(values, TRANSFORM_PZ) },
        rotation: [
          pick(values, TRANSFORM_RX),
          pick(values, TRANSFORM_RY),
          pick(values, TRANSFORM_RZ),
          pick(values, TRANSFORM_RW),
        ] as Quat,
        scale: { x: pick(values, TRANSFORM_SX), y: pick(values, TRANSFORM_SY), z: pick(values, TRANSFORM_SZ) },
      }),
    );

/** Adapt `inputLookDelta`: the boundary `[dx, dy]` slice to a `Vec2` (always present — `(0, 0)` when there was no look). */
const adaptInputLookDelta =
  (game: WasmGameExport) =>
  (tick: Ticks): Vec2 => {
    const values = [...game.inputLookDelta(tick)];
    return { x: pick(values, 0), y: pick(values, 1) };
  };

/** Adapt `inputPointer`: the boundary `[x, y, down]` (empty = absent) to a `PointerSample`. */
const adaptInputPointer =
  (game: WasmGameExport) =>
  (tick: Ticks): Result<PointerSample> =>
    fromScalars(
      game.inputPointer(tick),
      (values): PointerSample => ({
        down: pick(values, POINTER_DOWN) !== 0,
        pos: { x: pick(values, 0), y: pick(values, 1) },
      }),
    );

/** Adapt `inputPointerPressed`: the boundary `[x, y]` (empty = absent) to a `Vec2`. */
const adaptInputPointerPressed =
  (game: WasmGameExport) =>
  (tick: Ticks): Result<Vec2> =>
    fromScalars(game.inputPointerPressed(tick), (values): Vec2 => ({ x: pick(values, 0), y: pick(values, 1) }));

/** Adapt `inputPressedAtTick`: the boundary `[tick]` (empty = never pressed) to a `Ticks`. */
const adaptInputPressedAtTick =
  (game: WasmGameExport) =>
  (tick: Ticks, action: string): Result<Ticks> =>
    fromScalars(game.inputPressedAtTick(tick, action), (values): Ticks => pick(values, 0));

/** Adapt `inputSwipe`: the boundary direction string (`""` = absent) to a `Result<Swipe>`. */
const adaptInputSwipe =
  (game: WasmGameExport) =>
  (tick: Ticks): Result<Swipe> => {
    const name = game.inputSwipe(tick);
    return pick<() => Result<Swipe>>(
      [(): Result<Swipe> => absent<Swipe>(), (): Result<Swipe> => name as Swipe],
      Number(name.length > 0),
    )();
  };

/** Adapt `physicsSetConfig`: destructure the gravity `Vec3` into scalar args. */
const adaptPhysicsSetConfig =
  (game: WasmGameExport) =>
  (gravity: Vec3, linearDamping: number, angularDamping: number): void => {
    game.physicsSetConfig(gravity.x, gravity.y, gravity.z, linearDamping, angularDamping);
  };

/** Adapt `physicsApplyImpulse`: destructure the impulse `Vec3` into scalar args. */
const adaptPhysicsApplyImpulse =
  (game: WasmGameExport) =>
  (body: Handle, impulse: Vec3): void => {
    game.physicsApplyImpulse(body, impulse.x, impulse.y, impulse.z);
  };

/** Adapt `physicsApplyForce`: destructure the force `Vec3` into scalar args. */
const adaptPhysicsApplyForce =
  (game: WasmGameExport) =>
  (body: Handle, force: Vec3): void => {
    game.physicsApplyForce(body, force.x, force.y, force.z);
  };

/** Adapt `physicsApplyTorque`: destructure the torque `Vec3` into scalar args. */
const adaptPhysicsApplyTorque =
  (game: WasmGameExport) =>
  (body: Handle, torque: Vec3): void => {
    game.physicsApplyTorque(body, torque.x, torque.y, torque.z);
  };

/** Adapt `physicsSetVelocity`: destructure the velocity `Vec3` into scalar args. */
const adaptPhysicsSetVelocity =
  (game: WasmGameExport) =>
  (body: Handle, velocity: Vec3): void => {
    game.physicsSetVelocity(body, velocity.x, velocity.y, velocity.z);
  };

/** Adapt `physicsSetAngularVelocity`: destructure the velocity `Vec3` into scalar args. */
const adaptPhysicsSetAngularVelocity =
  (game: WasmGameExport) =>
  (body: Handle, velocity: Vec3): void => {
    game.physicsSetAngularVelocity(body, velocity.x, velocity.y, velocity.z);
  };

/** Adapt `tweenAdd`: destructure the `TweenCurve` struct into the scalar boundary args. */
const adaptTweenAdd =
  (game: WasmGameExport) =>
  (tick: Ticks, curve: TweenCurve): Handle =>
    game.tweenAdd(tick, curve.from, curve.to, curve.durationTicks, curve.easeIndex);

/*
 * Adapt the snake_case wasm `WasmGame` to the loop core's camelCase NativeBridge.
 *
 * Methods that already match the `NativeBridge` shape are forwarded `.bind(game)`,
 * NOT as bare `game.method` references: a wasm-bindgen export is a prototype method
 * that reads `this.__wbg_ptr`, so a bare reference invoked as `bridge.method(…)`
 * loses its receiver and traps as "null pointer passed to rust". Binding pins the
 * receiver; the `adapt*` wrappers already call `game.method(…)` explicitly and so
 * need no bind.
 */
export const bridgeFromWasm = (game: WasmGameExport): NativeBridge => ({
  advance: adaptAdvance(game),
  inputIsDown: (tick: Ticks, action: string): boolean => game.inputIsDown(tick, action),
  inputLookDelta: adaptInputLookDelta(game),
  inputPointer: adaptInputPointer(game),
  inputPointerPressed: adaptInputPointerPressed(game),
  inputPressed: (tick: Ticks, action: string): boolean => game.inputPressed(tick, action),
  inputPressedAtTick: adaptInputPressedAtTick(game),
  inputReleased: (tick: Ticks, action: string): boolean => game.inputReleased(tick, action),
  inputSwipe: adaptInputSwipe(game),
  machineCreate: game.machineCreate.bind(game),
  machineCurrent: game.machineCurrent.bind(game),
  machineTicksInState: game.machineTicksInState.bind(game),
  machineTransition: game.machineTransition.bind(game),
  physicsAddBody: game.physicsAddBody.bind(game),
  physicsApplyForce: adaptPhysicsApplyForce(game),
  physicsApplyImpulse: adaptPhysicsApplyImpulse(game),
  physicsApplyTorque: adaptPhysicsApplyTorque(game),
  physicsSetAngularVelocity: adaptPhysicsSetAngularVelocity(game),
  physicsSetConfig: adaptPhysicsSetConfig(game),
  physicsSetVelocity: adaptPhysicsSetVelocity(game),
  rngBelow: game.rngBelow.bind(game),
  rngPermutation: game.rngPermutation.bind(game),
  rngStream: game.rngStream.bind(game),
  rngUnit: game.rngUnit.bind(game),
  rngWeighted: game.rngWeighted.bind(game),
  snapshot: game.snapshot.bind(game),
  timerAfter: game.timerAfter.bind(game),
  timerCancel: game.timerCancel.bind(game),
  timerEvery: game.timerEvery.bind(game),
  timersDue: game.timersDue.bind(game),
  tweenActive: game.tweenActive.bind(game),
  tweenAdd: adaptTweenAdd(game),
  tweenCancel: game.tweenCancel.bind(game),
  tweenCompleted: game.tweenCompleted.bind(game),
  tweenValue: game.tweenValue.bind(game),
  worldAlive: game.worldAlive.bind(game),
  worldChildrenOf: game.worldChildrenOf.bind(game),
  worldDespawn: game.worldDespawn.bind(game),
  worldDespawnSubtree: game.worldDespawnSubtree.bind(game),
  worldGet: adaptWorldGet(game),
  worldHas: game.worldHas.bind(game),
  worldParentOf: adaptWorldParentOf(game),
  worldQuery: game.worldQuery.bind(game),
  worldRemove: game.worldRemove.bind(game),
  worldSet: adaptWorldSet(game),
  worldSetParent: game.worldSetParent.bind(game),
  worldSpawn: adaptWorldSpawn(game),
  worldWorldTransform: adaptWorldWorldTransform(game),
});
