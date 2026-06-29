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

import type { Entity, Handle, PlayerId } from "./vocabulary.ts";

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
}

/** The seed reported before a host binds — a neutral, inert default. */
const UNBOUND_SEED = 0n;

/** The handle returned by handle-minting reads before a host binds (a null handle). */
const UNBOUND_HANDLE = 0;

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
  loadSound: (): Handle => UNBOUND_HANDLE,
  normalizeAngle: (angle: number): number => angle,
  notifyReady: (): void => {
    // No-op until a host is bound
  },
  overlapCircle: (): readonly Entity[] => [],
  playMusic: (): Handle => UNBOUND_HANDLE,
  playSound: (): Handle => UNBOUND_HANDLE,
  playTone: (): Handle => UNBOUND_HANDLE,
  reportOutcome: (): void => {
    // No-op until a host is bound
  },
  reportOutcomes: (): void => {
    // No-op until a host is bound
  },
  scheduleSound: (): Handle => UNBOUND_HANDLE,
  setMasterVolume: (): void => {
    // No-op until a host is bound
  },
  setMuted: (): void => {
    // No-op until a host is bound
  },
  stopVoice: (): void => {
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
