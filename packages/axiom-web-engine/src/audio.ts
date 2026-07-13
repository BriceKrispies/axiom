/*
 * audio.ts — a tiny WebAudio layer: `playTone` fires one oscillator of
 * the requested wave/frequency through a gain envelope (a ~5 ms attack so the
 * onset doesn't click, then an exponential decay over the tone's duration,
 * optionally delayed a few hundred ms so one event can play a two-note figure)
 * and auto-stops it; `startAmbience`/`setAmbienceLevel`/`stopAmbience` run one
 * looping low-passed noise bed (a neutral room-tone ambience) whose gain the
 * consumer can swell.
 *
 * One `AudioContext` is created lazily on the first call and reused forever; if
 * `AudioContext` doesn't exist (headless / node) every call is a silent no-op,
 * so importing this module never requires a DOM. A suspended context (browser
 * autoplay policy) gets a fire-and-forget `resume()` — the first
 * user-gesture-driven call unlocks it. The ambience noise is generated once
 * with a deterministic LCG (no `Math.random`). No queues, no assets.
 */

import type { ToneSpec } from "./api.ts";

/** Default peak gain when `ToneSpec.volume` is omitted. */
const DEFAULT_VOLUME = 0.15;

/** Attack time in seconds (fast enough to feel instant, slow enough not to click). */
const ATTACK_S = 0.005;

/** The floor the exponential decay ramps to (exponentialRamp cannot reach 0). */
const DECAY_FLOOR = 0.0001;

/** The one lazily-created shared context (undefined until the first call). */
let sharedCtx: AudioContext | undefined;

const context = (): AudioContext | undefined => {
  if (typeof AudioContext === "undefined") {
    return undefined;
  }
  sharedCtx ??= new AudioContext();
  if (sharedCtx.state === "suspended") {
    // Autoplay policy: fire-and-forget; a rejection just means still locked.
    void sharedCtx.resume().catch(() => {
      /* remains suspended until a qualifying user gesture */
    });
  }
  return sharedCtx;
};

/** Play one procedural tone. Silent no-op where WebAudio is unavailable. */
export function playTone(spec: ToneSpec): void {
  const ctx = context();
  if (ctx === undefined) {
    return;
  }
  const duration = Math.max(spec.duration, ATTACK_S * 2);
  const peak = spec.volume ?? DEFAULT_VOLUME;
  const t0 = ctx.currentTime + Math.max(0, spec.delay ?? 0);

  const osc = ctx.createOscillator();
  osc.type = spec.wave;
  osc.frequency.value = spec.freq;

  const gain = ctx.createGain();
  gain.gain.setValueAtTime(DECAY_FLOOR, t0);
  gain.gain.linearRampToValueAtTime(peak, t0 + ATTACK_S);
  gain.gain.exponentialRampToValueAtTime(DECAY_FLOOR, t0 + duration);

  osc.connect(gain);
  gain.connect(ctx.destination);
  osc.start(t0);
  osc.stop(t0 + duration + 0.02);
}

// ── ambience (one looping low-passed noise bed) ───────────────────────────────

let ambienceSource: AudioBufferSourceNode | undefined;
let ambienceGain: GainNode | undefined;

/** Start the looping room-tone bed at `volume` (0..1). Idempotent. */
export function startAmbience(volume: number): void {
  const ctx = context();
  if (ctx === undefined || ambienceSource !== undefined) {
    return;
  }
  // Two seconds of deterministic pink-ish noise (LCG; no Math.random), softened
  // by averaging so it reads as a distant room, then low-passed further.
  const length = ctx.sampleRate * 2;
  const buffer = ctx.createBuffer(1, length, ctx.sampleRate);
  const data = buffer.getChannelData(0);
  let seed = 0x2f6e2b1 >>> 0;
  let smooth = 0;
  for (let i = 0; i < length; i += 1) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const white = seed / 0xffffffff - 0.5;
    smooth = smooth * 0.97 + white * 0.03;
    data[i] = smooth * 6;
  }

  const source = ctx.createBufferSource();
  source.buffer = buffer;
  source.loop = true;
  const filter = ctx.createBiquadFilter();
  filter.type = "lowpass";
  filter.frequency.value = 420;
  const gain = ctx.createGain();
  gain.gain.value = volume;
  source.connect(filter);
  filter.connect(gain);
  gain.connect(ctx.destination);
  source.start();
  ambienceSource = source;
  ambienceGain = gain;
}

/** Adjust the ambience gain (e.g. a swell in response to an event). */
export function setAmbienceLevel(volume: number): void {
  if (ambienceGain !== undefined && sharedCtx !== undefined) {
    ambienceGain.gain.setTargetAtTime(Math.max(0, volume), sharedCtx.currentTime, 0.1);
  }
}

/** Stop and drop the ambience loop (teardown / hot reload). */
export function stopAmbience(): void {
  ambienceSource?.stop();
  ambienceSource?.disconnect();
  ambienceGain?.disconnect();
  ambienceSource = undefined;
  ambienceGain = undefined;
}
