/*
 * Impact effects — a faithful port of `penalty_effects.rs`. Every descriptor is a
 * pure function of (kind, tick, impact pose, detail, award total, celebrate). No
 * trig: oscillation comes from small lookup tables. Drives net wobble (Goal),
 * post/crossbar shake (Post), ball deflection/drift (Save/Miss), crowd bounce,
 * additive camera juice, a result banner + score popup, and a save impact flash.
 */

import { type Vec3, ZERO, add, lerp, vec3 } from "./engine.ts";
import type { ResultDetail, ResultKind } from "./result.ts";
import { GOAL_HALF_WIDTH, GOAL_HEIGHT, GOAL_LINE_Z, GROUND_Y, NET_DEPTH } from "./scene-constants.ts";

export type EffectKind = "Goal" | "Save" | "Post" | "Miss" | "SessionComplete";

const NET_WOBBLE_COLS = 5;
const NET_WOBBLE_ROWS = 4;
const WOBBLE_AMPLITUDE = 0.35;
const DEFLECT_TICKS = 20;
const CAMERA_SHAKE_TICKS = 16;
const POP_TICKS = 12;

const OSC_LUT = [0, 7, 10, 7, 0, -7, -10, -7];
const BOUNCE_LUT = [0, 6, 10, 9, 6, 3, 1, 0];
const SHAKE_LUT = [10, -8, 6, -4];

const osc = (i: number): number => OSC_LUT[((i % 8) + 8) % 8]! / 10;
const bounce = (i: number): number => BOUNCE_LUT[((i % 8) + 8) % 8]! / 10;
const shake = (i: number): number => SHAKE_LUT[((i % 4) + 4) % 4]! / 100;
const decay = (tick: number, span: number): number => (tick < span ? (span - tick) / span : 0);

const durationOf = (kind: EffectKind): number =>
  kind === "Goal" ? 72 : kind === "Save" ? 54 : kind === "Post" ? 54 : kind === "Miss" ? 42 : 90;

export const bannerText = (kind: EffectKind): string => (kind === "SessionComplete" ? "FINAL SCORE" : kind.toUpperCase());

export const kindFromResult = (kind: ResultKind): EffectKind => kind;

// ── state ────────────────────────────────────────────────────────────────────

export interface ImpactEffectState {
  readonly kind: EffectKind;
  readonly tick: number;
  readonly impact: Vec3;
  readonly detail: ResultDetail;
  readonly awardTotal: number;
  readonly celebrate: boolean;
}

export const effectForResult = (kind: ResultKind, detail: ResultDetail, impact: Vec3, awardTotal: number): ImpactEffectState => ({
  kind: kindFromResult(kind),
  tick: 0,
  impact,
  detail,
  awardTotal,
  celebrate: false,
});

export const effectSessionComplete = (finalScore: number): ImpactEffectState => ({
  kind: "SessionComplete",
  tick: 0,
  impact: ZERO,
  detail: "Scored",
  awardTotal: finalScore,
  celebrate: finalScore > 0,
});

export const effectAdvanced = (effect: ImpactEffectState): ImpactEffectState => ({ ...effect, tick: effect.tick + 1 });

// ── descriptor ───────────────────────────────────────────────────────────────

export interface WobbleNode {
  readonly base: Vec3;
  readonly displaced: Vec3;
}
export interface EffectDescriptor {
  readonly kind: EffectKind;
  readonly tick: number;
  readonly netWobble: { rear: WobbleNode[]; front: WobbleNode[] } | null;
  readonly frameShake: { target: "LeftPost" | "RightPost" | "Crossbar"; offset: Vec3 } | null;
  readonly ballDeflection: { current: Vec3 } | null;
  readonly crowdAmplitude: number;
  readonly cameraOffset: Vec3;
  readonly banner: { text: string; scale: number; pulse: number };
  readonly scorePopup: { points: number; scale: number } | null;
  readonly foreground: { position: Vec3; size: number; alpha: number }[];
}

const wobblePanel = (impact: Vec3, tick: number, z: number): WobbleNode[] => {
  const nodes: WobbleNode[] = [];
  for (let row = 0; row < NET_WOBBLE_ROWS; row += 1) {
    for (let col = 0; col < NET_WOBBLE_COLS; col += 1) {
      const ordinal = row * NET_WOBBLE_COLS + col;
      const x = -GOAL_HALF_WIDTH + GOAL_HALF_WIDTH * 2 * (col / (NET_WOBBLE_COLS - 1));
      const y = GROUND_Y + GOAL_HEIGHT * (row / (NET_WOBBLE_ROWS - 1));
      const base = vec3(x, y, z);
      const dx = x - impact.x;
      const dy = y - impact.y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      const falloff = 1 / (1 + dist * 1.2);
      const disp = WOBBLE_AMPLITUDE * decay(tick, 72) * falloff * osc(tick + ordinal);
      nodes.push({ base, displaced: add(base, vec3(0, 0, -disp)) });
    }
  }
  return nodes;
};

const netWobbleOf = (state: ImpactEffectState): EffectDescriptor["netWobble"] =>
  state.kind === "Goal"
    ? { rear: wobblePanel(state.impact, state.tick, GOAL_LINE_Z - NET_DEPTH), front: wobblePanel(state.impact, state.tick, GOAL_LINE_Z) }
    : null;

const frameShakeOf = (state: ImpactEffectState): EffectDescriptor["frameShake"] => {
  if (state.kind !== "Post") return null;
  const target = state.detail === "HitLeftPost" ? "LeftPost" : state.detail === "HitRightPost" ? "RightPost" : "Crossbar";
  const d = decay(state.tick, 54);
  return { target, offset: vec3(shake(state.tick) * d, shake(state.tick + 1) * d, 0) };
};

const deflectionBias = (detail: ResultDetail): Vec3 =>
  detail === "SavedByLeftHand" ? vec3(0.7, 0.1, 0.4) : detail === "SavedByRightHand" ? vec3(-0.7, 0.1, 0.4) : vec3(0, -0.25, 0.55);

const ballDeflectionOf = (state: ImpactEffectState): EffectDescriptor["ballDeflection"] => {
  const t = Math.min(state.tick, DEFLECT_TICKS) / DEFLECT_TICKS;
  if (state.kind === "Save") return { current: lerp(state.impact, add(state.impact, deflectionBias(state.detail)), t) };
  if (state.kind === "Miss") return { current: lerp(state.impact, add(state.impact, vec3(0, -0.1, -1.1)), t) };
  return null;
};

const crowdAmplitudeOf = (state: ImpactEffectState): number =>
  state.kind === "Goal"
    ? 0.5
    : state.kind === "Save"
      ? 0.3
      : state.kind === "Post"
        ? 0.15
        : state.kind === "Miss"
          ? 0.05
          : state.celebrate
            ? 0.6
            : 0;

const cameraIntensityOf = (kind: EffectKind): number =>
  kind === "Goal" ? 1.2 : kind === "Save" ? 0.9 : kind === "Post" ? 1.0 : kind === "Miss" ? 0.5 : 0.6;

const cameraOffsetOf = (state: ImpactEffectState): Vec3 => {
  if (state.tick >= CAMERA_SHAKE_TICKS) return ZERO;
  const intensity = cameraIntensityOf(state.kind);
  const d = decay(state.tick, CAMERA_SHAKE_TICKS);
  return vec3(shake(state.tick) * intensity * d, shake(state.tick + 2) * intensity * d, 0);
};

const foregroundOf = (state: ImpactEffectState): EffectDescriptor["foreground"] => {
  if (state.kind !== "Save") return [];
  const a = decay(state.tick, POP_TICKS);
  return [{ position: state.impact, size: 0.3 + 0.4 * a, alpha: a }];
};

export const describeEffect = (state: ImpactEffectState): EffectDescriptor => {
  const pop = decay(state.tick, POP_TICKS);
  return {
    kind: state.kind,
    tick: state.tick,
    netWobble: netWobbleOf(state),
    frameShake: frameShakeOf(state),
    ballDeflection: ballDeflectionOf(state),
    crowdAmplitude: crowdAmplitudeOf(state),
    cameraOffset: cameraOffsetOf(state),
    banner: { text: bannerText(state.kind), scale: 1 + 0.5 * pop, pulse: Math.abs(osc(state.tick)) },
    scorePopup: state.kind === "SessionComplete" ? null : { points: state.awardTotal, scale: 1 + 0.4 * pop },
    foreground: foregroundOf(state),
  };
};

/** A crowd card's vertical bounce offset at its stable ordinal (for the scene animator). */
export const crowdCardOffset = (amplitude: number, tick: number, ordinal: number, duration: number): number =>
  amplitude * decay(tick, duration) * bounce(tick + ordinal);

export const effectDuration = durationOf;
