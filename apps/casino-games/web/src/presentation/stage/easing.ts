/*
 * easing.ts — the shared animation-timing vocabulary. Every reveal, pop,
 * settle, and camera ease in Casino Games phrases its motion through these
 * pure curves; no game hand-rolls its own timing math.
 */

export const clamp01 = (t: number): number => Math.min(1, Math.max(0, t));

export const lerp = (a: number, b: number, t: number): number => a + (b - a) * t;

export const smoothstep = (t: number): number => {
  const x = clamp01(t);
  return x * x * (3 - 2 * x);
};

export const easeOutCubic = (t: number): number => 1 - (1 - clamp01(t)) ** 3;

export const easeInCubic = (t: number): number => clamp01(t) ** 3;

/** Overshoot-and-settle (lid pops, card lands). */
export const easeOutBack = (t: number): number => {
  const x = clamp01(t) - 1;
  const c = 1.70158;
  return 1 + x * x * ((c + 1) * x + c);
};

/** A springy settle with two visible bounces (dice, dropped prizes). */
export const easeOutElastic = (t: number): number => {
  const x = clamp01(t);
  if (x === 0 || x === 1) {
    return x;
  }
  return 2 ** (-10 * x) * Math.sin((x * 10 - 0.75) * ((2 * Math.PI) / 3)) + 1;
};

/** Phase progress in [0, 1] given a phase age in ticks and a duration. */
export const progress = (ageTicks: number, durationTicks: number): number =>
  clamp01(durationTicks <= 0 ? 1 : ageTicks / durationTicks);

/** A single 0→1→0 pulse over a duration (anticipation squash, button flash). */
export const pulse = (t: number): number => Math.sin(Math.PI * clamp01(t));

/** Gentle idle bob in [-1, 1] from a tick clock (deterministic, no wall time). */
export const bob = (tick: number, periodTicks: number, phase = 0): number =>
  Math.sin(((tick / periodTicks) * 2 + phase) * Math.PI);
