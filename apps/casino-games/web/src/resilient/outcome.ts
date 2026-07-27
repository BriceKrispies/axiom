/*
 * outcome.ts — the words a result is reported in, decided without a DOM.
 *
 * The server already renders a full result page for the form rung. Every rung
 * above it has to say the SAME things in place, and the fastest way for two renderings of one
 * result to disagree is for each to compose its own sentences. So the copy is a
 * pure function of the response and lives here, where a test can read it.
 *
 * Nothing in this file touches `document`. Turning this description into
 * elements is `main.ts`'s job — and it does it with `textContent`, never
 * `innerHTML`, so no string built here can become markup.
 */

import type { PickResponse, RevealedChest } from "./contract.ts";

export interface OutcomeCopy {
  readonly headline: string;
  readonly detail: string;
  readonly facts: string;
  readonly won: boolean;
  /** Per-chest text for the board summary, index-aligned. */
  readonly board: readonly string[];
}

const chestLine = (chest: RevealedChest, picked: number): string => {
  const held = chest.reward === null ? "empty" : `${chest.reward.tierLabel} — ${chest.reward.rewardLabel}`;
  return `Chest ${chest.index + 1}${chest.index === picked ? " (yours)" : ""}: ${held}`;
};

export const describeOutcome = (response: PickResponse): OutcomeCopy => {
  const prize =
    response.reward === null
      ? "That chest was empty."
      : `${response.reward.tierLabel} — ${response.reward.rewardLabel} (${response.reward.rarity}).`;
  const replayed = response.replay ? " This round was already decided, so you are seeing the recorded result." : "";

  return {
    board: response.board.map((chest) => chestLine(chest, response.picked)),
    detail: `You opened chest ${response.picked + 1}. ${prize}${replayed}`,
    facts:
      `seed ${response.seed} · round ${response.round} · ` +
      `${response.winnerCount} of ${response.chestCount} chests held a prize · ` +
      `target ${Math.round(response.targetWinRate * 100)}%`,
    headline: response.won ? "You won!" : "Empty chest",
    won: response.won,
  };
};
