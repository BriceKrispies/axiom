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

import type { Entity, PlayerId } from "./vocabulary.ts";

/** Host-supplied session configuration: a seed plus opaque parameters (SPEC-12). */
export interface SessionConfig {
  readonly seed: bigint;
  readonly params: Record<string, string | number>;
}

/** The terminal result of a game / a player's room (SPEC-12 §15). */
export interface Outcome {
  readonly won: boolean;
  readonly score: number;
  readonly metrics?: Record<string, number>;
}

/** The native channel the free authoring functions project (SPEC-03/05/12 §4.2). */
export interface HostBridge {
  /** Constrain `value` to `[low, high]` (native `MathApi`). */
  readonly clamp: (value: number, low: number, high: number) => number;
  /** Wrap `angle` to `(-π, π]` (native `MathApi`). */
  readonly normalizeAngle: (angle: number) => number;
  /** Entities whose committed transform overlaps the circle, in stable order. */
  readonly overlapCircle: (centerX: number, centerY: number, radius: number) => readonly Entity[];
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
}

/** The seed reported before a host binds — a neutral, inert default. */
const UNBOUND_SEED = 0n;

/*
 * The inert host used before `bindNative`: every read returns a neutral value
 * and every signal is a no-op. This keeps the free surface total (no `null`
 * bridge to branch on) and makes "called before the app bound a host" a quiet,
 * observable no-op rather than a crash.
 */
const UNBOUND_HOST: HostBridge = {
  bindAction: (): void => {
    // No-op until a host is bound
  },
  clamp: (value: number): number => value,
  getSessionConfig: (): SessionConfig => ({ params: {}, seed: UNBOUND_SEED }),
  normalizeAngle: (angle: number): number => angle,
  notifyReady: (): void => {
    // No-op until a host is bound
  },
  overlapCircle: (): readonly Entity[] => [],
  reportOutcome: (): void => {
    // No-op until a host is bound
  },
  reportOutcomes: (): void => {
    // No-op until a host is bound
  },
};

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
