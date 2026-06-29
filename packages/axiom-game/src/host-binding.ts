/*
 * The installed host channel behind the SDK's FREE authoring functions — the
 * `bindAction`/`clamp`/`getSessionConfig`/`reportOutcome` surface that is not
 * scoped to a `Sim` or a `Scene` and so has nowhere to receive a bridge as an
 * argument. The runtime app installs its native channel once at boot via
 * `bindNative`; the free functions read it back here. This mirrors the
 * Wave-0 `defaultRegistry` that backs the free `onFixedUpdate`/`onRender`.
 *
 * `HostBridge` is the subset of the native seam the free surface needs. The real
 * runtime-app bridge implements both this and `NativeBridge` on one object; a
 * test installs a fake. Before `bindNative`, an inert default makes every free
 * call a safe no-op returning a neutral value, so the surface never throws on an
 * unbound host — it is simply silent until the app binds it.
 *
 * Session state (the bound bridge and the terminal-outcome latch) lives here in
 * one place: `bindNative` opens a fresh session, so it also clears the latch.
 */

import type {
  CameraDescriptor,
  GridField,
  LightDescriptor,
  MaterialDescriptor,
  PerspectiveSpec,
} from "./host-descriptors.ts";
import type { Cell, Entity, Handle, Mat4, PlayerId, Quat, RayHit, Result, Vec3 } from "./vocabulary.ts";
import { type Draw2dBridge, UNBOUND_DRAW2D } from "./draw2d-binding.ts";
import { UNBOUND_HOST_BASE } from "./unbound-host.ts";

/** Host-supplied session configuration: a seed plus opaque parameters (SPEC-12). */
export interface SessionConfig {
  readonly seed: bigint;
  readonly params: Record<string, string | number>;
}

/** Per-voice playback options (SPEC-08); each field defaults host-side when absent. */
export interface SoundOptions {
  readonly volume?: number;
  readonly pitch?: number;
  readonly loop?: boolean;
}

/** Music-playlist options (SPEC-08): loop the list and crossfade between tracks. */
export interface MusicOptions {
  readonly loop?: boolean;
  readonly crossfadeSeconds?: number;
}

/** An ADSR amplitude envelope for a synthesized tone (SPEC-08). */
export interface ToneEnvelope {
  readonly attack: number;
  readonly decay: number;
  readonly sustain: number;
  readonly release: number;
}

/** A low-frequency oscillator modulating a tone's frequency (SPEC-08). */
export interface ToneLfo {
  readonly freq: number;
  readonly depth: number;
}

/** A neutral synthesis description — wave kind as a field, never a branch (SPEC-08). */
export interface ToneSpec {
  readonly wave: "sawtooth" | "sine" | "square" | "triangle";
  readonly freq: number;
  readonly duration: number;
  readonly envelope?: ToneEnvelope;
  readonly volume?: number;
  readonly lfo?: ToneLfo;
}

/** Scheduled-playback options (SPEC-08): the gain to start a deferred voice at. */
export interface ScheduleOptions {
  readonly volume?: number;
}

/** The terminal result of a game / a player's room (SPEC-12 §15). */
export interface Outcome {
  readonly won: boolean;
  readonly score: number;
  readonly metrics?: Record<string, number>;
}

/** The native channel the free authoring functions project (SPEC-03/05/12 §4.2). Extends the 2D drawing channel (SPEC-04, `draw2d-binding.ts`). */
export interface HostBridge extends Draw2dBridge {
  /** Constrain `value` to `[low, high]` (native `MathApi`). */
  readonly clamp: (value: number, low: number, high: number) => number;
  /** Wrap `angle` to `(-π, π]` (native `MathApi`). */
  readonly normalizeAngle: (angle: number) => number;
  /** Entities whose committed transform overlaps the circle, in stable order (SPEC-03). */
  readonly overlapCircle: (centerX: number, centerY: number, radius: number) => readonly Entity[];
  /** Entities whose committed bounds overlap the query box, in stable order (SPEC-03). */
  readonly overlapBox: (center: Vec3, halfExtents: Vec3) => readonly Entity[];
  /** The nearest bounded ray hit, or the empty value on a miss (SPEC-03). */
  readonly raycast: (origin: Vec3, direction: Vec3, maxDistance: number) => Result<RayHit>;
  /** Bind an action name to the physical `keys` that trigger it (SPEC-05). */
  readonly bindAction: (action: string, keys: readonly string[]) => void;
  /** The host's session configuration, constant for the whole session (SPEC-12). */
  readonly getSessionConfig: () => SessionConfig;
  /** Signal that the first frame can render (SPEC-12). */
  readonly notifyReady: () => void;
  /** Forward the single terminal outcome to the host channel (SPEC-12). */
  readonly reportOutcome: (outcome: Outcome) => void;
  /** Forward the per-player room outcomes to the host channel (SPEC-12 §16.6). */
  readonly reportOutcomes: (results: Readonly<Record<PlayerId, Outcome>>) => void;

  // Audio (SPEC-08): presentation-side; handles are opaque, never read back into sim.
  /** Register a sound asset by URL, returning its handle immediately (app owns fetch/decode). */
  readonly loadSound: (url: string) => Handle;
  /** Start a voice playing sound `id`; return the voice handle. */
  readonly playSound: (id: Handle, opts?: SoundOptions) => Handle;
  /** Stop a playing voice (a stale handle is a clean no-op). */
  readonly stopVoice: (voice: Handle) => void;
  /** Start a music playlist (crossfaded), returning its voice handle. */
  readonly playMusic: (urls: readonly string[], opts?: MusicOptions) => Handle;
  /** Synthesize and play a tone from its neutral spec; return the voice handle. */
  readonly playTone: (spec: ToneSpec) => Handle;
  /** Schedule sound `id` to start at `atSeconds` on the audio clock; return the voice handle. */
  readonly scheduleSound: (id: Handle, atSeconds: number, opts?: ScheduleOptions) => Handle;
  /** Set the master output gain in `[0, 1]`. */
  readonly setMasterVolume: (volume: number) => void;
  /** Mute or unmute all output. */
  readonly setMuted: (muted: boolean) => void;

  // Grid / pathfinding (SPEC-06): the native `axiom-grid` core owns the BFS/wavefront; the projection feeds it a `GridField` (dims + passability mask) and forwards the cell sequence / distances. Pure functions of their args.
  /** The shortest cell path `start`→`goal`, or the empty value when unreachable. */
  readonly gridPath: (field: GridField, start: Cell, goal: Cell) => Result<readonly Cell[]>;
  /** Whether `goal` is reachable from `start`. */
  readonly gridReachable: (field: GridField, start: Cell, goal: Cell) => boolean;
  /** The row-major BFS distance field from `start` (`Infinity` at unreachable cells). */
  readonly gridDistanceField: (field: GridField, start: Cell) => readonly number[];
  /** The single best next cell stepping `from` toward `target` (stays put with no passable neighbour). */
  readonly gridStepToward: (field: GridField, from: Cell, target: Cell) => Cell;

  // 3D scene authoring (SPEC-11): mesh/material/camera/light marshal to the existing scene/render facades; handles are opaque, kinds are dense table indices the projection resolves from the contract's string discriminant.
  /** Create a primitive mesh by its dense kind index (0=box, 1=sphere, 2=cylinder); return its handle. */
  readonly createMesh: (meshKind: number) => Handle;
  /** Create a lit material from its resolved descriptor; return its handle. */
  readonly createMaterial: (material: MaterialDescriptor) => Handle;
  /** Build the perspective camera node (look-at + intrinsics) from its descriptor. */
  readonly setCamera3D: (camera: CameraDescriptor) => void;
  /** Add a light from its descriptor; return its entity. */
  readonly addLight: (light: LightDescriptor) => Entity;

  // 3D math (SPEC-11): every `v3`/`mat4`/`quat` op routes here — the native `MathApi` is the ONE deterministic source of truth (SPEC-03 §3.2); never a TS re-implementation.
  /** Vector sum. */
  readonly v3Add: (lhs: Vec3, rhs: Vec3) => Vec3;
  /** Vector difference. */
  readonly v3Sub: (lhs: Vec3, rhs: Vec3) => Vec3;
  /** Scalar multiple of a vector. */
  readonly v3Scale: (vector: Vec3, scalar: number) => Vec3;
  /** Dot product. */
  readonly v3Dot: (lhs: Vec3, rhs: Vec3) => number;
  /** Cross product. */
  readonly v3Cross: (lhs: Vec3, rhs: Vec3) => Vec3;
  /** Euclidean length. */
  readonly v3Len: (vector: Vec3) => number;
  /** Unit vector in the same direction. */
  readonly v3Normalize: (vector: Vec3) => Vec3;
  /** Distance between two points. */
  readonly v3Dist: (lhs: Vec3, rhs: Vec3) => number;
  /** Linear blend between two vectors. */
  readonly v3Lerp: (lhs: Vec3, rhs: Vec3, fraction: number) => Vec3;
  /** The 4×4 identity matrix. */
  readonly mat4Identity: () => Mat4;
  /** Matrix product `lhs · rhs`. */
  readonly mat4Multiply: (lhs: Mat4, rhs: Mat4) => Mat4;
  /** A right-handed perspective projection matrix from its spec. */
  readonly mat4Perspective: (spec: PerspectiveSpec) => Mat4;
  /** A right-handed look-at view matrix. */
  readonly mat4LookAt: (eye: Vec3, target: Vec3, up: Vec3) => Mat4;
  /** Matrix inverse. */
  readonly mat4Invert: (matrix: Mat4) => Mat4;
  /** A TRS (translate · rotate · scale) composition matrix. */
  readonly mat4FromTRS: (translation: Vec3, rotation: Quat, scale: Vec3) => Mat4;
  /** The identity quaternion. */
  readonly quatIdentity: () => Quat;
  /** A quaternion from intrinsic Euler angles (radians). */
  readonly quatFromEuler: (pitch: number, yaw: number, roll: number) => Quat;
  /** Quaternion product (composition of rotations). */
  readonly quatMultiply: (lhs: Quat, rhs: Quat) => Quat;
  /** The unit quaternion in the same direction. */
  readonly quatNormalize: (quaternion: Quat) => Quat;
  /** The rotation matrix of a quaternion. */
  readonly quatToMat4: (quaternion: Quat) => Mat4;
}

/** The full inert channel: the non-2D defaults (`unbound-host.ts`) composed with the inert 2D surface, so `boundHost()` is a total `HostBridge` before any `bindNative`. */
const UNBOUND_HOST: HostBridge = Object.assign(UNBOUND_HOST_BASE, UNBOUND_DRAW2D);

/** The mutable session: the bound host and whether a terminal outcome was emitted. */
const session: { host: HostBridge; outcomeEmitted: boolean } = {
  host: UNBOUND_HOST,
  outcomeEmitted: false,
};

/*
 * Install the runtime app's native host channel and open a fresh session. The
 * app calls this once at boot; tests call it in setup to inject a fake. Opening
 * a session clears the terminal-outcome latch.
 */
export const bindNative = (bridge: HostBridge): void => {
  session.host = bridge;
  session.outcomeEmitted = false;
};

/** The currently bound host (the inert default before `bindNative`). */
export const boundHost = (): HostBridge => session.host;

/*
 * Latch the terminal outcome: returns `true` exactly once per session (the first
 * call) and `false` thereafter, so a game cannot report two terminal states
 * (SPEC-12 §4.2 emit-exactly-once).
 */
export const latchOutcome = (): boolean => {
  const first = !session.outcomeEmitted;
  session.outcomeEmitted = true;
  return first;
};
