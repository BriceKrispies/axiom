/*
 * env.ts — the combat interpreter's environment and its hard runtime bounds.
 * Every combat carries a `CombatCounters` record; the interpreter increments it
 * on every event, summon, copied ability, and attack action, and flips
 * `terminated` with a reason the moment any bound is reached. The engine checks
 * that flag and ends the combat as a deterministic draw with a `diagnostic`
 * event — so no authored card, however pathological, can make combat hang.
 */

import { EFFECT_BOUNDS } from "../effects/language.ts";
import type { LoadedContent } from "../content/load.ts";
import type { EmittedEvent, EventSink } from "../events.ts";
import type { InstanceId } from "../ids.ts";
import type { Rng } from "../rng.ts";
import type { Rules } from "../tuning.ts";

export interface CombatCounters {
  events: number;
  summons: number;
  copies: number;
  actions: number;
  terminated: boolean;
  reason: string | null;
}

export const newCounters = (): CombatCounters => ({
  events: 0,
  summons: 0,
  copies: 0,
  actions: 0,
  terminated: false,
  reason: null,
});

export interface CombatEnv {
  readonly rules: Rules;
  readonly content: LoadedContent;
  readonly rng: Rng;
  readonly events: EventSink;
  readonly combatId: number;
  readonly counters: CombatCounters;
  readonly allocate: () => InstanceId;
}

/** Mark the combat terminated by a bound (idempotent — keeps the first reason). */
export const terminate = (env: CombatEnv, reason: string): void => {
  if (!env.counters.terminated) {
    env.counters.terminated = true;
    env.counters.reason = reason;
    env.events.emit({ kind: "diagnostic", combatId: env.combatId, reason });
  }
};

/** Emit a combat event under the per-combat event bound. */
export const cemit = (env: CombatEnv, event: EmittedEvent): void => {
  env.counters.events += 1;
  if (env.counters.events > EFFECT_BOUNDS.maxEventsPerCombat) {
    terminate(env, "event_bound");
    return;
  }
  env.events.emit(event);
};
