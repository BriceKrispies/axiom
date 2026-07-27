/*
 * contract.test.ts — the one rule both sides of the wire share.
 *
 * `parsePick` is what turns "the string the browser sent" into "the chest the
 * player meant", on the server for a urlencoded form POST and in the page for a
 * button's `value`. If those two ever disagreed, the no-JS tier and the enhanced
 * tier would be playing subtly different games — which is exactly the drift this
 * build exists to prevent. So the rule is tested once, here, where both import
 * it from.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { describeOutcome } from "./outcome.ts";
import { parsePick, type PickResponse } from "./contract.ts";

test("a pick is an integer inside the board", () => {
  assert.equal(parsePick("0", 9), 0);
  assert.equal(parsePick("8", 9), 8);
  assert.equal(parsePick(" 5 ", 9), 5);
});

test("everything outside the board is refused, not clamped", () => {
  // Clamping would silently open a chest the player never pressed.
  [undefined, "", "  ", "9", "-1", "4.5", "four", "NaN", "Infinity", "1e1"].forEach((raw) => {
    assert.equal(parsePick(raw, 9), null, `expected ${String(raw)} to be refused`);
  });
});

const response = (overrides: Partial<PickResponse> = {}): PickResponse => ({
  board: Array.from({ length: 9 }, (unused, index) => ({ index, reward: null })),
  chestCount: 9,
  kind: "pick",
  picked: 2,
  replay: false,
  reward: null,
  round: 3,
  seed: 4242,
  targetWinRate: 0.44,
  winnerCount: 4,
  won: false,
  ...overrides,
});

test("a loss reads as a loss, in one-based chest numbers", () => {
  const copy = describeOutcome(response());
  assert.equal(copy.won, false);
  assert.equal(copy.headline, "Empty chest");
  assert.match(copy.detail, /chest 3\. That chest was empty\./);
  assert.match(copy.facts, /seed 4242 · round 3 · 4 of 9 chests held a prize · target 44%/);
});

test("a win names the tier and the reward", () => {
  const copy = describeOutcome(
    response({
      board: [{ index: 2, reward: { rarity: "rare", rewardLabel: "Radiant gem", tierId: "rare", tierLabel: "Gem Trophy" } }],
      reward: { rarity: "rare", rewardLabel: "Radiant gem", tierId: "rare", tierLabel: "Gem Trophy" },
      won: true,
    }),
  );
  assert.equal(copy.headline, "You won!");
  assert.match(copy.detail, /Gem Trophy — Radiant gem \(rare\)/);
  assert.equal(copy.board[0], "Chest 3 (yours): Gem Trophy — Radiant gem");
});

test("a replayed round says so, so a refresh is never mistaken for a new pick", () => {
  assert.match(describeOutcome(response({ replay: true })).detail, /already decided/);
  assert.doesNotMatch(describeOutcome(response()).detail, /already decided/);
});
