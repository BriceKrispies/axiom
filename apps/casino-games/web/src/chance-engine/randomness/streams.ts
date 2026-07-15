/*
 * streams.ts — deterministic, PURE random streams for the chance engine.
 *
 * Every random decision in Casino Games is a pure function of
 * (rootSeed, stream purpose, integer keys). There is no hidden RNG state, no
 * `Math.random()`, and no draw order to get wrong: two callers can never
 * perturb each other because each draw names exactly what it is for.
 *
 * The purposes are the engine's independence invariant: the GAMEPLAY stream
 * decides outcomes; PLACEMENT positions winning objects; TIER picks reward
 * tiers; TRAJECTORY shapes committed physical paths; AMBIENT drives idle
 * dances; PARTICLES drives celebration debris; AUDIO varies tone pitch; CAMERA
 * drives cinematic wobble. Adding one extra sparkle draws only from PARTICLES,
 * so it can never change who wins — the determinism tests pin this.
 */

/** The named, independent random streams. Decorative purposes must never feed
 * an outcome decision; the resolver draws only from "gameplay"/"placement"/"tier". */
export type StreamPurpose =
  | "gameplay"
  | "placement"
  | "tier"
  | "trajectory"
  | "ambient"
  | "particles"
  | "audio"
  | "camera";

export const STREAM_PURPOSES: readonly StreamPurpose[] = [
  "gameplay",
  "placement",
  "tier",
  "trajectory",
  "ambient",
  "particles",
  "audio",
  "camera",
];

/** One 32-bit avalanche round (the murmur3/splitmix finalizer family). */
const mix32 = (value: number): number => {
  let h = value >>> 0;
  h = Math.imul(h ^ (h >>> 16), 0x21f0aaad) >>> 0;
  h = Math.imul(h ^ (h >>> 15), 0x735a2d97) >>> 0;
  return (h ^ (h >>> 15)) >>> 0;
};

/** FNV-1a over the purpose name, so each stream owns a distinct seed space. */
const purposeHash = (purpose: string): number => {
  let h = 0x811c9dc5;
  for (let i = 0; i < purpose.length; i += 1) {
    h = Math.imul(h ^ purpose.charCodeAt(i), 0x01000193) >>> 0;
  }
  return h >>> 0;
};

/** The derived 32-bit seed of one named stream (recorded in the audit record). */
export const streamSeed = (rootSeed: number, purpose: StreamPurpose): number =>
  mix32((rootSeed >>> 0) ^ purposeHash(purpose));

/**
 * The core draw: a uniform float in [0, 1) that is a pure function of the root
 * seed, the stream purpose, and the integer `keys` (round number, object index,
 * draw label…). Keys must be integers — fractional keys are truncated.
 */
export const sample01 = (rootSeed: number, purpose: StreamPurpose, ...keys: readonly number[]): number => {
  let h = streamSeed(rootSeed, purpose);
  for (const key of keys) {
    h = mix32((h + Math.imul(key | 0, 0x9e3779b1)) >>> 0);
  }
  return h / 4_294_967_296;
};

/** A uniform float in [min, max). */
export const sampleRange = (
  min: number,
  max: number,
  rootSeed: number,
  purpose: StreamPurpose,
  ...keys: readonly number[]
): number => min + sample01(rootSeed, purpose, ...keys) * (max - min);

/** A uniform integer in [0, n). */
export const sampleInt = (n: number, rootSeed: number, purpose: StreamPurpose, ...keys: readonly number[]): number =>
  Math.floor(sample01(rootSeed, purpose, ...keys) * n);

/** True with probability `p`. */
export const sampleChance = (p: number, rootSeed: number, purpose: StreamPurpose, ...keys: readonly number[]): boolean =>
  sample01(rootSeed, purpose, ...keys) < p;

/** A uniformly-picked element of `items` (items must be non-empty). */
export const samplePick = <T>(
  items: readonly T[],
  rootSeed: number,
  purpose: StreamPurpose,
  ...keys: readonly number[]
): T => items[sampleInt(items.length, rootSeed, purpose, ...keys)] as T;

/**
 * A deterministic Fisher–Yates shuffle of `items` drawn from one stream. The
 * swap for position `i` draws with the extra key `i`, so the permutation is a
 * pure function of (seed, purpose, keys).
 */
export const shuffled = <T>(
  items: readonly T[],
  rootSeed: number,
  purpose: StreamPurpose,
  ...keys: readonly number[]
): readonly T[] => {
  const out = [...items];
  for (let i = out.length - 1; i > 0; i -= 1) {
    const j = sampleInt(i + 1, rootSeed, purpose, ...keys, i);
    const held = out[i] as T;
    out[i] = out[j] as T;
    out[j] = held;
  }
  return out;
};

/** The presentation seed carried inside a committed outcome: reveal/celebration
 * animation derives from THIS value, so an injected outcome fully pins its own
 * presentation and a seeded round replays bit-for-bit. */
export const presentationSeedOf = (rootSeed: number, round: number): number =>
  mix32(streamSeed(rootSeed, "trajectory") + Math.imul(round | 0, 0x9e3779b1));
