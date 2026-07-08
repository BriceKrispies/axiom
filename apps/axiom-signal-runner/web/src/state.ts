/*
 * The initial-state factory. `initialState(level)` is the single source of truth for
 * where a run begins; the game's constructor and its restart both call it, so a
 * restart is provably identical to a fresh boot (the tests pin this).
 */

import { BASE_SPEED, STORM_SECONDS, STORM_START_BACK } from "./constants.ts";
import type { AbilityState, Level, Runner, State, Storm } from "./types.ts";

/** A fresh runner at the start line: centered, cruising, no charge. */
const freshRunner = (): Runner => ({
  boostTicks: 0,
  charge: 0,
  crashes: 0,
  dist: 0,
  invulnTicks: 0,
  latVel: 0,
  lateral: 0,
  lean: 0,
  shieldTicks: 0,
  speed: BASE_SPEED,
});

/** All abilities ready, no helper deployed. */
const freshAbility = (): AbilityState => ({
  boostCd: 0,
  droneCd: 0,
  helper: { active: false, lateral: 0, ticks: 0, z: 0 },
  pulseCd: 0,
  shieldCd: 0,
});

/** The storm front parked its start-distance behind the runner. */
const freshStorm = (): Storm => ({ dist: -STORM_START_BACK, intensity: 0 });

/** Build the run's opening state over a generated `level`. */
export const initialState = (level: Level): State => ({
  ability: freshAbility(),
  beaconReady: false,
  beaconRestored: false,
  elapsed: 0,
  level,
  loseReason: null,
  phase: "run",
  platesActivated: 0,
  runner: freshRunner(),
  shardsCollected: 0,
  storm: freshStorm(),
  tick: 0,
  timeLeft: STORM_SECONDS,
});
