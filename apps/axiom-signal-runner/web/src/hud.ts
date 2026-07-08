/*
 * The HUD/UI model — a pure projection of `State` into exactly the values the panels
 * draw (objective counts, timer text, speed, charge segments, ability card states,
 * minimap nodes). Keeping it pure means the renderer holds no game logic and the UI
 * model is unit-testable: the test asserts the model tracks the live state.
 */

import {
  BOOST_COST,
  CHARGE_SEGMENTS,
  DRONE_COST,
  KMH_FACTOR,
  PLATE_GOAL,
  PULSE_COST,
  SHARD_GOAL,
  SHIELD_COST,
} from "./constants.ts";
import type { AbilityKind, State } from "./types.ts";

/** One ability card's display state. */
export interface AbilityCard {
  readonly kind: AbilityKind;
  readonly label: string;
  /** Chargeable and off cooldown. */
  readonly ready: boolean;
  /** Currently in effect (boost burst / shield up / helper flying). */
  readonly active: boolean;
  /** Remaining cooldown as a fraction in [0, 1] (0 = ready). */
  readonly cooldown: number;
}

/** One node on the minimap route (a shard, a plate, or the beacon). */
export interface MiniNode {
  /** Progress along the route in [0, 1]. */
  readonly t: number;
  readonly kind: "shard" | "plate" | "beacon";
  readonly done: boolean;
}

/** The complete UI model for one frame. */
export interface Hud {
  readonly objectiveTitle: string;
  readonly beaconReady: boolean;
  readonly shards: number;
  readonly shardGoal: number;
  readonly plates: number;
  readonly plateGoal: number;
  readonly beaconRestored: boolean;
  readonly timer: string;
  readonly stormLabel: string;
  readonly stormIntensity: number;
  readonly speedKmh: number;
  readonly charge: number;
  readonly chargeSegments: number;
  readonly chargeFilled: number;
  readonly abilities: readonly AbilityCard[];
  readonly progress: number;
  readonly stormProgress: number;
  readonly nodes: readonly MiniNode[];
  readonly phase: State["phase"];
  readonly loseReason: State["loseReason"];
  readonly crashes: number;
}

const pad2 = (n: number): string => (n < 10 ? `0${n}` : String(n));

/** Format seconds as `MM:SS.d` (the reference's `01:23.4`). */
export const formatTimer = (seconds: number): string => {
  const t = Math.max(0, seconds);
  const m = Math.floor(t / 60);
  const s = Math.floor(t % 60);
  const tenth = Math.floor((t * 10) % 10);
  return `${pad2(m)}:${pad2(s)}.${tenth}`;
};

const COSTS: Record<AbilityKind, number> = {
  boost: BOOST_COST,
  drone: DRONE_COST,
  pulse: PULSE_COST,
  shield: SHIELD_COST,
};

const MAX_CD: Record<AbilityKind, number> = { boost: 36, drone: 60, pulse: 60, shield: 60 };

const buildCards = (state: State): AbilityCard[] => {
  const { runner, ability } = state;
  const active: Record<AbilityKind, boolean> = {
    boost: runner.boostTicks > 0,
    drone: ability.helper.active,
    pulse: false,
    shield: runner.shieldTicks > 0,
  };
  const cd: Record<AbilityKind, number> = {
    boost: ability.boostCd,
    drone: ability.droneCd,
    pulse: ability.pulseCd,
    shield: ability.shieldCd,
  };
  const kinds: AbilityKind[] = ["boost", "shield", "pulse", "drone"];
  return kinds.map((kind): AbilityCard => ({
    active: active[kind],
    cooldown: Math.max(0, Math.min(1, cd[kind] / MAX_CD[kind])),
    kind,
    label: kind.toUpperCase(),
    ready: cd[kind] === 0 && runner.charge + 1e-6 >= COSTS[kind],
  }));
};

const buildNodes = (state: State): MiniNode[] => {
  const { level } = state;
  const shardNodes = level.shards.map((s): MiniNode => ({ done: s.collected, kind: "shard", t: s.z / level.beaconZ }));
  const plateNodes = level.plates.map((p): MiniNode => ({ done: p.activated, kind: "plate", t: p.z / level.beaconZ }));
  return [...shardNodes, ...plateNodes, { done: state.beaconRestored, kind: "beacon", t: 1 }];
};

/** Project the live `state` into its full frame UI model. */
export const buildHud = (state: State): Hud => {
  const clamp01 = (v: number): number => Math.max(0, Math.min(1, v));
  return {
    abilities: buildCards(state),
    beaconReady: state.beaconReady,
    beaconRestored: state.beaconRestored,
    charge: state.runner.charge,
    chargeFilled: Math.round(state.runner.charge * CHARGE_SEGMENTS),
    chargeSegments: CHARGE_SEGMENTS,
    crashes: state.runner.crashes,
    loseReason: state.loseReason,
    nodes: buildNodes(state),
    objectiveTitle: "ACTIVATE RELAY",
    phase: state.phase,
    plateGoal: PLATE_GOAL,
    plates: state.platesActivated,
    progress: clamp01(state.runner.dist / state.level.beaconZ),
    shardGoal: SHARD_GOAL,
    shards: state.shardsCollected,
    speedKmh: Math.round(Math.max(0, state.runner.speed) * KMH_FACTOR),
    stormIntensity: state.storm.intensity,
    stormLabel: state.storm.intensity > 0.66 ? "STORM HERE" : "STORM APP.",
    stormProgress: clamp01(state.storm.dist / state.level.beaconZ),
    timer: formatTimer(state.timeLeft),
  };
};
