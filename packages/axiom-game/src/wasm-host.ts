/*
 * The wasm↔HOST adapter: it builds a `HostBridge` from the raw `WasmGame` exports
 * `apps/axiom-game-runtime` produces — the sibling of `wasm-bridge.ts`
 * (`bridgeFromWasm`, which builds the `NativeBridge`). This is the piece the
 * earlier keystone waves left open: the `HostBridge` is what the FREE authoring
 * surface (`math3d`'s `v3`/`mat4`/`quat`, `grid.ts`, `sound.ts`, the 3D `scene3d`
 * create* methods, `clamp`/`normalizeAngle`) projects through, installed once at
 * boot by `bindNative`.
 *
 * Like `wasm-bridge.ts` / `raf-loop.ts` it is the wasm-binding platform edge: it
 * is coverage-exempt (its correctness is the exact byte layout of the live wasm
 * boundary, verified via the Playwright path, not a fake — see the
 * `--test-coverage-exclude` in package.json) and keeps the Branchless Law ON
 * (every selection is a `pick`/`orElse` combinator from `control-flow.ts`, never
 * an `if`/`?:`/`??`). It lives in its own file rather than swelling
 * `wasm-bridge.ts` past its budget; it is scoped in `.oxlintrc.json` exactly like
 * `wasm-bridge.ts` (`max-lines`/`max-params`/`no-unsafe-type-assertion` off — the
 * one adapter carries the whole host boundary, the scalar-vector wasm signatures
 * are inherently >3 args, and the byte boundary is untyped).
 *
 * ## Boundary conventions this adapter reshapes (the Rust half is the matching
 * `apps/axiom-game-runtime/src/{mathbridge,grid,audio}.rs`)
 *   - math (SPEC-03/11): a `Vec3`/`Mat4`/`Quat` crosses as a `Float64Array` slice
 *     (`[x,y,z]` / 16 column-major / `[x,y,z,w]`); the edge packs the contract's
 *     `{x,y,z}` / `number[]` / `[x,y,z,w]` into a slice and unpacks the result;
 *   - grid (SPEC-06): a query crosses as `(cols, rows, passable-bytes, …cells)`;
 *     the edge flattens `GridField.passable` to a `Uint8Array` mask and the cells
 *     to `[x,y]` int slices, and reshapes the flat `Float64Array` result into
 *     `Cell[]` / a distance `number[]` / a single `Cell`;
 *   - audio (SPEC-08): handles cross as numbers; the option records destructure
 *     into the scalar `(volume, pitch, loop)` / `(at, volume)` args, and the tone
 *     `wave` string resolves to its dense index by `indexOf` (a table select);
 *   - draw2d (SPEC-04): a colour packs to a `0xRRGGBBAA` u32, a point/`Rect`/
 *     emitter-recipe to a `Float64Array` slice, and `layer`/`alpha`/`gravity`
 *     default here (the audio-style host-side defaulting); `draw2dFinish`'s flat
 *     `Vec<f64>` command list reshapes to a `number[]`.
 *
 * ## Now-wired host groups (Wave 3b) and the partials they carry
 * The spatial-query (SPEC-03), 3D-authoring (SPEC-11), input-bind (SPEC-05) and
 * embed-channel (SPEC-12) groups are bound here over the matching Rust exports
 * (`apps/axiom-game-runtime/src/{query,scene3d,input}.rs` + `wasm.rs`). Where a
 * Wave-2 export carries less than the contract descriptor, the edge forwards the
 * subset the export accepts (documented, not silent):
 *   - **`createMaterial`** consumes only the base-colour `[r,g,b]`; emissive /
 *     roughness / opacity are dropped (the native `add_material` takes a lit
 *     colour only).
 *   - **`setCamera3D`** sends the eye position + look-at `target` + vertical FOV
 *     (converted radians → degrees) + near/far; the native `set_camera` aims the
 *     camera from the position toward the target (world up = +Y).
 *   - **`addLight`** binds the directional arm only (direction + colour +
 *     intensity); the `kind` discriminant is dropped (the native `add_light`
 *     mints a `DirectionalLight`).
 *   - **`raycast`** — the native export returns `[entity, x, y, z]` (no distance),
 *     so the `RayHit.distance` is closed at the edge with the native `v3Dist`
 *     (the single math source of truth), never a TS re-derivation.
 *   - **`reportOutcomes`** re-flushes the single latched outcome; the per-player
 *     `results` map is dropped (the native channel is single-outcome).
 * One group stays deferred:
 *   - **`mat4Invert`**: `axiom-math` exposes no general 4×4 inverse (only
 *     `Quat::inverse` and the uniform-scale-TRS `Transform::inverse`); a TS
 *     re-derivation would violate the single-math-source rule. Awaits a
 *     `Mat4::inverse` primitive in the math layer.
 */

import {
  type CameraDescriptor,
  type ControllerInput,
  type ControllerSpec,
  type GridField,
  IDENTITY_MAT4,
  type LightDescriptor,
  type MaterialDescriptor,
  type PerspectiveSpec,
} from "./host-descriptors.ts";
import type { Cell, Circle, Entity, FontSpec, Handle, Mat4, Quat, RayHit, Rect, Result, Rgba, TextureId, Transform, Vec2, Vec3 } from "./vocabulary.ts";
import type { EllipseRadii, EmitterConfig, LineStyle, ShapeStyle, SpriteAnimation, SpriteOpts, TextMetrics, TextOpts } from "./draw2d-binding.ts";
import type {
  HostBridge,
  MusicOptions,
  Outcome,
  ScheduleOptions,
  SessionConfig,
  SoundOptions,
  ToneSpec,
} from "./host-binding.ts";
import type { UiBridge, UiStyle, UiTextOpts, UiViewport } from "./ui-binding.ts";
import { orElse, pick } from "./control-flow.ts";
import { seedFromHalves } from "./seed-codec.ts";

/*
 * The raw host-facing `WasmGame` exports this adapter reads. Vectors/matrices/
 * quaternions and grid results come back as `Float64Array`; handles and scalars
 * as numbers; `gridReachable` as a boolean. Cells/vectors are passed in as the
 * matching `Float64Array` / `Int32Array` slices.
 */
export interface WasmHostExport {
  // Math — v2 (SPEC-03 §4.2)
  readonly v2Add: (lhs: Float64Array, rhs: Float64Array) => Float64Array;
  readonly v2Sub: (lhs: Float64Array, rhs: Float64Array) => Float64Array;
  readonly v2Scale: (vector: Float64Array, scalar: number) => Float64Array;
  readonly v2Dot: (lhs: Float64Array, rhs: Float64Array) => number;
  readonly v2Len: (vector: Float64Array) => number;
  readonly v2Normalize: (vector: Float64Array) => Float64Array;
  readonly v2Dist: (lhs: Float64Array, rhs: Float64Array) => number;
  readonly v2Lerp: (lhs: Float64Array, rhs: Float64Array, fraction: number) => Float64Array;
  // Math — v3 (SPEC-11 §4.2)
  readonly v3Add: (lhs: Float64Array, rhs: Float64Array) => Float64Array;
  readonly v3Sub: (lhs: Float64Array, rhs: Float64Array) => Float64Array;
  readonly v3Scale: (vector: Float64Array, scalar: number) => Float64Array;
  readonly v3Dot: (lhs: Float64Array, rhs: Float64Array) => number;
  readonly v3Cross: (lhs: Float64Array, rhs: Float64Array) => Float64Array;
  readonly v3Len: (vector: Float64Array) => number;
  readonly v3Normalize: (vector: Float64Array) => Float64Array;
  readonly v3Dist: (lhs: Float64Array, rhs: Float64Array) => number;
  readonly v3Lerp: (lhs: Float64Array, rhs: Float64Array, fraction: number) => Float64Array;
  // Math — mat4 (SPEC-11 §4.2); `mat4Invert` is deferred (see header).
  readonly mat4Identity: () => Float64Array;
  readonly mat4Multiply: (lhs: Float64Array, rhs: Float64Array) => Float64Array;
  readonly mat4Perspective: (fovy: number, aspect: number, near: number, far: number) => Float64Array;
  readonly mat4LookAt: (eye: Float64Array, target: Float64Array, up: Float64Array) => Float64Array;
  readonly mat4FromTRS: (translation: Float64Array, rotation: Float64Array, scale: Float64Array) => Float64Array;
  // Math — quat (SPEC-11 §4.2)
  readonly quatIdentity: () => Float64Array;
  readonly quatFromEuler: (pitch: number, yaw: number, roll: number) => Float64Array;
  readonly quatMultiply: (lhs: Float64Array, rhs: Float64Array) => Float64Array;
  readonly quatNormalize: (quaternion: Float64Array) => Float64Array;
  readonly quatToMat4: (quaternion: Float64Array) => Float64Array;
  // Math — scalar (SPEC-03 §4.2)
  readonly clamp: (value: number, low: number, high: number) => number;
  readonly lerp: (start: number, end: number, fraction: number) => number;
  readonly normalizeAngle: (angle: number) => number;
  /*
   * Pure predicates (SPEC-03 §4.2): a rect crosses as a `[x, y, w, h]` slice, a
   * circle as a `[centerX, centerY, radius]` slice, a point as a `[x, y]` slice;
   * each returns a boolean.
   */
  readonly aabbOverlap: (lhs: Float64Array, rhs: Float64Array) => boolean;
  readonly pointInRect: (point: Float64Array, rect: Float64Array) => boolean;
  readonly circleOverlap: (lhs: Float64Array, rhs: Float64Array) => boolean;
  // Grid (SPEC-06 §4.2)
  readonly gridPath: (
    cols: number,
    rows: number,
    mask: Uint8Array,
    start: Int32Array,
    goal: Int32Array,
  ) => Float64Array;
  readonly gridReachable: (
    cols: number,
    rows: number,
    mask: Uint8Array,
    start: Int32Array,
    goal: Int32Array,
  ) => boolean;
  readonly gridDistanceField: (cols: number, rows: number, mask: Uint8Array, start: Int32Array) => Float64Array;
  readonly gridStepToward: (
    cols: number,
    rows: number,
    mask: Uint8Array,
    from: Int32Array,
    target: Int32Array,
  ) => Float64Array;
  // Audio (SPEC-08 §4.2)
  readonly loadSound: (url: string) => number;
  readonly playSound: (id: number, volume: number, pitch: number, looping: boolean) => number;
  readonly scheduleSound: (id: number, at: number, volume: number) => number;
  readonly stopVoice: (voice: number) => void;
  readonly playMusic: (urls: readonly string[], looping: boolean, crossfade: number) => number;
  readonly playTone: (waveIndex: number, freq: number, duration: number, volume: number) => number;
  readonly setMasterVolume: (volume: number) => void;
  readonly setMuted: (muted: boolean) => void;
  // Draw2d (SPEC-04 §10): colours arrive packed as a `0xRRGGBBAA` u32; points/bounds/emitter-config/ellipse-geom cross as `Float64Array` slices; handles cross as numbers; `draw2dFinish` returns the flat command list.
  readonly draw2dCamera2d: (center: Float64Array, zoom: number) => void;
  readonly draw2dRect: (bounds: Float64Array, fill: number, stroke: number, strokeWidth: number, layer: number, alpha: number) => void;
  readonly draw2dCircle: (center: Float64Array, radius: number, fill: number, stroke: number, strokeWidth: number, layer: number, alpha: number) => void;
  readonly draw2dEllipse: (geom: Float64Array, fill: number, stroke: number, strokeWidth: number, layer: number, alpha: number) => void;
  readonly draw2dLine: (from: Float64Array, to: Float64Array, color: number, width: number, layer: number, alpha: number) => void;
  readonly draw2dCreateEmitter: (config: Float64Array) => number;
  readonly draw2dEmit: (id: number, at: Float64Array, direction: Float64Array) => void;
  readonly draw2dAdvanceParticles: (dt: number) => void;
  readonly draw2dCreateRenderTarget: (width: number, height: number) => number;
  readonly draw2dBeginTarget: (target: number) => void;
  readonly draw2dEndTarget: () => void;
  readonly draw2dTargetTexture: (target: number) => number;
  readonly draw2dFinish: () => Float64Array;
  readonly draw2dSampleAnimation: (frames: Float64Array, fps: number, elapsed: number, looping: boolean) => Float64Array;
  readonly draw2dSprite: (texture: number, opts: Float64Array) => void;
  readonly draw2dText: (value: string, opts: Float64Array) => void;
  readonly draw2dMeasureText: (value: string, fontSize: number) => Float64Array;
  // Presentation assets (SPEC-04 §10): a texture handle minted for a url; the app fetches/decodes the pixels.
  readonly loadTexture: (url: string) => number;
  /*
   * 3D scene authoring (SPEC-11 §4.2): a mesh kind crosses as its `string` name, a
   * colour/position/direction as a `Float64Array` slice; each call returns the
   * engine handle / light-node id it minted as a number.
   */
  readonly createMesh: (kind: string) => number;
  readonly createMaterial: (rgb: Float64Array) => number;
  readonly setCamera3D: (
    position: Float64Array,
    target: Float64Array,
    fovDeg: number,
    near: number,
    far: number,
  ) => void;
  readonly addLight: (direction: Float64Array, rgb: Float64Array, intensity: number) => number;
  /*
   * 3D scene node authoring (SPEC-11 §4.2): spawn a renderable from a `(mesh,
   * material)` handle pair at a flat 10-tuple transform `[tx,ty,tz, qx,qy,qz,qw,
   * sx,sy,sz]`; move/rescale a node or set its collision box by id; clear the
   * whole scene. Handles / ids cross as numbers, a transform / half-extents as a
   * `Float64Array`.
   */
  readonly spawnRenderable: (meshId: number, materialId: number, transform: Float64Array) => number;
  readonly setNodeTransform: (entity: number, transform: Float64Array) => void;
  readonly setNodeBounds: (entity: number, halfExtents: Float64Array) => void;
  readonly clearScene: () => void;
  /*
   * First-person controller (SPEC-11): spawn the active camera as controller
   * `index` at a `[x,y,z]` position with `fovDeg`/near/far; drive it each frame
   * with a `[x,y,z]` local move plus yaw/pitch radian deltas (applied immediately).
   */
  readonly spawnController: (position: Float64Array, fovDeg: number, near: number, far: number, index: number) => number;
  readonly controlFirstPerson: (index: number, moveLocal: Float64Array, yaw: number, pitch: number) => void;
  /*
   * Spatial queries (SPEC-03 §4.2): a point/direction crosses as a 3-element
   * `Float64Array`; overlaps return the matching entity ids as a flat
   * `Float64Array`; `raycast` returns `[]` or `[entity, hitX, hitY, hitZ]`.
   */
  readonly overlapCircle: (center: Float64Array, radius: number) => Float64Array;
  readonly overlapBox: (center: Float64Array, halfExtents: Float64Array) => Float64Array;
  readonly raycast: (origin: Float64Array, direction: Float64Array, maxDistance: number) => Float64Array;
  // Input bind (SPEC-05 §4.2): the action name + the physical key tokens.
  readonly bindAction: (action: string, keys: readonly string[]) => void;
  /*
   * Embed host channel (SPEC-12 §4.2): the inbound seed + opaque params JSON, the
   * readiness signal, and the single-outcome reporters. The 64-bit seed crosses as
   * two u32 `number` halves (`seed_lo` + `seed_hi`), never a BigInt i64, so the
   * Binaryen `wasm2js` fallback can run; `seedFromHalves` recombines them into the
   * author-facing `bigint`, preserving the full 2^64 seed space (determinism).
   */
  readonly seed_lo: number;
  readonly seed_hi: number;
  readonly sessionParams: () => string;
  readonly notifyReady: () => void;
  readonly report_outcome: (won: boolean, score: number) => boolean;
  readonly reportOutcomes: () => boolean;
  /*
   * Screen-space UI / HUD (SPEC-09 §4.2): a viewport / pointer / bounds crosses as a
   * `Float64Array` slice, a colour packed as a `0xRRGGBBAA` u32, a texture as a number.
   * `uiViewport` returns `[width, height]`; `uiDrawList` the accumulated byte log;
   * `uiSolveLayout` the flat `NODE_STRIDE`-wide table → the flat `[x, y, w, h]…` rects.
   */
  readonly uiBeginFrame: (viewport: Float64Array, pointer: Float64Array, pressed: boolean) => void;
  readonly uiRect: (bounds: Float64Array, fill: number, stroke: number, strokeWidth: number) => void;
  readonly uiText: (value: string, pos: Float64Array, color: number, size: number) => void;
  readonly uiSprite: (texture: number, bounds: Float64Array) => void;
  readonly uiButton: (bounds: Float64Array, label: string, fill: number, stroke: number, sw: number) => boolean;
  readonly uiViewport: () => Float64Array;
  readonly uiDrawList: () => Uint8Array;
  readonly uiSolveLayout: (vw: number, vh: number, nodes: Float64Array) => Float64Array;
}

/** The component indices a vector / quaternion result is unpacked at. */
const Z_INDEX = 2;
const W_INDEX = 3;
/** The two scalars per flat cell in a grid path / step result. */
const CELL_STRIDE = 2;
/** Default per-voice / playlist option values (the host defaults SPEC-08 wave-side). */
const FULL_VOLUME = 1;
const UNCHANGED_PITCH = 1;
const NO_LOOP = false;
const NO_CROSSFADE = 0;
/** The wave kinds in their dense native index order (SPEC-08 `Wave`). */
const WAVE_KINDS: readonly ToneSpec["wave"][] = ["sine", "square", "sawtooth", "triangle"];
/** The native mesh-kind names, indexed by the dense `HostBridge.createMesh` kind (0=box→cube, 1=sphere, 2=cylinder). */
const MESH_NAMES: readonly string[] = ["cube", "sphere", "cylinder"];
/** The `[r, g, b]` channel count of an `Rgba` the native lit-colour authoring consumes (alpha dropped). */
const RGB_LENGTH = 3;
/** Degrees in a half turn — the numerator of the radians→degrees scale the native camera's degree FOV needs. */
const DEGREES_PER_HALF_TURN = 180;
/** Radians → degrees, for the native camera's degree-valued vertical FOV. */
const RAD_TO_DEG = DEGREES_PER_HALF_TURN / Math.PI;

/*
 * Draw2d boundary constants (SPEC-04 §10). A colour packs `[r, g, b, a]` (each in
 * `[0, 1]`) into a `0xRRGGBBAA` u32 by positional scale — `r` is the high byte —
 * mirroring `apps/axiom-game-runtime/src/draw2d.rs`'s `rgba()` unpacker. Arithmetic
 * scaling (not bit-shifts) keeps the codec free of bitwise operators.
 */
const RED_SCALE = 16_777_216;
const GREEN_SCALE = 65_536;
const BLUE_SCALE = 256;
const ALPHA_SCALE = 1;
const RGBA_SCALES: readonly number[] = [RED_SCALE, GREEN_SCALE, BLUE_SCALE, ALPHA_SCALE];
/** A colour channel's 8-bit maximum. */
const CHANNEL_MAX = 255;
/** The default z-layer / fully-opaque alpha a 2D draw resolves to when omitted. */
const DEFAULT_LAYER = 0;
const FULL_ALPHA = 1;
/** No gravity — the emitter default when `gravity` is omitted. */
const NO_GRAVITY: Vec2 = { x: 0, y: 0 };
/** The transparent colour a UI style's omitted `stroke` defaults to (packs to `0x00000000`). */
const TRANSPARENT: Rgba = [0, 0, 0, 0];
/** The stroke width a UI style's omitted `strokeWidth` defaults to. */
const NO_STROKE_WIDTH = 0;
/** The sprite-draw defaults applied host-side for omitted `SpriteOpts` fields (SPEC-04 §4.2). */
const NO_ROTATION_SPRITE = 0;
const UNIT_SCALE: Vec2 = { x: 1, y: 1 };
const TOP_LEFT_ANCHOR: Vec2 = { x: 0, y: 0 };
const WHITE_TINT: Rgba = [1, 1, 1, 1];
/** The "whole texture" source — a zero-size sub-rect the presenter reads as the full bitmap. */
const WHOLE_TEXTURE: Rect = { height: 0, width: 0, x: 0, y: 0 };
/** The text alignments in their dense native index order (SPEC-04 §4.2: 0=left, 1=center, 2=right). */
const ALIGN_NAMES: readonly TextOpts["align"][] = ["left", "center", "right"];
/** The built-in monospace font a `loadFont` returns (Tier-0 ships one family; `size` is the default). */
const DEFAULT_FONT: FontSpec = { family: "monospace", size: 16 };

/** The absent `Result` value, materialized without the lint-banned `undefined` literal. */
const absent = <Value>(slot?: Value): Value | undefined => slot;

/** Pack a `Vec3` into the boundary `[x, y, z]` slice. */
const packVec3 = (vector: Vec3): Float64Array => Float64Array.from([vector.x, vector.y, vector.z]);

/** Pack a [`Transform`] into the native flat 10-tuple `[tx,ty,tz, qx,qy,qz,qw, sx,sy,sz]` slice the scene-node authoring boundary takes. */
const packTransform = (transform: Transform): Float64Array =>
  Float64Array.from([
    transform.position.x,
    transform.position.y,
    transform.position.z,
    ...transform.rotation,
    transform.scale.x,
    transform.scale.y,
    transform.scale.z,
  ]);

/** A `Vec2` as the boundary `[x, y]` slice. */
const packVec2 = (vector: Vec2): Float64Array => Float64Array.from([vector.x, vector.y]);

/** Unpack a boundary `[x, y]` slice into a `Vec2`. */
const unpackVec2 = (raw: Float64Array): Vec2 => {
  const values = [...raw];
  return { x: pick(values, 0), y: pick(values, 1) };
};

/** A `Rect` as the boundary `[x, y, w, h]` bounds slice. */
const packRect = (rect: Rect): Float64Array => Float64Array.from([rect.x, rect.y, rect.width, rect.height]);

/** Flatten a `Rect[]` to the boundary `[x, y, w, h, …]` slice (the flip-book frames). */
const flattenRects = (rects: readonly Rect[]): Float64Array =>
  Float64Array.from(rects.flatMap((rect): readonly number[] => [rect.x, rect.y, rect.width, rect.height]));

/** Unpack a boundary `[x, y, w, h]` slice into a `Rect` (the sampled flip-book frame). */
const unpackRect = (raw: Float64Array): Rect => {
  const values = [...raw];
  return { height: pick(values, W_INDEX), width: pick(values, Z_INDEX), x: pick(values, 0), y: pick(values, 1) };
};

/** A `Circle` as the boundary `[centerX, centerY, radius]` slice. */
const packCircle = (circle: Circle): Float64Array =>
  Float64Array.from([circle.center.x, circle.center.y, circle.radius]);

/** Pack an `Rgba`'s linear `[r, g, b]` channels into the boundary colour slice (alpha dropped — native lit colour). */
const packRgb = (color: Rgba): Float64Array => Float64Array.from([...color].slice(0, RGB_LENGTH));

/** Parse the wasm host channel's opaque session-params JSON object string into the contract record (the byte boundary is untyped). */
const parseParams = (json: string): SessionConfig["params"] => JSON.parse(json) as SessionConfig["params"];

/** Unpack a boundary `[x, y, z]` slice into a `Vec3`. */
const unpackVec3 = (raw: Float64Array): Vec3 => {
  const values = [...raw];
  return { x: pick(values, 0), y: pick(values, 1), z: pick(values, Z_INDEX) };
};

/** A boundary `Mat4` is the 16 column-major numbers verbatim. */
const unpackMat4 = (raw: Float64Array): Mat4 => [...raw];

/** Unpack a boundary `[x, y, z, w]` slice into a `Quat`. */
const unpackQuat = (raw: Float64Array): Quat => {
  const values = [...raw];
  return [pick(values, 0), pick(values, 1), pick(values, Z_INDEX), pick(values, W_INDEX)] as Quat;
};

/** Flatten a `GridField`'s passability mask to the boundary byte array. */
const maskOf = (field: GridField): Uint8Array => Uint8Array.from(field.passable.map(Number));

/** A cell as the boundary `[x, y]` int slice. */
const cellSlice = (cell: Cell): Int32Array => Int32Array.from([cell.x, cell.y]);

/** Reshape a flat `[x0, y0, x1, y1, …]` result into `Cell[]`. */
const toCells = (raw: Float64Array): readonly Cell[] => {
  const values = [...raw];
  return Array.from({ length: values.length / CELL_STRIDE }, (_unused, index): Cell => ({
    x: pick(values, index * CELL_STRIDE),
    y: pick(values, index * CELL_STRIDE + 1),
  }));
};

/** The 2D math + predicate `HostBridge` ops (SPEC-03 §4.2): the `v2` algebra, the scalar `lerp`, and the pure `Aabb`/`Sphere` predicates, every one forwarding to the native math (one deterministic source of truth). */
const math2dBridge = (game: WasmHostExport): Pick<
  HostBridge,
  | "v2Add" | "v2Sub" | "v2Scale" | "v2Dot" | "v2Len" | "v2Normalize" | "v2Dist" | "v2Lerp"
  | "lerp" | "aabbOverlap" | "pointInRect" | "circleOverlap"
> => ({
  aabbOverlap: (lhs: Rect, rhs: Rect): boolean => game.aabbOverlap(packRect(lhs), packRect(rhs)),
  circleOverlap: (lhs: Circle, rhs: Circle): boolean =>
    game.circleOverlap(packCircle(lhs), packCircle(rhs)),
  lerp: (start: number, end: number, fraction: number): number => game.lerp(start, end, fraction),
  pointInRect: (point: Vec2, rect: Rect): boolean => game.pointInRect(packVec2(point), packRect(rect)),
  v2Add: (lhs: Vec2, rhs: Vec2): Vec2 => unpackVec2(game.v2Add(packVec2(lhs), packVec2(rhs))),
  v2Dist: (lhs: Vec2, rhs: Vec2): number => game.v2Dist(packVec2(lhs), packVec2(rhs)),
  v2Dot: (lhs: Vec2, rhs: Vec2): number => game.v2Dot(packVec2(lhs), packVec2(rhs)),
  v2Len: (vector: Vec2): number => game.v2Len(packVec2(vector)),
  v2Lerp: (lhs: Vec2, rhs: Vec2, fraction: number): Vec2 =>
    unpackVec2(game.v2Lerp(packVec2(lhs), packVec2(rhs), fraction)),
  v2Normalize: (vector: Vec2): Vec2 => unpackVec2(game.v2Normalize(packVec2(vector))),
  v2Scale: (vector: Vec2, scalar: number): Vec2 => unpackVec2(game.v2Scale(packVec2(vector), scalar)),
  v2Sub: (lhs: Vec2, rhs: Vec2): Vec2 => unpackVec2(game.v2Sub(packVec2(lhs), packVec2(rhs))),
});

/** The 3D math `HostBridge` ops (SPEC-11 §4.2): the `v3`/`mat4`/`quat` algebra + the scalar `clamp`/`normalizeAngle`, every one forwarding to the native `MathApi`. */
const mathCoreBridge = (game: WasmHostExport): Pick<
  HostBridge,
  | "v3Add" | "v3Sub" | "v3Scale" | "v3Dot" | "v3Cross" | "v3Len" | "v3Normalize" | "v3Dist" | "v3Lerp"
  | "mat4Identity" | "mat4Multiply" | "mat4Perspective" | "mat4LookAt" | "mat4Invert" | "mat4FromTRS"
  | "quatIdentity" | "quatFromEuler" | "quatMultiply" | "quatNormalize" | "quatToMat4"
  | "clamp" | "normalizeAngle"
> => ({
  clamp: (value: number, low: number, high: number): number => game.clamp(value, low, high),
  mat4FromTRS: (translation: Vec3, rotation: Quat, scale: Vec3): Mat4 =>
    unpackMat4(game.mat4FromTRS(packVec3(translation), Float64Array.from(rotation), packVec3(scale))),
  mat4Identity: (): Mat4 => unpackMat4(game.mat4Identity()),
  // Deferred: no general 4x4 inverse in axiom-math (see header) — inert identity.
  mat4Invert: (): Mat4 => IDENTITY_MAT4,
  mat4LookAt: (eye: Vec3, target: Vec3, up: Vec3): Mat4 =>
    unpackMat4(game.mat4LookAt(packVec3(eye), packVec3(target), packVec3(up))),
  mat4Multiply: (lhs: Mat4, rhs: Mat4): Mat4 =>
    unpackMat4(game.mat4Multiply(Float64Array.from(lhs), Float64Array.from(rhs))),
  mat4Perspective: (spec: PerspectiveSpec): Mat4 =>
    unpackMat4(game.mat4Perspective(spec.fovY, spec.aspect, spec.near, spec.far)),
  normalizeAngle: (angle: number): number => game.normalizeAngle(angle),
  quatFromEuler: (pitch: number, yaw: number, roll: number): Quat =>
    unpackQuat(game.quatFromEuler(pitch, yaw, roll)),
  quatIdentity: (): Quat => unpackQuat(game.quatIdentity()),
  quatMultiply: (lhs: Quat, rhs: Quat): Quat =>
    unpackQuat(game.quatMultiply(Float64Array.from(lhs), Float64Array.from(rhs))),
  quatNormalize: (quaternion: Quat): Quat => unpackQuat(game.quatNormalize(Float64Array.from(quaternion))),
  quatToMat4: (quaternion: Quat): Mat4 => unpackMat4(game.quatToMat4(Float64Array.from(quaternion))),
  v3Add: (lhs: Vec3, rhs: Vec3): Vec3 => unpackVec3(game.v3Add(packVec3(lhs), packVec3(rhs))),
  v3Cross: (lhs: Vec3, rhs: Vec3): Vec3 => unpackVec3(game.v3Cross(packVec3(lhs), packVec3(rhs))),
  v3Dist: (lhs: Vec3, rhs: Vec3): number => game.v3Dist(packVec3(lhs), packVec3(rhs)),
  v3Dot: (lhs: Vec3, rhs: Vec3): number => game.v3Dot(packVec3(lhs), packVec3(rhs)),
  v3Len: (vector: Vec3): number => game.v3Len(packVec3(vector)),
  v3Lerp: (lhs: Vec3, rhs: Vec3, fraction: number): Vec3 =>
    unpackVec3(game.v3Lerp(packVec3(lhs), packVec3(rhs), fraction)),
  v3Normalize: (vector: Vec3): Vec3 => unpackVec3(game.v3Normalize(packVec3(vector))),
  v3Scale: (vector: Vec3, scalar: number): Vec3 => unpackVec3(game.v3Scale(packVec3(vector), scalar)),
  v3Sub: (lhs: Vec3, rhs: Vec3): Vec3 => unpackVec3(game.v3Sub(packVec3(lhs), packVec3(rhs))),
});

/** The whole math `HostBridge` (the 3D-core + 2D groups), split so each forwarder group stays within the per-function line budget; composed here into the full math surface. */
const mathBridge = (game: WasmHostExport): Pick<
  HostBridge,
  | "v2Add" | "v2Sub" | "v2Scale" | "v2Dot" | "v2Len" | "v2Normalize" | "v2Dist" | "v2Lerp"
  | "v3Add" | "v3Sub" | "v3Scale" | "v3Dot" | "v3Cross" | "v3Len" | "v3Normalize" | "v3Dist" | "v3Lerp"
  | "mat4Identity" | "mat4Multiply" | "mat4Perspective" | "mat4LookAt" | "mat4Invert" | "mat4FromTRS"
  | "quatIdentity" | "quatFromEuler" | "quatMultiply" | "quatNormalize" | "quatToMat4"
  | "clamp" | "lerp" | "normalizeAngle"
  | "aabbOverlap" | "pointInRect" | "circleOverlap"
> => Object.assign(mathCoreBridge(game), math2dBridge(game));

/** The grid `HostBridge` ops (SPEC-06), forwarding to the native `axiom-grid` BFS / wavefront core. */
const gridBridge = (game: WasmHostExport): Pick<
  HostBridge,
  "gridPath" | "gridReachable" | "gridDistanceField" | "gridStepToward"
> => ({
  gridDistanceField: (field: GridField, start: Cell): readonly number[] => [
    ...game.gridDistanceField(field.cols, field.rows, maskOf(field), cellSlice(start)),
  ],
  gridPath: (field: GridField, start: Cell, goal: Cell): readonly Cell[] | undefined => {
    const cells = toCells(game.gridPath(field.cols, field.rows, maskOf(field), cellSlice(start), cellSlice(goal)));
    // An empty result means unreachable, which the contract maps to the empty Result.
    return pick<() => readonly Cell[] | undefined>(
      [(): readonly Cell[] | undefined => absent<readonly Cell[]>(), (): readonly Cell[] => cells],
      Number(cells.length > 0),
    )();
  },
  gridReachable: (field: GridField, start: Cell, goal: Cell): boolean =>
    game.gridReachable(field.cols, field.rows, maskOf(field), cellSlice(start), cellSlice(goal)),
  gridStepToward: (field: GridField, from: Cell, target: Cell): Cell => {
    const step = [...game.gridStepToward(field.cols, field.rows, maskOf(field), cellSlice(from), cellSlice(target))];
    return { x: pick(step, 0), y: pick(step, 1) };
  },
});

/** The audio `HostBridge` ops (SPEC-08), forwarding to the native `axiom-audio` mixer core. */
const audioBridge = (game: WasmHostExport): Pick<
  HostBridge,
  "loadSound" | "playSound" | "stopVoice" | "playMusic" | "playTone" | "scheduleSound" | "setMasterVolume" | "setMuted"
> => ({
  loadSound: (url: string): Handle => game.loadSound(url),
  playMusic: (urls: readonly string[], opts?: MusicOptions): Handle => {
    const options = orElse(opts, {});
    return game.playMusic(urls, orElse(options.loop, NO_LOOP), orElse(options.crossfadeSeconds, NO_CROSSFADE));
  },
  playSound: (id: Handle, opts?: SoundOptions): Handle => {
    const options = orElse(opts, {});
    return game.playSound(
      id,
      orElse(options.volume, FULL_VOLUME),
      orElse(options.pitch, UNCHANGED_PITCH),
      orElse(options.loop, NO_LOOP),
    );
  },
  playTone: (spec: ToneSpec): Handle =>
    game.playTone(WAVE_KINDS.indexOf(spec.wave), spec.freq, spec.duration, orElse(spec.volume, FULL_VOLUME)),
  scheduleSound: (id: Handle, atSeconds: number, opts?: ScheduleOptions): Handle =>
    game.scheduleSound(id, atSeconds, orElse(orElse(opts, {}).volume, FULL_VOLUME)),
  setMasterVolume: (volume: number): void => {
    game.setMasterVolume(volume);
  },
  setMuted: (muted: boolean): void => {
    game.setMuted(muted);
  },
  stopVoice: (voice: Handle): void => {
    game.stopVoice(voice);
  },
});

/** One colour channel (`0..1`) as its `0..255` byte. */
const byteOf = (channel: number): number => Math.round(channel * CHANNEL_MAX);

/** Pack an `Rgba` into the boundary `0xRRGGBBAA` u32 by positional scale (no bitwise). */
const packRgba = (color: Rgba): number =>
  [...color].reduce((packed, channel, index): number => packed + byteOf(channel) * pick(RGBA_SCALES, index), 0);

/** Flatten an `EmitterConfig` to the boundary `[count, lifetime, speed, spread, gravityX, gravityY, size, colorStart, colorEnd, layer]` slice. */
const packEmitter = (config: EmitterConfig): Float64Array => {
  const gravity = orElse(config.gravity, NO_GRAVITY);
  return Float64Array.from([
    config.count,
    config.lifetimeSeconds,
    config.speed,
    config.spread,
    gravity.x,
    gravity.y,
    config.size,
    packRgba(config.colorStart),
    packRgba(config.colorEnd),
    orElse(config.layer, DEFAULT_LAYER),
  ]);
};

/** Flatten a `SpriteOpts` to the boundary `[posX, posY, rotation, scaleX, scaleY, anchorX, anchorY, srcX, srcY, srcW, srcH, tintRGBA, flipX, flipY, layer, alpha]` slice, defaulting every optional host-side. */
const packSpriteOpts = (opts: SpriteOpts): Float64Array => {
  const scale = orElse(opts.scale, UNIT_SCALE);
  const anchor = orElse(opts.anchor, TOP_LEFT_ANCHOR);
  const source = orElse(opts.source, WHOLE_TEXTURE);
  return Float64Array.from([
    opts.pos.x,
    opts.pos.y,
    orElse(opts.rotation, NO_ROTATION_SPRITE),
    scale.x,
    scale.y,
    anchor.x,
    anchor.y,
    source.x,
    source.y,
    source.width,
    source.height,
    packRgba(orElse(opts.tint, WHITE_TINT)),
    Number(orElse(opts.flipX, false)),
    Number(orElse(opts.flipY, false)),
    orElse(opts.layer, DEFAULT_LAYER),
    orElse(opts.alpha, FULL_ALPHA),
  ]);
};

/** Flatten a `TextOpts` to the boundary `[posX, posY, fontSize, colorRGBA, align, layer, alpha]` slice; `align` resolves to its dense index by table position. */
const packTextOpts = (opts: TextOpts): Float64Array =>
  Float64Array.from([
    opts.pos.x,
    opts.pos.y,
    opts.font.size,
    packRgba(opts.color),
    ALIGN_NAMES.indexOf(orElse(opts.align, "left")),
    orElse(opts.layer, DEFAULT_LAYER),
    orElse(opts.alpha, FULL_ALPHA),
  ]);

/** The 2D drawing `HostBridge` ops (SPEC-04 §10), every one forwarding to the native `axiom-draw2d` builder via the Wave-2 `draw2d*` exports. */
/** The default radians an ellipse's omitted `rotation` resolves to (axis-aligned). */
const NO_ROTATION = 0;

/** The 2D shape `HostBridge` ops (SPEC-04 §10): camera + the filled/stroked shapes + the self-coloured line, every one packing its colours to `0xRRGGBBAA` and defaulting `stroke`/`strokeWidth`/`layer`/`alpha` host-side. */
const draw2dShapeBridge = (game: WasmHostExport): Pick<
  HostBridge,
  "draw2dCamera2d" | "draw2dRect" | "draw2dCircle" | "draw2dEllipse" | "draw2dLine"
> => ({
  draw2dCamera2d: (center: Vec2, zoom: number): void => {
    game.draw2dCamera2d(packVec2(center), zoom);
  },
  draw2dCircle: (center: Vec2, radius: number, style: ShapeStyle): void => {
    game.draw2dCircle(
      packVec2(center),
      radius,
      packRgba(style.fill),
      packRgba(orElse(style.stroke, TRANSPARENT)),
      orElse(style.strokeWidth, NO_STROKE_WIDTH),
      orElse(style.layer, DEFAULT_LAYER),
      orElse(style.alpha, FULL_ALPHA),
    );
  },
  draw2dEllipse: (center: Vec2, radii: EllipseRadii, style: ShapeStyle): void => {
    game.draw2dEllipse(
      Float64Array.from([center.x, center.y, radii.rx, radii.ry, orElse(radii.rotation, NO_ROTATION)]),
      packRgba(style.fill),
      packRgba(orElse(style.stroke, TRANSPARENT)),
      orElse(style.strokeWidth, NO_STROKE_WIDTH),
      orElse(style.layer, DEFAULT_LAYER),
      orElse(style.alpha, FULL_ALPHA),
    );
  },
  draw2dLine: (from: Vec2, to: Vec2, style: LineStyle): void => {
    game.draw2dLine(
      packVec2(from),
      packVec2(to),
      packRgba(style.color),
      style.width,
      orElse(style.layer, DEFAULT_LAYER),
      orElse(style.alpha, FULL_ALPHA),
    );
  },
  draw2dRect: (bounds: Rect, style: ShapeStyle): void => {
    game.draw2dRect(
      packRect(bounds),
      packRgba(style.fill),
      packRgba(orElse(style.stroke, TRANSPARENT)),
      orElse(style.strokeWidth, NO_STROKE_WIDTH),
      orElse(style.layer, DEFAULT_LAYER),
      orElse(style.alpha, FULL_ALPHA),
    );
  },
});

/** The 2D sprite + text `HostBridge` ops (SPEC-04 §4.2): the textured sprite, the monospace text run, and the deterministic `measureText`, each marshalling its opts record to the boundary slice. */
const draw2dTextBridge = (game: WasmHostExport): Pick<
  HostBridge,
  "draw2dSprite" | "draw2dText" | "draw2dMeasureText"
> => ({
  draw2dMeasureText: (value: string, font: FontSpec): TextMetrics => {
    const extent = [...game.draw2dMeasureText(value, font.size)];
    return { height: pick(extent, 1), width: pick(extent, 0) };
  },
  draw2dSprite: (texture: TextureId, opts: SpriteOpts): void => {
    game.draw2dSprite(texture, packSpriteOpts(opts));
  },
  draw2dText: (value: string, opts: TextOpts): void => {
    game.draw2dText(value, packTextOpts(opts));
  },
});

/** The 2D particle + render-target + finalize `HostBridge` ops (SPEC-04 §10.1 / §10.3), forwarding to the native `axiom-draw2d` builder. */
const draw2dSystemBridge = (game: WasmHostExport): Pick<
  HostBridge,
  | "draw2dCreateEmitter" | "draw2dEmit" | "draw2dAdvanceParticles"
  | "draw2dCreateRenderTarget" | "draw2dBeginTarget" | "draw2dEndTarget" | "draw2dTargetTexture" | "draw2dFinish"
  | "draw2dSampleAnimation"
> => ({
  draw2dAdvanceParticles: (dtSeconds: number): void => {
    game.draw2dAdvanceParticles(dtSeconds);
  },
  draw2dBeginTarget: (target: Handle): void => {
    game.draw2dBeginTarget(target);
  },
  draw2dCreateEmitter: (config: EmitterConfig): Handle => game.draw2dCreateEmitter(packEmitter(config)),
  draw2dCreateRenderTarget: (width: number, height: number): Handle => game.draw2dCreateRenderTarget(width, height),
  draw2dEmit: (id: Handle, at: Vec2, direction: Vec2): void => {
    game.draw2dEmit(id, packVec2(at), packVec2(direction));
  },
  draw2dEndTarget: (): void => {
    game.draw2dEndTarget();
  },
  draw2dFinish: (): readonly number[] => [...game.draw2dFinish()],
  draw2dSampleAnimation: (anim: SpriteAnimation, elapsedSeconds: number, looping: boolean): Rect =>
    unpackRect(game.draw2dSampleAnimation(flattenRects(anim.frames), anim.fps, elapsedSeconds, looping)),
  draw2dTargetTexture: (target: Handle): Handle => game.draw2dTargetTexture(target),
});

/** The whole 2D drawing `HostBridge` (SPEC-04 §10): the shape verbs + the particle/render-target/finalize verbs, every one forwarding to the native `axiom-draw2d` builder via the Wave-2.5 `draw2d*` exports. */
const draw2dBridge = (game: WasmHostExport): Pick<
  HostBridge,
  | "draw2dCamera2d" | "draw2dRect" | "draw2dCircle" | "draw2dEllipse" | "draw2dLine"
  | "draw2dSprite" | "draw2dText" | "draw2dMeasureText"
  | "draw2dCreateEmitter" | "draw2dEmit" | "draw2dAdvanceParticles"
  | "draw2dCreateRenderTarget" | "draw2dBeginTarget" | "draw2dEndTarget" | "draw2dTargetTexture" | "draw2dFinish"
  | "draw2dSampleAnimation"
> => Object.assign(draw2dShapeBridge(game), draw2dTextBridge(game), draw2dSystemBridge(game));

/** The presentation-asset `HostBridge` ops (SPEC-04 §10): `loadTexture` mints a stable handle over the native registry (the app resolves the pixels); `loadFont` returns the built-in monospace `FontSpec` (Tier-0 ships one family). */
const assetBridge = (game: WasmHostExport): Pick<HostBridge, "loadTexture" | "loadFont"> => ({
  loadFont: (): FontSpec => DEFAULT_FONT,
  loadTexture: (url: string): TextureId => game.loadTexture(url),
});

/** The 3D scene-authoring `HostBridge` ops (SPEC-11), forwarding to the native runtime scene authoring on `RunningApp` (`add_mesh` / `add_material` / `set_camera` / `add_light` / `spawn` / `set::<Transform>` / `set::<Bounds>` / `reauthor`). */
const scene3dBridge = (game: WasmHostExport): Pick<
  HostBridge,
  | "createMesh"
  | "createMaterial"
  | "setCamera3D"
  | "addLight"
  | "spawnRenderable"
  | "setNodeTransform"
  | "setNodeBounds"
  | "clearScene"
  | "createController"
  | "controlFirstPerson"
> => ({
  // Directional arm only: the native `add_light` mints a `DirectionalLight` (the `kind` discriminant is dropped — see header).
  addLight: (light: LightDescriptor): Entity =>
    game.addLight(packVec3(light.vector), packRgb(light.color), light.intensity),
  clearScene: (): void => {
    game.clearScene();
  },
  controlFirstPerson: (input: ControllerInput): void => {
    game.controlFirstPerson(input.index, packVec3(input.moveLocal), input.yawDelta, input.pitchDelta);
  },
  createController: (spec: ControllerSpec, index: number): Entity =>
    game.spawnController(packVec3(spec.position), spec.fovY * RAD_TO_DEG, spec.near, spec.far, index),
  // Base colour only: emissive / roughness / opacity are dropped (native lit-colour authoring — see header).
  createMaterial: (material: MaterialDescriptor): Handle => game.createMaterial(packRgb(material.baseColor)),
  createMesh: (meshKind: number): Handle => game.createMesh(pick(MESH_NAMES, meshKind)),
  // Eye position + look-at target + degree FOV + near/far: the native `set_camera` aims from position toward target (world up = +Y — see header).
  setCamera3D: (camera: CameraDescriptor): void => {
    game.setCamera3D(
      packVec3(camera.position),
      packVec3(camera.target),
      camera.fovY * RAD_TO_DEG,
      camera.near,
      camera.far,
    );
  },
  setNodeBounds: (entity: Entity, halfExtents: Vec3): void => {
    game.setNodeBounds(entity, packVec3(halfExtents));
  },
  setNodeTransform: (entity: Entity, transform: Transform): void => {
    game.setNodeTransform(entity, packTransform(transform));
  },
  spawnRenderable: (mesh: Handle, material: Handle, transform: Transform): Entity =>
    game.spawnRenderable(mesh, material, packTransform(transform)),
});

/** The spatial-query `HostBridge` ops (SPEC-03), forwarding to the native `axiom-scene` Entity-addressed query surface (`overlap_circle` / `overlap_box` / `raycast_hit`). */
const queryBridge = (game: WasmHostExport): Pick<
  HostBridge,
  "overlapCircle" | "overlapBox" | "raycast"
> => ({
  overlapBox: (center: Vec3, halfExtents: Vec3): readonly Entity[] => [
    ...game.overlapBox(packVec3(center), packVec3(halfExtents)),
  ],
  // The 2D circle query lifts to the scene's z=0 plane (the native query is a sphere over committed bounds).
  overlapCircle: (centerX: number, centerY: number, radius: number): readonly Entity[] => [
    ...game.overlapCircle(Float64Array.from([centerX, centerY, 0]), radius),
  ],
  raycast: (origin: Vec3, direction: Vec3, maxDistance: number): Result<RayHit> => {
    const originSlice = packVec3(origin);
    const raw = [...game.raycast(originSlice, packVec3(direction), maxDistance)];
    /*
     * Empty = a miss (the empty `Result`); else `[entity, x, y, z]`. The native
     * export omits the distance, so it is closed here with the native `v3Dist` —
     * the one math source of truth.
     */
    return pick<() => Result<RayHit>>(
      [
        (): Result<RayHit> => absent<RayHit>(),
        (): RayHit => {
          const point = unpackVec3(Float64Array.from(raw.slice(1)));
          return { distance: game.v3Dist(originSlice, packVec3(point)), entity: pick(raw, 0), point };
        },
      ],
      Number(raw.length > 0),
    )();
  },
});

/** The embed host-channel `HostBridge` ops (SPEC-12) + the input `bindAction` (SPEC-05), forwarding to the wasm host channel. */
const channelBridge = (game: WasmHostExport): Pick<
  HostBridge,
  "bindAction" | "getSessionConfig" | "notifyReady" | "reportOutcome" | "reportOutcomes"
> => ({
  bindAction: (action: string, keys: readonly string[]): void => {
    game.bindAction(action, keys);
  },
  getSessionConfig: (): SessionConfig => ({
    params: parseParams(game.sessionParams()),
    seed: seedFromHalves(game.seed_lo, game.seed_hi),
  }),
  notifyReady: (): void => {
    game.notifyReady();
  },
  // The single terminal outcome (`metrics` is presentation-only, not carried by the native channel).
  reportOutcome: (outcome: Outcome): void => {
    game.report_outcome(outcome.won, outcome.score);
  },
  // Re-flush the single latched outcome; the per-player `results` map is dropped (single-outcome channel — see header).
  reportOutcomes: (): void => {
    game.reportOutcomes();
  },
});

/** The screen-space UI `HostBridge` ops (SPEC-09), forwarding to the native `axiom-interface` `UiSurface` + `axiom-layout::solve` via the Wave-2 `ui*` exports. Colours pack to `0xRRGGBBAA`; `stroke`/`strokeWidth` default host-side; `uiViewport` unpacks `[width, height]`. */
const uiBridge = (game: WasmHostExport): UiBridge => ({
  uiBeginFrame: (viewport: UiViewport, pointer: Vec2, pressed: boolean): void => {
    game.uiBeginFrame(Float64Array.from([viewport.width, viewport.height]), packVec2(pointer), pressed);
  },
  uiButton: (bounds: Rect, label: string, style: UiStyle): boolean =>
    game.uiButton(
      packRect(bounds),
      label,
      packRgba(style.fill),
      packRgba(orElse(style.stroke, TRANSPARENT)),
      orElse(style.strokeWidth, NO_STROKE_WIDTH),
    ),
  uiDrawList: (): Uint8Array => game.uiDrawList(),
  uiRect: (bounds: Rect, style: UiStyle): void => {
    game.uiRect(
      packRect(bounds),
      packRgba(style.fill),
      packRgba(orElse(style.stroke, TRANSPARENT)),
      orElse(style.strokeWidth, NO_STROKE_WIDTH),
    );
  },
  uiSolveLayout: (viewport: UiViewport, nodes: readonly number[]): readonly number[] => [
    ...game.uiSolveLayout(viewport.width, viewport.height, Float64Array.from(nodes)),
  ],
  uiSprite: (texture: Handle, bounds: Rect): void => {
    game.uiSprite(texture, packRect(bounds));
  },
  uiText: (value: string, opts: UiTextOpts): void => {
    game.uiText(value, Float64Array.from([opts.x, opts.y]), packRgba(opts.color), opts.size);
  },
  uiViewport: (): UiViewport => {
    const size = [...game.uiViewport()];
    return { height: pick(size, 1), width: pick(size, 0) };
  },
});

/**
 * Build the installed `HostBridge` from the raw `WasmGame` exports — the host
 * counterpart of `bridgeFromWasm`. The app calls `bindNative(hostFromWasm(game))`
 * once at boot so the free authoring surface projects through the live wasm core.
 * The eight groups partition the `HostBridge` keys, so their `Object.assign`
 * intersection is exactly a `HostBridge` (no cast, no banned object spread). The
 * audio + scene/query/channel/ui groups are folded into nested inner assigns so each
 * `Object.assign` stays within its typed (≤4-source) overload (math/grid/draw2d +
 * the inner result = four outer sources).
 */
export const hostFromWasm = (game: WasmHostExport): HostBridge =>
  Object.assign(
    mathBridge(game),
    gridBridge(game),
    draw2dBridge(game),
    Object.assign(
      audioBridge(game),
      scene3dBridge(game),
      queryBridge(game),
      Object.assign(channelBridge(game), uiBridge(game), assetBridge(game)),
    ),
  );
