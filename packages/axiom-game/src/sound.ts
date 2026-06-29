/*
 * The audio free functions (SPEC-08 §4.2). Audio is presentation-side and not
 * scoped to a `Sim` or a `Scene`, so — like `clamp`/`getSessionConfig` — these are
 * FREE functions projected through the installed `HostBridge` (`host-binding.ts`):
 * the runtime app binds its Web Audio channel once via `bindNative`, and these
 * forward to it. `loadSound` returns a handle immediately; the fetch/decode is the
 * app's job (it owns the network and the wasm marshalling).
 *
 * Every function is a one-line forward, including the optional `opts` (which the
 * host defaults wave-side): there is no branch here because there is no decision —
 * the projection only relays the call. No audio value is ever read back into a
 * sim-class API (SPEC-08 §6 presentation exclusion); these all return opaque
 * handles or nothing.
 */

import {
  type MusicOptions,
  type ScheduleOptions,
  type SoundOptions,
  type ToneSpec,
  boundHost,
} from "./host-binding.ts";
import type { Handle } from "./vocabulary.ts";

/** Register a sound asset by URL, returning its handle immediately (SPEC-08 §4.2). */
export const loadSound = (url: string): Handle => boundHost().loadSound(url);

/** Start a voice playing sound `id`; return the voice handle (SPEC-08 §4.2). */
export const playSound = (id: Handle, opts?: SoundOptions): Handle =>
  boundHost().playSound(id, opts);

/** Stop a playing voice (SPEC-08 §4.2). */
export const stopVoice = (voice: Handle): void => {
  boundHost().stopVoice(voice);
};

/** Start a crossfaded music playlist; return its voice handle (SPEC-08 §4.2). */
export const playMusic = (urls: readonly string[], opts?: MusicOptions): Handle =>
  boundHost().playMusic(urls, opts);

/** Synthesize and play a tone from its spec; return the voice handle (SPEC-08 §4.2). */
export const playTone = (spec: ToneSpec): Handle => boundHost().playTone(spec);

/** Schedule sound `id` to start at `atSeconds` on the audio clock (SPEC-08 §4.2). */
export const scheduleSound = (id: Handle, atSeconds: number, opts?: ScheduleOptions): Handle =>
  boundHost().scheduleSound(id, atSeconds, opts);

/** Set the master output gain in `[0, 1]` (SPEC-08 §4.2). */
export const setMasterVolume = (volume: number): void => {
  boundHost().setMasterVolume(volume);
};

/** Mute or unmute all output (SPEC-08 §4.2). */
export const setMuted = (muted: boolean): void => {
  boundHost().setMuted(muted);
};
