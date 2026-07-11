/*
 * polish.ts — the PRESENTATION reaction state, entirely SDK-free and
 * deterministic (fixed-tick timers only; no wall clock, no randomness), so it
 * is fully testable under `node --test`. The session feeds it the unified
 * `GameEvent` stream and advances it once per tick; the scene/HUD read plain
 * numbers out of it. Nothing in here may influence gameplay: no launch, no
 * collider, no score, no aim — every output is a cosmetic offset, pulse, or
 * label, and `reset()` returns everything to the exact initial presentation.
 *
 * Building blocks: `Impulse` (a fixed-duration linear-decay envelope with an
 * optional sine oscillation — two to three diminishing wobbles, guaranteed to
 * reach exactly zero) and a handful of pure mapping functions (impact speed →
 * bounded amplitude/volume/pitch, streak → presentation level, score
 * count-up) shared with the audio layer and the HUD.
 */

import { type Vec3, clamp, mix, vec3 } from "./vec.ts";
import { BALLS_PER_RACK, POLISH_TUNING as P } from "./constants.ts";
import type { GameEvent, NetView } from "./types.ts";

// ── pure mappings (shared with audio + HUD; directly unit-tested) ─────────────

/** Normalized impact strength: 0 at rest, 1 at `impactSpeedFull`, clamped. */
export const impactNorm = (speed: number): number => clamp(speed / P.impactSpeedFull, 0, 1);

/** Impact audio volume, clamped to the tuning band. */
export const impactVolume = (speed: number): number => mix(P.minImpactVolume, P.maxImpactVolume, impactNorm(speed));

/** Impact audio pitch multiplier, clamped to the tuning band. */
export const impactPitch = (speed: number): number => mix(P.minImpactPitch, P.maxImpactPitch, impactNorm(speed));

/** Streak presentation level: 0 (streak 0–1), 1 (=2), 2 (=3), 3 (≥4). */
export const streakPresentationLevel = (streak: number): number => {
  if (streak >= P.streakGlowLevel) return 3;
  if (streak >= P.streakLevelAccent) return 2;
  if (streak >= P.streakLevelStrong) return 1;
  return 0;
};

/** One step of the HUD score count-up: fast, monotonic, and guaranteed to land
 * exactly on `target`. */
export const countTowards = (current: number, target: number): number => {
  if (current === target) return target;
  const step = Math.max(1, Math.ceil(Math.abs(target - current) * 0.25));
  return current < target ? Math.min(target, current + step) : Math.max(target, current - step);
};

/** The golden ball's intermittent glint window (deterministic from the tick). */
export const glintOn = (tick: number): boolean => tick % 90 < 8;

// ── the damped impulse (envelope · optional oscillation) ──────────────────────

/** A fixed-duration reaction: linear-decay envelope from `strength` to exactly
 * zero, with `cycles` sine oscillations across the duration. */
export class Impulse {
  #ticksLeft = 0;
  #duration = 1;
  #strength = 0;

  fire(strength: number, durationTicks: number): void {
    this.#strength = strength;
    this.#duration = Math.max(1, durationTicks);
    this.#ticksLeft = this.#duration;
  }

  advance(): void {
    if (this.#ticksLeft > 0) this.#ticksLeft -= 1;
  }

  get active(): boolean {
    return this.#ticksLeft > 0;
  }

  /** Elapsed fraction 0..1 (1 once finished). */
  progress(): number {
    return 1 - this.#ticksLeft / this.#duration;
  }

  /** The decaying envelope: `strength` at fire, exactly 0 at the end. */
  envelope(): number {
    return this.#ticksLeft <= 0 ? 0 : this.#strength * (this.#ticksLeft / this.#duration);
  }

  /** Envelope-scaled sine wobble with `cycles` full oscillations (exactly 0,
   * never −0, once spent). */
  oscillation(cycles: number): number {
    const env = this.envelope();
    return env === 0 ? 0 : env * Math.sin(this.progress() * cycles * Math.PI * 2);
  }

  reset(): void {
    this.#ticksLeft = 0;
    this.#strength = 0;
  }
}

// ── the reaction state ────────────────────────────────────────────────────────

interface NetReaction {
  mode: "swish" | "make" | null;
  t: number;
  lateralX: number;
  lateralZ: number;
}

export class PolishState {
  readonly #rim = new Impulse();
  readonly #board = new Impulse();
  readonly #kick = new Impulse();
  readonly #rackDip = new Impulse();
  readonly #slotSettle: Impulse[] = Array.from({ length: BALLS_PER_RACK }, () => new Impulse());
  #net: NetReaction = { lateralX: 0, lateralZ: 0, mode: null, t: 0 };
  #squash = new Map<number, number>();
  #crowd = 0;
  #glow = 0;
  #glowTarget = 0;
  #streakPulseSeq = 0;
  #streakBrokenSeq = 0;
  #award: { points: number; seq: number } | null = null;
  #awardSeq = 0;
  #stationLabel: string | null = null;
  #stationLabelTicks = 0;

  /** Feed one gameplay event (the session calls this as events are emitted). */
  onEvent(event: GameEvent): void {
    switch (event.kind) {
      case "ballPickupStarted": {
        this.#rackDip.fire(0.012, P.rackSettleTicks);
        // Neighbours settle; the slot index gives each pickup a tiny signature.
        const amp = 0.008 * (1 + event.slot * 0.07);
        const left = this.#slotSettle[event.slot - 1];
        const right = this.#slotSettle[event.slot + 1];
        left?.fire(amp, P.rackSettleTicks);
        right?.fire(amp, P.rackSettleTicks);
        break;
      }
      case "ballReleased":
        this.#kick.fire(1, P.releaseKickTicks + P.followThroughTicks);
        break;
      case "rimHit":
        this.#rim.fire(P.rimVibrationStrength * impactNorm(event.speed), P.rimVibrationTicks);
        break;
      case "backboardHit":
        this.#board.fire(P.backboardShakeStrength * impactNorm(event.speed), P.backboardShakeTicks);
        break;
      case "floorHit":
        if (impactNorm(event.speed) > 0.3 && this.#squash.size < 8) {
          this.#squash.set(event.seq, P.ballSquashTicks);
        }
        break;
      case "basketMade": {
        this.#net = event.swish
          ? { lateralX: 0, lateralZ: 0, mode: "swish", t: 0 }
          : {
              lateralX: clamp(event.entryX * 0.8, -1, 1) * P.netDisplacementStrength,
              lateralZ: clamp(event.entryZ * 0.8, -1, 1) * P.netDisplacementStrength,
              mode: "make",
              t: 0,
            };
        this.#crowd = Math.min(
          1,
          this.#crowd + 0.4 + 0.1 * streakPresentationLevel(event.streak) + (event.swish ? 0.15 : 0),
        );
        this.#awardSeq += 1;
        this.#award = { points: event.points, seq: this.#awardSeq };
        break;
      }
      case "streakIncreased":
        this.#streakPulseSeq += 1;
        this.#glowTarget = streakPresentationLevel(event.streak) >= 3 ? 1 : 0;
        break;
      case "streakBroken":
        this.#streakBrokenSeq += 1;
        this.#glowTarget = 0;
        break;
      case "rackCompleted":
        this.#rackDip.fire(0.02, P.rackSettleTicks);
        break;
      case "stationTransitionCompleted":
        this.#stationLabel = event.label;
        this.#stationLabelTicks = P.stationLabelTicks;
        this.#rackDip.fire(0.015, P.rackSettleTicks);
        break;
      case "gameRestarted":
        this.reset();
        break;
      default:
        break;
    }
  }

  /** Advance every timer one fixed tick. */
  advance(): void {
    this.#rim.advance();
    this.#board.advance();
    this.#kick.advance();
    this.#rackDip.advance();
    for (const s of this.#slotSettle) s.advance();
    if (this.#net.mode !== null) {
      this.#net.t += 1;
      if (this.#net.t >= P.netSnapTicks + P.netSwayTicks) {
        this.#net = { lateralX: 0, lateralZ: 0, mode: null, t: 0 };
      }
    }
    for (const [seq, ticks] of this.#squash) {
      if (ticks <= 1) this.#squash.delete(seq);
      else this.#squash.set(seq, ticks - 1);
    }
    this.#crowd = Math.max(0, this.#crowd - 1 / P.crowdReactionTicks);
    this.#glow += (this.#glowTarget - this.#glow) * 0.08;
    if (Math.abs(this.#glow - this.#glowTarget) < 0.002) this.#glow = this.#glowTarget;
    if (this.#stationLabelTicks > 0) {
      this.#stationLabelTicks -= 1;
      if (this.#stationLabelTicks === 0) this.#stationLabel = null;
    }
  }

  /** Everything back to the exact initial presentation (restart). */
  reset(): void {
    this.#rim.reset();
    this.#board.reset();
    this.#kick.reset();
    this.#rackDip.reset();
    for (const s of this.#slotSettle) s.reset();
    this.#net = { lateralX: 0, lateralZ: 0, mode: null, t: 0 };
    this.#squash.clear();
    this.#crowd = 0;
    this.#glow = 0;
    this.#glowTarget = 0;
    this.#award = null;
    this.#awardSeq = 0;
    this.#streakPulseSeq = 0;
    this.#streakBrokenSeq = 0;
    this.#stationLabel = null;
    this.#stationLabelTicks = 0;
  }

  // ── reads (all pure of side effects) ────────────────────────────────────────

  /** Visible-rim displacement: 2–3 diminishing wobbles, exactly zero at rest. */
  rimOffset(): Vec3 {
    const wobble = this.#rim.oscillation(2.5);
    return vec3(wobble, Math.abs(wobble) * -0.5 + 0, 0);
  }

  /** Visible-backboard displacement (z-dominant nod toward the court). */
  boardOffset(): Vec3 {
    const wobble = this.#board.oscillation(2);
    return vec3(wobble * 0.4, 0, wobble);
  }

  /** The net reaction pose. A swish is a sharper, more vertical snap; a rimmed
   * make is softer with more flare and the lateral entry displacement. */
  net(): NetView {
    const { mode, t, lateralX, lateralZ } = this.#net;
    if (mode === null) return { drop: 0, flare: 0, lateralX: 0, lateralZ: 0 };
    const snapT = clamp(t / P.netSnapTicks, 0, 1);
    const swayT = clamp((t - P.netSnapTicks) / P.netSwayTicks, 0, 1);
    // Fast ease-out attack, then a damped wobble that ends at exactly zero.
    const attack = 1 - (1 - snapT) * (1 - snapT);
    const decay = t < P.netSnapTicks ? 1 : (1 - swayT) * (1 + 0.35 * Math.sin(swayT * 3 * Math.PI * 2));
    const level = attack * Math.max(0, decay);
    const sharp = mode === "swish" ? 1.25 : 0.8;
    return {
      drop: level * sharp,
      flare: level * (mode === "swish" ? 0.35 : 0.9),
      lateralX: mode === "swish" ? 0 : lateralX * level,
      lateralZ: mode === "swish" ? 0 : lateralZ * level,
    };
  }

  /** Vertical squash multiplier for a live ball (1 = spherical). */
  squash(seq: number): number {
    const ticks = this.#squash.get(seq);
    if (ticks === undefined) return 1;
    return mix(1, P.ballSquashAmount, ticks / P.ballSquashTicks);
  }

  /** Eye-position recoil distance (m) at this instant of the release kick —
   * fast out over the kick, settling to zero through the follow-through.
   * POSITION only; the camera's orientation is never touched. */
  kickRecoil(): number {
    if (!this.#kick.active) return 0;
    const t = this.#kick.progress() * (P.releaseKickTicks + P.followThroughTicks);
    const out = clamp(t / P.releaseKickTicks, 0, 1);
    const back = clamp((t - P.releaseKickTicks) / P.followThroughTicks, 0, 1);
    return P.releaseKickStrength * (1 - (1 - out) * (1 - out)) * (1 - back);
  }

  rackDip(): number {
    return this.#rackDip.envelope();
  }

  slotSettle(): readonly number[] {
    return this.#slotSettle.map((s) => s.oscillation(2));
  }

  crowdPulse(): number {
    return this.#crowd;
  }

  glow(): number {
    return this.#glow;
  }

  streakPulseSeq(): number {
    return this.#streakPulseSeq;
  }

  streakBrokenSeq(): number {
    return this.#streakBrokenSeq;
  }

  award(): { readonly points: number; readonly seq: number } | null {
    return this.#award;
  }

  stationLabel(): string | null {
    return this.#stationLabel;
  }

  /** Development counter: reactions currently animating. */
  activeEffects(): number {
    let n = this.#squash.size;
    if (this.#rim.active) n += 1;
    if (this.#board.active) n += 1;
    if (this.#kick.active) n += 1;
    if (this.#rackDip.active) n += 1;
    if (this.#net.mode !== null) n += 1;
    for (const s of this.#slotSettle) if (s.active) n += 1;
    return n;
  }
}
