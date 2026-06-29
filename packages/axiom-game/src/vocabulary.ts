/*
 * The neutral value vocabulary every authoring projection traffics in (SPEC-00
 * §0.2 / §5). These are pure data shapes — opaque numeric handles and small
 * records — with no behavior: the nouns the `Sim`/`World`/`Input` projections
 * hand back. They are intentionally structural (a bare `number` for every
 * handle) so the wasm `NativeBridge` can carry them across the boundary as
 * plain values and a replay can re-bind them (SPEC-00 §9).
 *
 * `Result<T> = T | undefined` is the single optional shape: a query miss, an
 * absent component, or a read on a dead handle is the empty value, never a
 * throw (SPEC-02 §5, whose contract notation is `T | null`). The TS projection
 * uses `undefined` — the TS-native optional and the form the SDK's lint law
 * mandates (`unicorn/no-null` bans the `null` literal, exactly as the Rust
 * spine is branchless). The empty value lives only in this type position and is
 * produced solely by the bridge implementation; the spine never writes an empty
 * literal (`eslint/no-undefined`), it forwards what the bridge returns.
 */

/** An opaque entity handle — an index over a native `EntityHandle` (SPEC-02). */
export type Entity = number;

/** A monotonic fixed-tick count (SPEC-00 §0.2). */
export type Ticks = number;

/** A duration in seconds (SPEC-00 §0.2). */
export type Seconds = number;

/** An opaque resource/timer/tween handle (SPEC-00 §0.2). */
export type Handle = number;

/** An opaque per-room player identity (SPEC-12 §16.6). */
export type PlayerId = number;

/** A 2D vector (SPEC-03). */
export interface Vec2 {
  readonly x: number;
  readonly y: number;
}

/** A 3D vector (SPEC-03 / SPEC-11). */
export interface Vec3 {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

/** An optional value: present `Value`, or `undefined` on a miss (SPEC-02 §5, contract `T | null`). */
export type Result<Value> = Value | undefined;

/*
 * An opaque typed component record. The engine stores its bytes and never reads
 * gameplay meaning (SPEC-02 §5); the `kind` discriminant is the only field the
 * world surface itself routes on. Concrete components extend this with their
 * own data fields.
 */
export interface Component {
  readonly kind: string;
}

/*
 * The kind token that selects a component column (SPEC-02 §5, contract
 * `ComponentKind<C>`). It is a plain string at runtime — the column name. The
 * spec's phantom component type is dropped in the TS projection: reconstructing
 * a typed `C` from the opaque stored record is exactly the unsafe downcast the
 * SDK's lint law forbids (`typescript/no-unsafe-type-assertion`), so `World.get`
 * returns the base `Component` and the author narrows on the `kind` discriminant
 * (the safe, idiomatic TS pattern).
 */
export type ComponentKind = string;
