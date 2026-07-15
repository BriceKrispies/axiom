/*
 * result-source.ts — the RESULT-SOURCE boundary. The chance engine never calls
 * `Math.random()`; a session resolves outcomes through exactly one of these
 * two sources:
 *
 * - `SeededChanceResultSource` — development, previews, tests, standalone
 *   play. Receives its seed explicitly; the same (seed, config, round, player
 *   context) always resolves the same `OutcomePlan`. It draws only from the
 *   gameplay / placement / tier streams in `randomness/streams.ts`.
 *
 * - `InjectedChanceResultSource` — integration with an authoritative service.
 *   Transport-neutral and app-local: something outside the game supplies (or
 *   asynchronously delivers) a committed outcome; `resolve` returns null until
 *   it has arrived, and the game only animates and reveals what was supplied.
 */

import type { CasinoGameConfig } from "../configuration/schema.ts";
import { presentationSeedOf } from "../randomness/streams.ts";
import type { ChoicePopulation } from "../probability/choice-population.ts";
import { planChoicePopulation } from "../probability/choice-population.ts";
import type { CombinationSpace } from "../probability/combination.ts";
import { planCombination } from "../probability/combination.ts";
import type { DestinationSlot } from "../probability/destination.ts";
import { planDestination } from "../probability/destination.ts";
import { planSingleReveal } from "../probability/single-reveal.ts";
import type { MechanicManifestation, OutcomePlan, OutcomeResolutionContext } from "./plan.ts";

/** What a game's mechanic needs resolved, declared at session creation. */
export type MechanicInit =
  | { readonly kind: "choice"; readonly choiceCount: number }
  | { readonly kind: "destination"; readonly slots: readonly DestinationSlot[] }
  | { readonly kind: "combination"; readonly space: CombinationSpace }
  | { readonly kind: "single" };

/** The pre-round mechanic plan. For a SEEDED choice game the population is
 * assigned here — before the player can possibly choose. */
export type MechanicPlan =
  | { readonly kind: "choice"; readonly population: ChoicePopulation | null; readonly choiceCount: number }
  | { readonly kind: "destination"; readonly slots: readonly DestinationSlot[] }
  | { readonly kind: "combination"; readonly space: CombinationSpace }
  | { readonly kind: "single" };

export interface ChanceResultRequest {
  readonly config: CasinoGameConfig<unknown>;
  readonly round: number;
  readonly mechanicPlan: MechanicPlan;
  readonly context: OutcomeResolutionContext;
}

/** The boundary a session resolves through. `resolve` is polled from the
 * committing phase: it returns the committed plan, or null while pending. */
export interface ChanceResultSource {
  readonly kind: "seeded" | "injected";
  readonly prepareRound: (config: CasinoGameConfig<unknown>, round: number, mechanic: MechanicInit) => MechanicPlan;
  readonly resolve: (request: ChanceResultRequest) => OutcomePlan | null;
}

const rewardOf = (config: CasinoGameConfig<unknown>, tierId: string | null) =>
  config.rewardTiers.find((tier) => tier.id === tierId)?.reward ?? null;

// ── seeded ──────────────────────────────────────────────────────────────────────

export class SeededChanceResultSource implements ChanceResultSource {
  public readonly kind = "seeded";
  public readonly seed: number;

  public constructor(seed: number) {
    this.seed = seed >>> 0;
  }

  public prepareRound(config: CasinoGameConfig<unknown>, round: number, mechanic: MechanicInit): MechanicPlan {
    if (mechanic.kind === "choice") {
      return {
        choiceCount: mechanic.choiceCount,
        kind: "choice",
        population: planChoicePopulation(config, mechanic.choiceCount, this.seed, round),
      };
    }
    return mechanic;
  }

  public resolve(request: ChanceResultRequest): OutcomePlan {
    const { config, context, mechanicPlan, round } = request;
    const roundId = `${this.seed}#${round}`;
    const presentationSeed = presentationSeedOf(this.seed, round);
    const finish = (win: boolean, tierId: string | null, manifestation: MechanicManifestation): OutcomePlan => ({
      manifestation,
      presentationSeed,
      reward: win ? rewardOf(config, tierId) : null,
      roundId,
      tierId: win ? tierId : null,
      win,
    });

    switch (mechanicPlan.kind) {
      case "choice": {
        const population = mechanicPlan.population as ChoicePopulation;
        const selectedIndex = context.selectedIndex ?? 0;
        const tierId = population.winnersByIndex[selectedIndex] ?? null;
        return finish(tierId !== null, tierId, {
          kind: "choice",
          selectedIndex,
          winnerCount: population.winnerCount,
          winnersByIndex: population.winnersByIndex,
        });
      }
      case "destination": {
        const plan = planDestination(mechanicPlan.slots, config.targetWinRate, this.seed, round);
        return finish(plan.win, plan.slot.tierId, {
          destinationId: plan.slot.id,
          destinationIndex: plan.index,
          kind: "destination",
        });
      }
      case "combination": {
        const plan = planCombination(config, mechanicPlan.space, this.seed, round);
        return finish(plan.win, plan.tierId, { combination: plan.combination, kind: "combination" });
      }
      case "single": {
        const plan = planSingleReveal(config, this.seed, round);
        const focusIndex = context.targetedPrizeIndex ?? context.castRegion ?? context.selectedIndex ?? 0;
        return finish(plan.win, plan.tierId, { focusIndex, kind: "single" });
      }
    }
  }
}

// ── injected ────────────────────────────────────────────────────────────────────

/** A committed outcome supplied by an authoritative service (transport-neutral). */
export interface InjectedOutcome {
  readonly roundId: string;
  readonly win: boolean;
  readonly tierId: string | null;
  readonly presentationSeed: number;
  /** Optional game-specific resolution data (destination index, combination…). */
  readonly resolution?: Partial<MechanicManifestation> & { readonly kind?: MechanicManifestation["kind"] };
}

/**
 * Holds outcomes delivered from outside. `supply` may be called before or
 * after the game reaches its commitment point; `resolve` returns null until
 * the round's outcome has arrived. The manifestation is BUILT FROM the
 * supplied result (+ its presentation seed) — the game only animates it.
 */
export class InjectedChanceResultSource implements ChanceResultSource {
  public readonly kind = "injected";
  readonly #pending = new Map<number, InjectedOutcome>();

  /** Deliver the committed outcome for `round`. */
  public supply(round: number, outcome: InjectedOutcome): void {
    this.#pending.set(round, outcome);
  }

  public prepareRound(_config: CasinoGameConfig<unknown>, _round: number, mechanic: MechanicInit): MechanicPlan {
    // No foreknowledge: the population/destination manifests from the outcome.
    return mechanic.kind === "choice"
      ? { choiceCount: mechanic.choiceCount, kind: "choice", population: null }
      : mechanic;
  }

  public resolve(request: ChanceResultRequest): OutcomePlan | null {
    const outcome = this.#pending.get(request.round);
    if (outcome === undefined) {
      return null;
    }
    const { config, context, mechanicPlan } = request;
    return {
      manifestation: manifestInjected(mechanicPlan, outcome, context),
      presentationSeed: outcome.presentationSeed,
      reward: outcome.win ? rewardOf(config, outcome.tierId) : null,
      roundId: outcome.roundId,
      tierId: outcome.win ? outcome.tierId : null,
      win: outcome.win,
    };
  }
}

/** Build the mechanic manifestation for an injected outcome. For a choice game
 * the SELECTED index manifests the committed result (committed before the pick
 * — the authority decided it); remaining slots decorate from the presentation
 * seed via the placement stream, never changing the material result. */
const manifestInjected = (
  plan: MechanicPlan,
  outcome: InjectedOutcome,
  context: OutcomeResolutionContext,
): MechanicManifestation => {
  switch (plan.kind) {
    case "choice": {
      const selectedIndex = context.selectedIndex ?? 0;
      const winnersByIndex = Array.from({ length: plan.choiceCount }, (_, i) =>
        i === selectedIndex ? (outcome.win ? outcome.tierId : null) : null,
      );
      return {
        kind: "choice",
        selectedIndex,
        winnerCount: outcome.win ? 1 : 0,
        winnersByIndex,
        ...(outcome.resolution?.kind === "choice" ? outcome.resolution : {}),
      } as MechanicManifestation;
    }
    case "destination": {
      const supplied = outcome.resolution?.kind === "destination" ? outcome.resolution : undefined;
      const index = (supplied as { destinationIndex?: number } | undefined)?.destinationIndex ?? 0;
      return {
        destinationId: plan.slots[index]?.id ?? String(index),
        destinationIndex: index,
        kind: "destination",
      };
    }
    case "combination": {
      const supplied = outcome.resolution?.kind === "combination" ? outcome.resolution : undefined;
      const combination = (supplied as { combination?: readonly number[] } | undefined)?.combination ?? [];
      return { combination, kind: "combination" };
    }
    case "single": {
      const focusIndex = context.targetedPrizeIndex ?? context.castRegion ?? context.selectedIndex ?? 0;
      return { focusIndex, kind: "single" };
    }
  }
};
