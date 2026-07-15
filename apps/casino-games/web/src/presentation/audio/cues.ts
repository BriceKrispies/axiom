/*
 * cues.ts — the procedural audio language, phrased entirely as engine
 * `ToneSpec` values (WebAudio oscillator + envelope; original tones, nothing
 * sampled or imitated). Pitch variation draws from the AUDIO stream, so it can
 * never touch an outcome. The mount harness scales volumes by the player's
 * sound settings before playing.
 */

import type { ToneSpec } from "@axiom/web-engine";
import type { Rarity } from "../../chance-engine/configuration/schema.ts";
import { sample01 } from "../../chance-engine/randomness/streams.ts";

const vary = (freq: number, seed: number, key: number): number =>
  freq * (1 + (sample01(seed, "audio", key) - 0.5) * 0.06);

/** Hover/step blip for selectable objects. */
export const hoverCue = (seed: number, key: number): readonly ToneSpec[] => [
  { duration: 0.03, freq: vary(880, seed, key), volume: 0.05, wave: "sine" },
];

/** The player committed (button press, release, drop). */
export const commitCue = (seed: number, key: number): readonly ToneSpec[] => [
  { duration: 0.07, freq: vary(520, seed, key), volume: 0.16, wave: "square" },
  { delay: 0.06, duration: 0.06, freq: vary(700, seed, key + 1), volume: 0.1, wave: "triangle" },
];

/** A mechanical tick (wheel divider, conveyor step, dial notch). */
export const tickCue = (seed: number, key: number): readonly ToneSpec[] => [
  { duration: 0.025, freq: vary(1300, seed, key), volume: 0.07, wave: "square" },
];

/** Mechanism thump (chest latch, vault bolt, elevator stop, rock strike). */
export const thumpCue = (seed: number, key: number): readonly ToneSpec[] => [
  { duration: 0.09, freq: vary(150, seed, key), volume: 0.3, wave: "square" },
];

/** Anticipation shimmer right before the reveal lands. */
export const shimmerCue = (seed: number, key: number): readonly ToneSpec[] => [
  { duration: 0.14, freq: vary(1560, seed, key), volume: 0.06, wave: "sine" },
  { delay: 0.07, duration: 0.14, freq: vary(1960, seed, key + 1), volume: 0.05, wave: "sine" },
];

const ARPEGGIOS: Readonly<Record<Rarity, readonly number[]>> = {
  common: [523, 659],
  jackpot: [523, 659, 784, 1047, 1319],
  rare: [523, 659, 784, 1047],
  uncommon: [523, 659, 784],
};

/** The win fanfare — a rising major arpeggio sized by rarity. */
export const winCue = (rarity: Rarity, seed: number): readonly ToneSpec[] =>
  ARPEGGIOS[rarity].map((freq, i) => ({
    delay: i * 0.07,
    duration: 0.18,
    freq: vary(freq, seed, 40 + i),
    volume: 0.24,
    wave: "triangle",
  }));

/** The friendly non-win response: short, soft, and downward but warm. */
export const tryAgainCue = (seed: number): readonly ToneSpec[] => [
  { duration: 0.1, freq: vary(392, seed, 50), volume: 0.12, wave: "triangle" },
  { delay: 0.09, duration: 0.14, freq: vary(330, seed, 51), volume: 0.1, wave: "triangle" },
];
