/*
 * engine/audio.ts — a tiny WebAudio tone synth: `playTone` fires one oscillator
 * of the requested wave/frequency through a gain envelope (a ~5 ms attack so the
 * onset doesn't click, then an exponential decay over the tone's duration) and
 * auto-stops it. One `AudioContext` is created lazily on the first call and
 * reused forever; if `AudioContext` doesn't exist (headless / node) every call
 * is a silent no-op, so importing this module never requires a DOM. A suspended
 * context (browser autoplay policy) gets a fire-and-forget `resume()` — the
 * first user-gesture-driven tone unlocks it. No queues, no assets.
 */

import type { ToneSpec } from "./api.ts";

/** Default peak gain when `ToneSpec.volume` is omitted. */
const DEFAULT_VOLUME = 0.15;

/** Attack time in seconds (fast enough to feel instant, slow enough not to click). */
const ATTACK_S = 0.005;

/** The floor the exponential decay ramps to (exponentialRamp cannot reach 0). */
const DECAY_FLOOR = 0.0001;

/** The one lazily-created shared context (undefined until the first tone). */
let sharedCtx: AudioContext | undefined;

/** Play one procedural tone. Silent no-op where WebAudio is unavailable. */
export function playTone(spec: ToneSpec): void {
  if (typeof AudioContext === "undefined") {
    return;
  }
  sharedCtx ??= new AudioContext();
  const ctx = sharedCtx;
  if (ctx.state === "suspended") {
    // Autoplay policy: fire-and-forget; a rejection just means still locked.
    void ctx.resume().catch(() => {
      /* remains suspended until a qualifying user gesture */
    });
  }

  const duration = Math.max(spec.duration, ATTACK_S * 2);
  const peak = spec.volume ?? DEFAULT_VOLUME;
  const t0 = ctx.currentTime;

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
