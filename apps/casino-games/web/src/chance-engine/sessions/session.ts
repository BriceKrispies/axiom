/*
 * session.ts — one deterministic round of one game, as an IMMUTABLE VALUE plus
 * pure transition functions (the home-run "pure step over immutable state"
 * idiom, without the class). A game's state embeds a `SessionState` and folds
 * it forward inside its own `update`; every rule the fairness contract makes
 * — legal phases only, one commitment, no reroll after commitment — is
 * enforced HERE, not re-implemented per game.
 */

import type { CasinoGameConfig } from "../configuration/schema.ts";
import { configHash } from "../configuration/serialization.ts";
import type { SessionAuditRecord } from "../diagnostics/audit.ts";
import type { OutcomePlan, OutcomeResolutionContext } from "../outcomes/plan.ts";
import type { ChanceResultSource, MechanicInit, MechanicPlan } from "../outcomes/result-source.ts";
import { STREAM_PURPOSES, streamSeed } from "../randomness/streams.ts";
import type { GamePhase } from "./phases.ts";
import { isInputLockedPhase, isLegalTransition } from "./phases.ts";

export interface SessionState {
  readonly config: CasinoGameConfig<unknown>;
  readonly seed: number;
  readonly round: number;
  readonly phase: GamePhase;
  readonly tick: number;
  readonly phaseStartTick: number;
  readonly mechanicPlan: MechanicPlan;
  readonly committed: OutcomePlan | null;
  readonly commitPhase: GamePhase | null;
  readonly commitTick: number | null;
  readonly inputContext: OutcomeResolutionContext | null;
  readonly completePhase: GamePhase | null;
  readonly completeTick: number | null;
  readonly replay: boolean;
}

/** Begin a fresh round in phase "intro". The mechanic plan (e.g. a seeded
 * choice population) is prepared NOW — before any player interaction exists. */
export const createSession = (
  config: CasinoGameConfig<unknown>,
  seed: number,
  round: number,
  source: ChanceResultSource,
  mechanic: MechanicInit,
  replay = false,
): SessionState => ({
  commitPhase: null,
  commitTick: null,
  committed: null,
  completePhase: null,
  completeTick: null,
  config,
  inputContext: null,
  mechanicPlan: source.prepareRound(config, round, mechanic),
  phase: "intro",
  phaseStartTick: 0,
  replay,
  round,
  seed: seed >>> 0,
  tick: 0,
});

/** Advance the session clock by one fixed tick. */
export const tickSession = (s: SessionState): SessionState => ({ ...s, tick: s.tick + 1 });

/** Move to `phase`. Throws on an illegal transition — a game bug, not a state.
 * The reveal is additionally sealed behind commitment: no committed outcome,
 * no "revealing", regardless of the phase graph. */
export const transition = (s: SessionState, phase: GamePhase): SessionState => {
  if (!isLegalTransition(s.phase, phase)) {
    throw new Error(`illegal phase transition ${s.phase} → ${phase} (game ${s.config.gameId})`);
  }
  if (phase === "revealing" && s.committed === null) {
    throw new Error(`cannot enter revealing without a committed outcome (game ${s.config.gameId})`);
  }
  const completed = phase === "complete" ? { completePhase: phase, completeTick: s.tick } : {};
  return { ...s, phase, phaseStartTick: s.tick, ...completed };
};

/** Ticks spent in the current phase (drives phase animation timelines). */
export const phaseAge = (s: SessionState): number => s.tick - s.phaseStartTick;

/** Whether player input must be ignored right now. */
export const inputLocked = (s: SessionState): boolean => isInputLockedPhase(s.phase);

/**
 * Resolve and commit the material outcome. Legal only in the "committing"
 * phase; committing twice throws. Returns the unchanged state while an
 * injected source is still pending (poll again next tick).
 */
export const commitOutcome = (
  s: SessionState,
  source: ChanceResultSource,
  context: OutcomeResolutionContext,
): SessionState => {
  if (s.phase !== "committing") {
    throw new Error(`commitOutcome outside the committing phase (in ${s.phase})`);
  }
  if (s.committed !== null) {
    throw new Error(`outcome already committed for round ${s.committed.roundId} — a committed result cannot reroll`);
  }
  const plan = source.resolve({ config: s.config, context, mechanicPlan: s.mechanicPlan, round: s.round });
  if (plan === null) {
    return s;
  }
  return { ...s, commitPhase: s.phase, commitTick: s.tick, committed: plan, inputContext: context };
};

/** The audit record for the diagnostics drawer (dev-only surface). */
export const auditOf = (s: SessionState, sourceKind: "seeded" | "injected"): SessionAuditRecord => ({
  commitPhase: s.commitPhase,
  commitTick: s.commitTick,
  completePhase: s.completePhase,
  completeTick: s.completeTick,
  configHash: configHash(s.config),
  gameId: s.config.gameId,
  inputContext: s.inputContext,
  manifestation: s.committed?.manifestation ?? null,
  replay: s.replay,
  round: s.round,
  schemaVersion: s.config.schemaVersion,
  seedOrRoundId: sourceKind === "seeded" ? String(s.seed) : (s.committed?.roundId ?? "pending"),
  streamSeeds: Object.fromEntries(STREAM_PURPOSES.map((p) => [p, streamSeed(s.seed, p)])) as SessionAuditRecord["streamSeeds"],
  tierId: s.committed?.tierId ?? null,
  win: s.committed?.win ?? null,
});
