/*
 * intensity.ts — the celebration ladder. One profile per outcome class, scaled
 * by the config's `celebrationIntensity` and the player's particle/shake
 * settings. Non-winning rounds get a gentle, brief, positive response — never
 * punitive; jackpot stays the largest while remaining readable and bounded.
 */

import type { Rarity } from "../../chance-engine/configuration/schema.ts";

export interface CelebrationProfile {
  /** Confetti/sparkle particle budget BEFORE the particle-scale setting. */
  readonly particles: number;
  /** Whether the reward beam appears. */
  readonly beam: boolean;
  /** Camera shake magnitude in world units (0 = none). */
  readonly shake: number;
  /** Celebration length in ticks at presentationSpeed 1. */
  readonly durationTicks: number;
  /** Reveal-focus camera pull strength [0, 1]. */
  readonly focusPull: number;
}

const PROFILES: Readonly<Record<Rarity | "loss", CelebrationProfile>> = {
  common: { beam: false, durationTicks: 80, focusPull: 0.25, particles: 18, shake: 0 },
  jackpot: { beam: true, durationTicks: 190, focusPull: 0.55, particles: 120, shake: 0.05 },
  loss: { beam: false, durationTicks: 45, focusPull: 0.1, particles: 6, shake: 0 },
  rare: { beam: true, durationTicks: 150, focusPull: 0.45, particles: 70, shake: 0.035 },
  uncommon: { beam: true, durationTicks: 110, focusPull: 0.35, particles: 36, shake: 0 },
};

/** Hard cap: no celebration ever exceeds this many particles. */
export const PARTICLE_CAP = 140;

export interface CelebrationTuning {
  readonly celebrationIntensity: number;
  readonly particleScale: number;
  readonly cameraShake: boolean;
  readonly reducedMotion: boolean;
  readonly presentationSpeed: number;
}

export interface ResolvedCelebration extends CelebrationProfile {
  readonly rarity: Rarity | "loss";
}

/** Resolve the profile for an outcome under the current tuning. Reduced motion
 * shortens and softens (scale/light emphasis instead of shake) but preserves
 * the reveal sequence — feedback is never simply disabled. */
export const celebrationOf = (rarity: Rarity | "loss", tuning: CelebrationTuning): ResolvedCelebration => {
  const base = PROFILES[rarity];
  const motion = tuning.reducedMotion ? 0.5 : 1;
  return {
    beam: base.beam,
    durationTicks: Math.max(30, Math.round((base.durationTicks * motion) / tuning.presentationSpeed)),
    focusPull: base.focusPull * (tuning.reducedMotion ? 0.4 : 1),
    particles: Math.min(
      PARTICLE_CAP,
      Math.round(base.particles * tuning.celebrationIntensity * tuning.particleScale * (tuning.reducedMotion ? 0.5 : 1)),
    ),
    rarity,
    shake: tuning.cameraShake && !tuning.reducedMotion ? base.shake * tuning.celebrationIntensity : 0,
  };
};
