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

/*
 * An opaque texture handle (SPEC-04 §10): what `loadTexture` returns and a
 * `Frame.sprite` draws. Stable for the session; the app resolves its pixels
 * (fetch/decode) and the engine only ever names the handle, never loads bytes.
 */
export type TextureId = Handle;

/*
 * A font selection (SPEC-04 §4.2): the `family` name, the `size` in surface
 * units, and an optional `weight`. What `loadFont` returns and a `Frame.text`
 * draws with. Tier-0 ships one built-in monospace family; `size` drives the
 * glyph size and the deterministic `measureText`.
 */
export interface FontSpec {
  readonly family: string;
  readonly size: number;
  readonly weight?: number;
}

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

/** An integer tile coordinate (SPEC-06 §5) — the cell newtype the grid queries traffic in. */
export interface Cell {
  readonly x: number;
  readonly y: number;
}

/** An axis-aligned 2D rectangle (SPEC-04 §10) — origin `(x, y)` plus `width`/`height`. */
export interface Rect {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

/*
 * A 2D circle (SPEC-03 §4.2) — a `center` point plus a `radius`. The neutral
 * value the `circleOverlap` predicate compares: the SDK bundles the contract's
 * `(aCenter, aR, bCenter, bR)` into two `Circle` records so the authoring call
 * stays within the ≤3-parameter law (the same record-bundling the camera /
 * perspective descriptors use); the geometry still routes to the native `Sphere`.
 */
export interface Circle {
  /** The circle center. */
  readonly center: Vec2;
  /** The circle radius. */
  readonly radius: number;
}

/*
 * A 4-channel colour (SPEC-11), as a positional `[r, g, b, a]` tuple. It is a
 * tuple rather than an `{ r, g, b, a }` record because the SDK's `id-length` law
 * admits only the geometric `x`/`y`/`z` single-letter names; `[r, g, b, a]` is
 * the conventional WebGL/CSS colour shape and crosses the wasm boundary as four
 * plain numbers (SPEC-11 §5 "plain number records").
 */
export type Rgba = readonly [number, number, number, number];

/*
 * A 4×4 matrix (SPEC-11), a 16-element row-major number array — the neutral
 * `Mat4` value the native `MathApi` produces and consumes. Plain numbers only, so
 * it marshals 1:1 across the wasm boundary (SPEC-11 §5); never re-derived in TS.
 */
export type Mat4 = readonly number[];

/*
 * A quaternion (SPEC-11), as a positional `[x, y, z, w]` tuple (vector part then
 * scalar `w`). A tuple — not an `{ x, y, z, w }` record — so the scalar `w` needs
 * no `id-length` exception; it is the neutral value the native `MathApi` returns
 * and is never re-implemented in TS.
 */
export type Quat = readonly [number, number, number, number];

/*
 * A resolved scene transform (SPEC-02 §4.2): the composed world `position` /
 * `rotation` / `scale` `worldTransform` reads back for a node this tick. It is
 * the projection of the native authoritative world transform (the flat
 * `[tx,ty,tz, qx,qy,qz,qw, sx,sy,sz]` tuple `worldWorldTransform` returns), so
 * `rotation` is the 3D `Quat` form (SPEC-02 names `number` for the 2D shorthand
 * and "quaternion form for 3D"; the native channel is the 3D form). Plain value
 * records only, so it marshals 1:1 across the wasm boundary; never re-derived in
 * TS.
 */
export interface Transform {
  /** The world-space position. */
  readonly position: Vec3;
  /** The world-space rotation (quaternion). */
  readonly rotation: Quat;
  /** The world-space scale. */
  readonly scale: Vec3;
}

/*
 * The nearest bounded ray hit a `raycast` reports (SPEC-03 §5): the `entity` the
 * ray struck, the world-space `point` it entered, and the `distance` from the
 * ray origin to that point. A pure value record; a miss is the empty `Result`,
 * never a throw (§0.2).
 */
export interface RayHit {
  /** The entity the ray struck. */
  readonly entity: Entity;
  /** The world-space entry point on the entity's bounds. */
  readonly point: Vec3;
  /** The distance from the ray origin to `point`. */
  readonly distance: number;
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
