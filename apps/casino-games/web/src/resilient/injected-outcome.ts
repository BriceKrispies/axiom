/*
 * injected-outcome.ts — the server's answer, in the vocabulary the shipped game
 * already speaks.
 *
 * The engine-rendered rung does NOT decide anything. It mounts the real
 * Treasure Chest Pick with an `InjectedChanceResultSource` — the chance
 * engine's existing boundary for "an authoritative service outside the game
 * committed this outcome; you may only animate it" — and the game's
 * `committing` phase polls that source until the answer arrives. Which is
 * exactly the shape of a form POST: press, hold, reveal what came back.
 *
 * This file is the whole translation, and it is a pure function of the wire
 * response, so it is testable with no DOM, no engine and no network.
 *
 * WHAT IS AND IS NOT CARRIED OVER. The material facts — did it win, which
 * reward tier — come from the response and nothing else. The `presentationSeed`
 * is DERIVED, through the same `presentationSeedOf` the seeded source uses, so
 * the celebration's cosmetic streams are deterministic for a given round
 * without the page ever drawing entropy of its own (this build has no seed; see
 * `main.ts`). A seed cannot change a committed outcome — it only decides which
 * sparks fly.
 */

import type { InjectedOutcome } from "../chance-engine/outcomes/result-source.ts";
import { presentationSeedOf } from "../chance-engine/randomness/streams.ts";
import type { PickResponse } from "./contract.ts";

/**
 * The committed outcome, as the chance engine's injected source wants it.
 *
 * `tierId` is read off the revealed reward rather than trusted from `won`
 * alone: the game resolves that id against its reward ladder
 * (`CHEST_REWARD_TIERS`, the same one the server decided with), so a response
 * that claims a win without naming a tier reveals an empty chest instead of
 * inventing a prize.
 */
export const injectedOutcomeOf = (response: PickResponse): InjectedOutcome => ({
  presentationSeed: presentationSeedOf(response.seed, response.round),
  roundId: `${response.seed}#${response.round}`,
  tierId: response.reward?.tierId ?? null,
  win: response.won && response.reward !== null,
});
