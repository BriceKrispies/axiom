/*
 * The deterministic fixed-step simulation. `stepSim(state, intent)` advances the
 * whole game by exactly one tick (`DT` seconds) as a pure function of the current
 * state and the tick's `Intent` — no wall clock, no RNG, no unordered iteration.
 * Two runs with the same initial `State` and the same intent sequence produce
 * byte-identical states, which the replay test pins.
 *
 * This is app-tier game logic (courier, shards, storm): it uses ordinary control
 * flow and lives nowhere near an engine layer.
 */

import {
  ACTIVATE_Z,
  BASE_SPEED,
  BOOST_CD_SECONDS,
  BOOST_COST,
  BOOST_SECONDS,
  BOOST_SPEED,
  BRAKE_FACTOR,
  BRAKE_STEER_BONUS,
  CRASH_KNOCK,
  CRASH_SPEED_FACTOR,
  CURVE_DRIFT,
  DRONE_CD_SECONDS,
  DRONE_COST,
  DT,
  FALL_MARGIN,
  FIXED_HZ,
  HELPER_SECONDS,
  HELPER_SPEED,
  INVULN_SECONDS,
  LAT_DAMPING,
  MAX_CRASHES,
  MAX_LAT_VEL,
  MAX_SPEED,
  OBSTACLE_HIT,
  OFFPATH_FACTOR,
  PLATE_GOAL,
  PLATE_LATERAL,
  PULSE_CD_SECONDS,
  PULSE_COST,
  PULSE_RADIUS_X,
  PULSE_RADIUS_Z,
  SHARD_CHARGE,
  SHARD_GOAL,
  SHARD_LATERAL,
  SHIELD_CD_SECONDS,
  SHIELD_COST,
  SHIELD_SECONDS,
  SPEED_EASE,
  STEER_ACCEL,
  STORM_BASE_SPEED,
  STORM_MAX_SPEED,
  STORM_SECONDS,
} from "./constants.ts";
import { pathWidthAt } from "./level.ts";
import type { AbilityKind, DroneHazard, Intent, LoseReason, Shard, State } from "./types.ts";

const clamp = (v: number, lo: number, hi: number): number => Math.max(lo, Math.min(hi, v));
const secs = (n: number): number => Math.round(n * FIXED_HZ);

/** The live lateral center a drone bobs to at `tick` (deterministic). */
export const droneLateralAt = (drone: DroneHazard, tick: number): number =>
  drone.baseLateral + Math.sin(tick * 0.05 + drone.z * 0.01) * drone.sway;

/** End the run as a loss with `reason` (first loss wins; idempotent). */
const lose = (state: State, reason: LoseReason): void => {
  if (state.phase === "run") {
    state.phase = "lose";
    state.loseReason = reason;
  }
};

/** Resolve a hazard contact: a live shield absorbs it, else it is a crash. */
const crash = (state: State, awayDir: number): void => {
  const r = state.runner;
  if (r.invulnTicks > 0) {
    return;
  }
  if (r.shieldTicks > 0) {
    r.shieldTicks = 0;
    r.invulnTicks = secs(INVULN_SECONDS * 0.6);
    return;
  }
  r.crashes += 1;
  r.speed *= CRASH_SPEED_FACTOR;
  r.latVel += awayDir * CRASH_KNOCK;
  r.invulnTicks = secs(INVULN_SECONDS);
  if (r.crashes >= MAX_CRASHES) {
    lose(state, "crashed");
  }
};

/** Try to spend `cost` charge to fire an ability; returns whether it fired. */
const spend = (state: State, cost: number): boolean => {
  if (state.runner.charge + 1e-6 < cost) {
    return false;
  }
  state.runner.charge = clamp(state.runner.charge - cost, 0, 1);
  return true;
};

/** Fire the boost: a short forward burst. */
const fireBoost = (state: State): void => {
  if (state.ability.boostCd === 0 && spend(state, BOOST_COST)) {
    state.runner.boostTicks = secs(BOOST_SECONDS);
    state.ability.boostCd = secs(BOOST_CD_SECONDS);
  }
};

/** Fire the shield: absorbs the next crash for a few seconds. */
const fireShield = (state: State): void => {
  if (state.ability.shieldCd === 0 && spend(state, SHIELD_COST)) {
    state.runner.shieldTicks = secs(SHIELD_SECONDS);
    state.ability.shieldCd = secs(SHIELD_CD_SECONDS);
  }
};

/** Fire the pulse: disable every drone within the pulse radius of the runner. */
const firePulse = (state: State): void => {
  if (state.ability.pulseCd === 0 && spend(state, PULSE_COST)) {
    state.ability.pulseCd = secs(PULSE_CD_SECONDS);
    const r = state.runner;
    for (const d of state.level.drones) {
      const near = Math.abs(d.z - r.dist) < PULSE_RADIUS_Z && Math.abs(droneLateralAt(d, state.tick) - r.lateral) < PULSE_RADIUS_X;
      d.disabled = d.disabled || near;
    }
  }
};

/** Fire the helper drone: it flies ahead collecting shards for a few seconds. */
const fireDrone = (state: State): void => {
  if (state.ability.droneCd === 0 && !state.ability.helper.active && spend(state, DRONE_COST)) {
    state.ability.droneCd = secs(DRONE_CD_SECONDS);
    state.ability.helper = { active: true, lateral: state.runner.lateral, ticks: secs(HELPER_SECONDS), z: state.runner.dist };
  }
};

const ABILITY_FIRE: Record<AbilityKind, (state: State) => void> = {
  boost: fireBoost,
  drone: fireDrone,
  pulse: firePulse,
  shield: fireShield,
};

/** Dispatch this tick's ability edges. */
const handleAbilities = (state: State, intent: Intent): void => {
  const edges: [boolean, AbilityKind][] = [
    [intent.boost, "boost"],
    [intent.shield, "shield"],
    [intent.pulse, "pulse"],
    [intent.drone, "drone"],
  ];
  for (const [fired, kind] of edges) {
    if (fired) {
      ABILITY_FIRE[kind](state);
    }
  }
};

/** The nearest uncollected shard ahead of `z`, or null. */
const nearestShardAhead = (state: State, z: number): Shard | null => {
  let best: Shard | null = null;
  for (const s of state.level.shards) {
    if (!s.collected && s.z >= z && (best === null || s.z < best.z)) {
      best = s;
    }
  }
  return best;
};

/** Collect a shard: bank it and add charge. */
const bankShard = (state: State, shard: Shard): void => {
  shard.collected = true;
  state.shardsCollected += 1;
  state.runner.charge = clamp(state.runner.charge + SHARD_CHARGE, 0, 1);
};

/** Advance the helper drone: fly ahead, home on the nearest shard, collect it. */
const stepHelper = (state: State): void => {
  const h = state.ability.helper;
  if (!h.active) {
    return;
  }
  h.ticks -= 1;
  const target = nearestShardAhead(state, h.z);
  h.z += HELPER_SPEED * DT;
  h.lateral += clamp((target === null ? 0 : target.lateral) - h.lateral, -30, 30);
  if (target !== null && Math.abs(target.z - h.z) < 80 && Math.abs(target.lateral - h.lateral) < SHARD_LATERAL) {
    bankShard(state, target);
  }
  if (h.ticks <= 0) {
    h.active = false;
  }
};

/** Resolve the runner's forward speed for this tick (ramp + boost + brake + off-path). */
const stepSpeed = (state: State, intent: Intent, offPath: boolean): void => {
  const r = state.runner;
  const ramp = clamp(r.dist / state.level.beaconZ, 0, 1);
  let target = BASE_SPEED + (MAX_SPEED - BASE_SPEED) * ramp;
  target += r.boostTicks > 0 ? BOOST_SPEED : 0;
  target *= intent.brake ? BRAKE_FACTOR : 1;
  target *= offPath ? OFFPATH_FACTOR : 1;
  r.speed += (target - r.speed) * clamp(SPEED_EASE * DT, 0, 1);
};

/** Resolve the runner's steering + lateral position for this tick. */
const stepSteer = (state: State, intent: Intent, width: number, curve: number): void => {
  const r = state.runner;
  const steer = intent.steerTo === null ? intent.steer : clamp((intent.steerTo * width - r.lateral) / width, -1, 1);
  const accel = STEER_ACCEL * (intent.brake ? BRAKE_STEER_BONUS : 1);
  r.latVel += steer * accel * DT;
  r.latVel -= curve * r.speed * CURVE_DRIFT * DT;
  r.latVel *= clamp(1 - LAT_DAMPING * DT, 0, 1);
  r.latVel = clamp(r.latVel, -MAX_LAT_VEL, MAX_LAT_VEL);
  r.lateral += r.latVel * DT;
  r.lean += (clamp(r.latVel / MAX_LAT_VEL, -1, 1) - r.lean) * 0.15;
};

/** Collect any shards / activate any plates the runner crossed this tick. */
const stepPickups = (state: State, prevDist: number): void => {
  const r = state.runner;
  for (const s of state.level.shards) {
    if (!s.collected && s.z > prevDist && s.z <= r.dist && Math.abs(r.lateral - s.lateral) < SHARD_LATERAL) {
      bankShard(state, s);
    }
  }
  for (const p of state.level.plates) {
    if (!p.activated && p.z > prevDist && p.z <= r.dist && Math.abs(r.lateral - p.lateral) < PLATE_LATERAL) {
      p.activated = true;
      state.platesActivated += 1;
    }
  }
};

/** Test every hazard the runner crossed this tick for a collision. */
const stepHazards = (state: State, prevDist: number): void => {
  const r = state.runner;
  for (const o of state.level.obstacles) {
    if (o.z > prevDist && o.z <= r.dist && Math.abs(r.lateral - o.lateral) < OBSTACLE_HIT + o.radius) {
      crash(state, Math.sign(r.lateral - o.lateral) || 1);
    }
  }
  for (const d of state.level.drones) {
    const dl = droneLateralAt(d, state.tick);
    if (!d.disabled && d.z > prevDist && d.z <= r.dist && Math.abs(r.lateral - dl) < OBSTACLE_HIT + 30) {
      crash(state, Math.sign(r.lateral - dl) || 1);
    }
  }
};

/** Advance the storm front and resolve time/storm loss conditions. */
const stepStorm = (state: State): void => {
  const t = clamp(state.elapsed / STORM_SECONDS, 0, 1);
  const speed = STORM_BASE_SPEED + (STORM_MAX_SPEED - STORM_BASE_SPEED) * t;
  state.storm.dist += speed * DT;
  const gap = state.runner.dist - state.storm.dist;
  state.storm.intensity = clamp(1 - gap / 1500, 0, 1);
  if (state.timeLeft <= 0) {
    lose(state, "time");
  }
  if (state.storm.dist >= state.runner.dist) {
    lose(state, "storm");
  }
};

/** Decrement all countdown timers by one tick. */
const tickTimers = (state: State): void => {
  const r = state.runner;
  const a = state.ability;
  r.boostTicks = Math.max(0, r.boostTicks - 1);
  r.shieldTicks = Math.max(0, r.shieldTicks - 1);
  r.invulnTicks = Math.max(0, r.invulnTicks - 1);
  a.boostCd = Math.max(0, a.boostCd - 1);
  a.shieldCd = Math.max(0, a.shieldCd - 1);
  a.pulseCd = Math.max(0, a.pulseCd - 1);
  a.droneCd = Math.max(0, a.droneCd - 1);
};

/** Advance the whole game by one fixed tick. */
export const stepSim = (state: State, intent: Intent): void => {
  if (state.phase !== "run") {
    return;
  }
  state.tick += 1;
  state.elapsed += DT;
  state.timeLeft = Math.max(0, state.timeLeft - DT);

  const r = state.runner;
  const width = pathWidthAt(state.level.nodes, r.dist);
  const nodeIndex = Math.max(0, Math.min(state.level.nodes.length - 1, Math.round(r.dist / state.level.segLen)));
  const curve = state.level.nodes[nodeIndex]?.curve ?? 0;

  handleAbilities(state, intent);

  const offPath = Math.abs(r.lateral) > width;
  stepSpeed(state, intent, offPath);
  stepSteer(state, intent, width, curve);

  const prevDist = r.dist;
  r.dist = Math.min(state.level.beaconZ, r.dist + r.speed * DT);

  stepPickups(state, prevDist);
  stepHazards(state, prevDist);
  stepHelper(state);

  // Falling off the edge ends the run.
  if (Math.abs(r.lateral) > width + FALL_MARGIN) {
    lose(state, "fell");
  }

  stepStorm(state);
  tickTimers(state);

  // Beacon activation: offered only once objectives are complete and in range.
  state.beaconReady =
    r.dist >= state.level.beaconZ - ACTIVATE_Z &&
    state.shardsCollected >= SHARD_GOAL &&
    state.platesActivated >= PLATE_GOAL;
  if (state.beaconReady && intent.confirm) {
    state.beaconRestored = true;
    state.phase = "win";
  }
};
