/*
 * idle.ts — the shared, deterministic "ready-stance" idle animation. It maps a part's
 * semantic TAG to a subtle per-part `PoseDelta` (a slow breathing pulse on the
 * torso/core, a matching settle on the head, and a weapon-ready sway on the arms),
 * driven ONLY by a caller-supplied `tick` + a stable per-figure `phase`. It reads no
 * wall-clock and holds no state, so the same (tag, tick, phase) is byte-identical
 * every time — the same idle plays in the gameplay arena and in the Figure Lab
 * gallery through the one `FigureInstance.animateIdle` path, never a screen-local copy.
 *
 * Presentation only: this layers on top of the rest pose inside `composeWorld` and can
 * never affect simulation state (it is downstream of, and invisible to, `src/sim/`).
 */

import { type Quat, quatFromEulerXyz, vec3 } from "../vec3.ts";
import type { PoseDelta } from "../compose.ts";
import type { SemanticPart } from "../parts.ts";

/** Breathing angular rate per tick (~0.35 Hz at 60 Hz) — slow and calm. */
const BREATH_RATE = 0.037;
/** The arms sway a touch slower than the breath for a living, uncorrelated feel. */
const SWAY_RATE = 0.033;

const breatheScale = (breath: number): PoseDelta => ({ scale: 1 + 0.02 * breath, pos: vec3(0, 0.016 * breath, 0) });

/**
 * The idle `PoseDelta` for one part, or `undefined` when the part holds still. Pure
 * and deterministic: a function of the part's semantic role, the tick, and a stable
 * per-figure phase offset (so group-mates do not breathe in lockstep).
 */
export const idleDeltaForTag = (tag: SemanticPart, tick: number, phase: number): PoseDelta | undefined => {
  const breath = Math.sin(tick * BREATH_RATE + phase);
  const sway = Math.sin(tick * SWAY_RATE + phase + 0.6);

  const chest: readonly SemanticPart[] = ["torso", "shell", "core", "body", "stem"];
  const crown: readonly SemanticPart[] = ["head", "face", "cap", "crest"];

  if (chest.includes(tag)) {
    return breatheScale(breath);
  }
  if (crown.includes(tag)) {
    // A small nod that also rides the chest lift, so the head stays seated.
    return { rot: quatFromEulerXyz(0.03 * breath, 0, 0), pos: vec3(0, 0.012 * breath, 0) };
  }
  if (tag === "upper_arm") {
    // Weapon-ready: a steady inward/outward sway about the roll axis.
    return { rot: rollQuat(0.05 * sway) };
  }
  if (tag === "shoulder") {
    return { rot: rollQuat(0.02 * breath) };
  }
  if (tag === "wing") {
    // A slow, wide flap keeps floating/winged silhouettes alive.
    return { rot: rollQuat(0.08 * sway) };
  }
  return undefined;
};

const rollQuat = (angle: number): Quat => quatFromEulerXyz(0, 0, angle);
