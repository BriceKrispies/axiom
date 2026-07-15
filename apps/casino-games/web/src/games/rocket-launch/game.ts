/*
 * game.ts — Rocket Launch controller. A toy launch pad sits below a ring of
 * destination planets; the planets ARE the destination slots and the winning
 * planet is committed when the countdown fires. The reveal flight is one
 * continuous, deterministic path composed of four sub-phases —
 * liftoff → orbit → dock → reveal — that always ends at the committed planet's
 * docking point via a believable bezier arc plus 1.5 shrinking orbits, with
 * curvature/jitter drawn from the trajectory stream. The whole flight is fast
 * (~3s at speed 1) and shrinks under reduced motion while preserving the phase
 * order. The endpoint, continuity, and ordering are pinned by the test.
 */

import type { EngineVec3, InputFrame, TickContext } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { DestinationSlot } from "../../chance-engine/probability/destination.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { clamp01, easeInCubic, lerp, smoothstep } from "../../presentation/stage/easing.ts";
import { addV3, lerpV3, scaleV3, v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

/** One authored destination planet. `tierId` null marks a non-winning moon. */
export interface PlanetSpec {
  readonly label: string;
  readonly tierId: string | null;
  readonly mass: number;
}

export interface RocketSpec {
  readonly planets: readonly PlanetSpec[];
}

export interface RocketExtra {
  /** Ticks the launch has been charging (0 = idle). */
  readonly chargeTicks: number;
}

export type RocketState = CasinoState<RocketExtra>;

// ── scene geometry (shared by controller + view) ─────────────────────────────

export const PAD = v3(0, 0.6, 0);
const RING_CENTER_Y = 4.3;
const RING_RADIUS_X = 3.1;
const RING_RADIUS_Y = 1.55;
/** The radius of the first (approach) orbit and the final docking orbit. */
const ORBIT_RADIUS = 1.25;
const DOCK_RADIUS = 0.55;
const CHARGE_FULL_TICKS = 72;

export const destinationSlotsOf = (spec: RocketSpec): readonly DestinationSlot[] =>
  spec.planets.map((planet, index) => ({ id: `${index}:${planet.label}`, mass: planet.mass, tierId: planet.tierId }));

/** World position of planet `index` on the upper arc (all planets face camera). */
export const planetPosition = (index: number, count: number): EngineVec3 => {
  const t = count > 1 ? index / (count - 1) : 0.5;
  const angle = Math.PI * (0.16 + 0.68 * t);
  return v3(Math.cos(angle) * RING_RADIUS_X, RING_CENTER_Y + Math.sin(angle) * RING_RADIUS_Y, 0);
};

/** The committed destination planet index (0 before commitment). */
export const committedPlanetIndex = (session: SessionState): number => {
  const plan = session.committed;
  return plan !== null && plan.manifestation.kind === "destination" ? plan.manifestation.destinationIndex : 0;
};

export const chargeStrength = (chargeTicks: number): number => Math.min(1, chargeTicks / CHARGE_FULL_TICKS);

/** How many of the three pad lights have ignited at this charge. */
export const ignitedLights = (chargeTicks: number): number => Math.min(3, Math.floor((chargeTicks / CHARGE_FULL_TICKS) * 3 + 1e-9));

export type FlightPhase = "liftoff" | "orbit" | "dock" | "reveal";

export interface FlightTimeline {
  readonly liftoff: number;
  readonly orbit: number;
  readonly dock: number;
  readonly reveal: number;
  readonly total: number;
}

export const flightTimeline = (presentationSpeed: number, reducedMotion: boolean): FlightTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const liftoff = speedTicks(Math.round(64 * scale), presentationSpeed);
  const orbit = speedTicks(Math.round(60 * scale), presentationSpeed);
  const dock = speedTicks(Math.round(26 * scale), presentationSpeed);
  const reveal = speedTicks(Math.round(30 * scale), presentationSpeed);
  return { dock, liftoff, orbit, reveal, total: liftoff + orbit + dock + reveal };
};

/** Which sub-phase the flight is in at `age`. */
export const flightPhaseAt = (age: number, timeline: FlightTimeline): FlightPhase => {
  if (age < timeline.liftoff) {
    return "liftoff";
  }
  if (age < timeline.liftoff + timeline.orbit) {
    return "orbit";
  }
  if (age < timeline.liftoff + timeline.orbit + timeline.dock) {
    return "dock";
  }
  return "reveal";
};

/** The planet-facing approach point where liftoff ends and orbiting begins. */
const approachPoint = (planet: EngineVec3): EngineVec3 => {
  const dx = PAD.x - planet.x;
  const dy = PAD.y - planet.y;
  const len = Math.hypot(dx, dy) || 1;
  return v3(planet.x + (dx / len) * ORBIT_RADIUS, planet.y + (dy / len) * ORBIT_RADIUS, 0);
};

/** The final docking point on the planet (a small offset toward the camera). */
export const dockPoint = (index: number, count: number): EngineVec3 => {
  const planet = planetPosition(index, count);
  return v3(planet.x, planet.y, planet.z + DOCK_RADIUS);
};

/** A point on the shrinking orbit at param `u` in [0, 1] (1.5 turns). */
const orbitAt = (planet: EngineVec3, approach: EngineVec3, u: number): EngineVec3 => {
  const baseAngle = Math.atan2(approach.y - planet.y, approach.x - planet.x);
  const angle = baseAngle + u * Math.PI * 3; // 1.5 turns
  const radius = lerp(ORBIT_RADIUS, DOCK_RADIUS, u);
  const wobbleZ = Math.sin(u * Math.PI * 3) * 0.25 * (1 - u);
  return v3(planet.x + Math.cos(angle) * radius, planet.y + Math.sin(angle) * radius, planet.z + wobbleZ);
};

/**
 * The rocket's world position at reveal age — one continuous path across the
 * four sub-phases. Liftoff is a quadratic bezier from the pad, up past an apex,
 * to the approach point (control-point curvature jittered from the trajectory
 * stream). Orbit spirals 1.5 turns inward. Dock eases onto the planet.
 */
export const rocketPosition = (
  age: number,
  index: number,
  count: number,
  seed: number,
  round: number,
  timeline: FlightTimeline,
): EngineVec3 => {
  const planet = planetPosition(index, count);
  const approach = approachPoint(planet);
  const phase = flightPhaseAt(age, timeline);

  if (phase === "liftoff") {
    const t = easeInCubic(clamp01(age / timeline.liftoff));
    const apexY = PAD.y + 1.9;
    const curveX = (sample01(seed, "trajectory", round, 0) - 0.5) * 1.4;
    const control = v3((PAD.x + approach.x) / 2 + curveX, apexY, 0);
    const a = lerpV3(PAD, control, t);
    const b = lerpV3(control, approach, t);
    return lerpV3(a, b, t);
  }
  if (phase === "orbit") {
    const u = clamp01((age - timeline.liftoff) / timeline.orbit);
    return orbitAt(planet, approach, u);
  }
  if (phase === "dock") {
    const u = smoothstep(clamp01((age - timeline.liftoff - timeline.orbit) / timeline.dock));
    return lerpV3(orbitAt(planet, approach, 1), dockPoint(index, count), u);
  }
  return dockPoint(index, count);
};

/** The rocket's facing heading (radians in the screen plane) from its velocity —
 * a finite-difference so the nose points along the path. */
export const rocketHeading = (
  age: number,
  index: number,
  count: number,
  seed: number,
  round: number,
  timeline: FlightTimeline,
): number => {
  const a = rocketPosition(Math.max(0, age - 1), index, count, seed, round, timeline);
  const b = rocketPosition(age, index, count, seed, round, timeline);
  const dy = b.y - a.y;
  const dx = b.x - a.x;
  return Math.abs(dx) + Math.abs(dy) < 1e-5 ? Math.PI / 2 : Math.atan2(dy, dx);
};

export const initialRocketExtra = (_session: SessionState): RocketExtra => ({ chargeTicks: 0 });

export const stepRocket = (
  runtime: GameRuntime<RocketSpec>,
  state: RocketState,
  input: InputFrame,
  _ctx: TickContext,
): RocketState => {
  const session = state.session;

  if (session.phase === "ready") {
    const holding = input.down.has("primary") || (input.pointer?.down ?? false);
    const charge = state.extra.chargeTicks;
    const full = charge >= CHARGE_FULL_TICKS;
    if (holding && !full) {
      return { ...state, extra: { chargeTicks: charge + 1 } };
    }
    if (full || (!holding && charge > 0)) {
      return {
        ...state,
        pendingContext: { launchStrength: chargeStrength(charge) },
        session: transition(session, "committing"),
      };
    }
    return state;
  }

  if (session.phase === "revealing") {
    const timeline = flightTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
    return state;
  }

  return state;
};

/** Bounded exhaust origin just behind the rocket, for the flame/smoke stream. */
export const exhaustOrigin = (position: EngineVec3, heading: number): EngineVec3 =>
  addV3(position, scaleV3(v3(Math.cos(heading), Math.sin(heading), 0), -0.42));
