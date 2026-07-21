import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "./content/bundle.ts";
import { EventSink } from "./events.ts";
import { Match } from "./match.ts";
import type { MatchPlayerInit } from "./match.ts";
import type { WarbandSnapshot } from "./model.ts";
import { applyRoundResolution } from "./resolution.ts";
import type { RoundResult } from "./resolution.ts";
import type { CombatResult } from "./combat/engine.ts";
import type { GhostStore } from "./pairing.ts";
import { DEFAULT_RULES } from "./tuning.ts";

const content = loadDefaultContent();
const eight: MatchPlayerInit[] = Array.from({ length: 8 }, (_, i) => ({ name: `P${i}`, isBot: true }));
const emptySnap = (ownerId: number, ghost = false): WarbandSnapshot => ({ ownerId, forgeRank: 1, ghost, slots: Array<null>(7).fill(null) });

const started = (): Match => {
  const m = new Match({ matchId: "t", seed: 1, content, players: eight });
  m.start();
  m.state.round = 3; // below escalation start → pure base formula
  return m;
};

test("consequence damage matches the configured formula and only the loser takes it", () => {
  const m = started();
  const store: GhostStore = { snapshots: new Map() };
  const result: CombatResult = { combatId: 0, winnerSide: "a", aVerdict: "win", bVerdict: "loss", survivors: 2, winnerForgeRank: 3, survivingTierSum: 5, bound: false };
  const rr: RoundResult = { pairing: { a: 0, b: 1, ghostOf: null }, result, snapA: emptySnap(0), snapB: emptySnap(1) };
  applyRoundResolution(m.state, DEFAULT_RULES, content, new EventSink(), store, [rr]);
  assert.equal(m.state.players[1]!.health, 30 - (2 + 3 + 5), "loser takes survivors+rank+tierSum");
  assert.equal(m.state.players[0]!.health, 30, "winner takes no damage");
});

test("a draw deals no player damage (before escalation)", () => {
  const m = started();
  const store: GhostStore = { snapshots: new Map() };
  const result: CombatResult = { combatId: 0, winnerSide: null, aVerdict: "draw", bVerdict: "draw", survivors: 0, winnerForgeRank: 0, survivingTierSum: 0, bound: false };
  const rr: RoundResult = { pairing: { a: 0, b: 1, ghostOf: null }, result, snapA: emptySnap(0), snapB: emptySnap(1) };
  applyRoundResolution(m.state, DEFAULT_RULES, content, new EventSink(), store, [rr]);
  assert.equal(m.state.players[0]!.health, 30);
  assert.equal(m.state.players[1]!.health, 30);
});

test("a ghost opponent takes no player damage", () => {
  const m = started();
  m.state.players[5]!.eliminated = true;
  m.state.players[5]!.placement = 6;
  const ghostHealthBefore = m.state.players[5]!.health;
  const store: GhostStore = { snapshots: new Map([[5, emptySnap(5)]]) };
  // Player 2 loses to the ghost of player 5 → player 2 takes damage, ghost none.
  const result: CombatResult = { combatId: 0, winnerSide: "b", aVerdict: "loss", bVerdict: "win", survivors: 1, winnerForgeRank: 1, survivingTierSum: 2, bound: false };
  const rr: RoundResult = { pairing: { a: 2, b: null, ghostOf: 5 }, result, snapA: emptySnap(2), snapB: emptySnap(5, true) };
  applyRoundResolution(m.state, DEFAULT_RULES, content, new EventSink(), store, [rr]);
  assert.equal(m.state.players[2]!.health, 30 - (1 + 1 + 2), "the live player takes the ghost's damage");
  assert.equal(m.state.players[5]!.health, ghostHealthBefore, "the ghost's own health is untouched");
});

test("simultaneous elimination assigns deterministic placements by tiebreak", () => {
  const m = started();
  // Two players both drop to <= 0 the same round; better tiebreak gets the lower placement.
  const store: GhostStore = { snapshots: new Map() };
  m.state.players[3]!.health = 4;
  m.state.players[6]!.health = 4;
  const lethal: CombatResult = { combatId: 0, winnerSide: "a", aVerdict: "win", bVerdict: "loss", survivors: 3, winnerForgeRank: 2, survivingTierSum: 4, bound: false };
  const rrA: RoundResult = { pairing: { a: 0, b: 3, ghostOf: null }, result: lethal, snapA: emptySnap(0), snapB: emptySnap(3) };
  const rrB: RoundResult = { pairing: { a: 1, b: 6, ghostOf: null }, result: lethal, snapA: emptySnap(1), snapB: emptySnap(6) };
  applyRoundResolution(m.state, DEFAULT_RULES, content, new EventSink(), store, [rrA, rrB]);
  assert.ok(m.state.players[3]!.eliminated && m.state.players[6]!.eliminated);
  // Both took the same damage from the same health, so the id tiebreak decides: 3 < 6.
  assert.ok(m.state.players[3]!.placement < m.state.players[6]!.placement);
});
