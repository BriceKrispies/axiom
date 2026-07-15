/*
 * game.ts — Prize Elevator controller. A vertical prize tower: the reward floors
 * ARE the destination slots, and the winning floor is committed when the launch
 * button is pressed. The reveal is an analytic ascent: the car cruises upward at
 * a constant rate, then decelerates on a smoothstep profile computed to stop
 * EXACTLY at the committed floor's height — continuous, no snap. Floor indicator
 * lamps light in order as the car passes each floor below the destination, the
 * car settles with a tiny bounce, and the doors slide open onto the floor's
 * vignette. The stop-height and monotone-rise properties are pinned by the test.
 */

import type { InputFrame, TickContext } from "@axiom/web-engine";
import type { DestinationSlot } from "../../chance-engine/probability/destination.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { clamp01, lerp, smoothstep } from "../../presentation/stage/easing.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

/** One authored floor of the tower. `tierId` null marks a non-winning floor
 * (a workshop/lobby). Floors are listed bottom→top in ascending desirability. */
export interface FloorSpec {
  readonly label: string;
  readonly tierId: string | null;
  readonly mass: number;
}

export interface ElevatorSpec {
  readonly floors: readonly FloorSpec[];
}

export interface ElevatorExtra {
  /** Idle button pulse phase kept for cue edges (unused across commit). */
  readonly armed: boolean;
}

export type ElevatorState = CasinoState<ElevatorExtra>;

/** World height of floor `index` (ground floor at FLOOR_BASE, evenly stacked). */
export const FLOOR_BASE = 0.7;
export const FLOOR_SPACING = 1.55;

export const floorHeight = (index: number): number => FLOOR_BASE + index * FLOOR_SPACING;

export const destinationSlotsOf = (spec: ElevatorSpec): readonly DestinationSlot[] =>
  spec.floors.map((floor, index) => ({ id: `${index}:${floor.label}`, mass: floor.mass, tierId: floor.tierId }));

/** The committed destination floor index (0 before commitment). */
export const committedFloorIndex = (session: SessionState): number => {
  const plan = session.committed;
  return plan !== null && plan.manifestation.kind === "destination" ? plan.manifestation.destinationIndex : 0;
};

export interface RideTimeline {
  readonly cruise: number;
  readonly decel: number;
  readonly settle: number;
  readonly doors: number;
  readonly total: number;
}

/**
 * The ride timeline. The car cruises for a distance-proportional span, then
 * decelerates over a fixed span, then settles and opens its doors. Cruise ticks
 * scale with the number of floors travelled so a tall trip reads as a long ride.
 */
export const rideTimeline = (targetIndex: number, presentationSpeed: number, reducedMotion: boolean): RideTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const cruise = speedTicks(Math.round((36 + targetIndex * 16) * scale), presentationSpeed);
  const decel = speedTicks(Math.round(40 * scale), presentationSpeed);
  const settle = speedTicks(Math.round(16 * scale), presentationSpeed);
  const doors = speedTicks(Math.round(26 * scale), presentationSpeed);
  return { cruise, decel, doors, settle, total: cruise + decel + settle + doors };
};

/**
 * The car's world height at reveal age. During cruise the car climbs linearly
 * toward the deceleration-onset height; during decel it eases (smoothstep) to
 * the committed floor exactly; afterward it holds (with a settle bounce added by
 * the view, not here). Monotone non-decreasing through cruise+decel.
 */
export const carHeight = (age: number, targetIndex: number, timeline: RideTimeline): number => {
  const target = floorHeight(targetIndex);
  const start = FLOOR_BASE;
  // Deceleration covers the final DECEL_FLOORS_FRACTION of the climb.
  const decelStartHeight = lerp(start, target, 0.72);
  if (age <= timeline.cruise) {
    return lerp(start, decelStartHeight, clamp01(age / Math.max(1, timeline.cruise)));
  }
  const decelAge = age - timeline.cruise;
  if (decelAge <= timeline.decel) {
    return lerp(decelStartHeight, target, smoothstep(clamp01(decelAge / timeline.decel)));
  }
  return target;
};

/** The final resting height of the car at reveal end — exactly the floor height. */
export const carStopHeight = (targetIndex: number): number => floorHeight(targetIndex);

/** Whether floor `floor` has been lit by the passing car at this age (every
 * floor strictly below the destination lights exactly once, in order). */
export const floorLit = (floor: number, age: number, targetIndex: number, timeline: RideTimeline): boolean => {
  if (floor >= targetIndex) {
    return floor === targetIndex && age >= timeline.cruise + timeline.decel;
  }
  return carHeight(age, targetIndex, timeline) >= floorHeight(floor) - 0.02;
};

/** Door-open fraction in [0, 1] (0 shut, 1 fully parted). */
export const doorOpen = (age: number, timeline: RideTimeline): number => {
  const openAge = age - (timeline.cruise + timeline.decel + timeline.settle);
  return smoothstep(clamp01(openAge / timeline.doors));
};

export const initialElevatorExtra = (_session: SessionState): ElevatorExtra => ({ armed: false });

export const stepElevator = (
  runtime: GameRuntime<ElevatorSpec>,
  state: ElevatorState,
  input: InputFrame,
  _ctx: TickContext,
): ElevatorState => {
  const session = state.session;

  if (session.phase === "ready") {
    const press = input.pressed.has("primary") || (input.pointer?.down ?? false);
    if (press) {
      return { ...state, extra: { armed: true }, pendingContext: {}, session: transition(session, "committing") };
    }
    return state;
  }

  if (session.phase === "revealing") {
    const timeline = rideTimeline(committedFloorIndex(session), session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
    return state;
  }

  return state;
};

/** The set of floor indices newly lit between two reveal ages (for step cues). */
export const floorsCrossed = (
  prev: ElevatorState,
  next: ElevatorState,
): readonly number[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const targetIndex = committedFloorIndex(session);
  const timeline = rideTimeline(targetIndex, session.config.presentationSpeed, false);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  return Array.from({ length: targetIndex + 1 }, (_unused, i) => i).filter(
    (i) => !floorLit(i, before, targetIndex, timeline) && floorLit(i, after, targetIndex, timeline),
  );
};
