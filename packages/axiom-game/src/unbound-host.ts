/*
 * The inert `HostBridge` non-2D defaults used before `bindNative` — every read
 * returns a neutral value and every signal is a no-op. Kept in its own module so
 * `host-binding.ts` stays within the 300-line budget, the same partition reason
 * `draw2d-binding.ts` was split out. The 2D surface (`UNBOUND_DRAW2D`) composes
 * onto this base in `host-binding.ts`.
 */

import type { Cell, Entity, Handle, Mat4, Quat, RayHit, Result, Vec3 } from "./vocabulary.ts";
import { IDENTITY_MAT4, IDENTITY_QUAT, ORIGIN_CELL, ZERO_VEC3 } from "./host-descriptors.ts";
import type { SessionConfig } from "./host-binding.ts";

/** The seed reported before a host binds — a neutral, inert default. */
const UNBOUND_SEED = 0n;

/** The handle returned by handle-minting reads before a host binds (a null handle). */
const UNBOUND_HANDLE = 0;

/** The neutral scalar an inert numeric math read returns before a host binds. */
const UNBOUND_SCALAR = 0;

/** The absent `Result` value (no `undefined` literal — the lint bans it). */
const absent = <Value>(slot?: Value): Value | undefined => slot;

/*
 * The inert host's non-2D defaults: every read returns a neutral value and every
 * signal is a no-op. This keeps the free surface total (no `null` bridge to branch
 * on) and makes "called before the app bound a host" a quiet, observable no-op
 * rather than a crash. `host-binding.ts` composes the inert 2D surface
 * (`UNBOUND_DRAW2D`) onto this base, the same Object.assign partition the wasm host
 * adapter uses.
 */
export const UNBOUND_HOST_BASE = {
  addLight: (): Entity => UNBOUND_HANDLE,
  bindAction: (): void => {
    // No-op until a host is bound
  },
  clamp: (value: number): number => value,
  createMaterial: (): Handle => UNBOUND_HANDLE,
  createMesh: (): Handle => UNBOUND_HANDLE,
  getSessionConfig: (): SessionConfig => ({ params: {}, seed: UNBOUND_SEED }),
  gridDistanceField: (): readonly number[] => [],
  gridPath: (): Result<readonly Cell[]> => [],
  gridReachable: (): boolean => false,
  gridStepToward: (): Cell => ORIGIN_CELL,
  loadSound: (): Handle => UNBOUND_HANDLE,
  mat4FromTRS: (): Mat4 => IDENTITY_MAT4,
  mat4Identity: (): Mat4 => IDENTITY_MAT4,
  mat4Invert: (): Mat4 => IDENTITY_MAT4,
  mat4LookAt: (): Mat4 => IDENTITY_MAT4,
  mat4Multiply: (): Mat4 => IDENTITY_MAT4,
  mat4Perspective: (): Mat4 => IDENTITY_MAT4,
  normalizeAngle: (angle: number): number => angle,
  notifyReady: (): void => {
    // No-op until a host is bound
  },
  overlapBox: (): readonly Entity[] => [],
  overlapCircle: (): readonly Entity[] => [],
  playMusic: (): Handle => UNBOUND_HANDLE,
  playSound: (): Handle => UNBOUND_HANDLE,
  playTone: (): Handle => UNBOUND_HANDLE,
  quatFromEuler: (): Quat => IDENTITY_QUAT,
  quatIdentity: (): Quat => IDENTITY_QUAT,
  quatMultiply: (): Quat => IDENTITY_QUAT,
  quatNormalize: (): Quat => IDENTITY_QUAT,
  quatToMat4: (): Mat4 => IDENTITY_MAT4,
  raycast: (): Result<RayHit> => absent<RayHit>(),
  reportOutcome: (): void => {
    // No-op until a host is bound
  },
  reportOutcomes: (): void => {
    // No-op until a host is bound
  },
  scheduleSound: (): Handle => UNBOUND_HANDLE,
  setCamera3D: (): void => {
    // No-op until a host is bound
  },
  setMasterVolume: (): void => {
    // No-op until a host is bound
  },
  setMuted: (): void => {
    // No-op until a host is bound
  },
  stopVoice: (): void => {
    // No-op until a host is bound
  },
  v3Add: (): Vec3 => ZERO_VEC3,
  v3Cross: (): Vec3 => ZERO_VEC3,
  v3Dist: (): number => UNBOUND_SCALAR,
  v3Dot: (): number => UNBOUND_SCALAR,
  v3Len: (): number => UNBOUND_SCALAR,
  v3Lerp: (): Vec3 => ZERO_VEC3,
  v3Normalize: (): Vec3 => ZERO_VEC3,
  v3Scale: (): Vec3 => ZERO_VEC3,
  v3Sub: (): Vec3 => ZERO_VEC3,
};
