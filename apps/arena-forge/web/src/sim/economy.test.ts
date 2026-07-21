import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "./content/bundle.ts";
import { Match } from "./match.ts";
import type { MatchPlayerInit } from "./match.ts";
import type { UnitInstance } from "./model.ts";
import { LocalMatchHost } from "../api/local-host.ts";
import { DEFAULT_RULES, shopSizeForRank } from "./tuning.ts";

const content = loadDefaultContent();
const eight: MatchPlayerInit[] = Array.from({ length: 8 }, (_, i) => ({ name: `P${i}`, isBot: true }));
const newMatch = (seed = 1): Match => new Match({ matchId: "t", seed, content, players: eight });

let idc = 900;
const dummy = (): UnitInstance => ({
  instanceId: (idc += 1), cardId: content.collectibleCards[0]!.id, forged: false, attack: 1, health: 1, grantedKeywords: [], visualStage: 0,
});

test("commands are rejected outside the shop phase", () => {
  const m = newMatch();
  const r = m.submit(0, { type: "buy", shopIndex: 0, destination: { to: "hand" } });
  assert.equal(r.ok, false);
  assert.equal(r.ok === false && r.reason, "wrong_phase");
});

test("a buy with insufficient gold is rejected and transactional", () => {
  const m = newMatch();
  m.start();
  const p = m.state.players[0]!;
  p.gold = 0; // broke — every collectible costs > 0
  const shopLen = p.shop.length;
  const rejected = m.submit(0, { type: "buy", shopIndex: 0, destination: { to: "hand" } });
  assert.equal(rejected.ok, false);
  assert.equal(rejected.ok === false && rejected.reason, "not_enough_gold");
  assert.equal(p.gold, 0, "failed buy must not change gold");
  assert.equal(p.shop.length, shopLen, "failed buy must not change the shop");
  assert.equal(p.hand.length, 0, "failed buy must not add a card");
  // With gold, the same buy succeeds and spends exactly the card's cost.
  p.gold = 20;
  const cost = Math.max(0, content.card(p.shop[0]!.cardId).cost - p.shop[0]!.discount);
  const ok = m.submit(0, { type: "buy", shopIndex: 0, destination: { to: "hand" } });
  assert.equal(ok.ok, true);
  assert.ok(p.gold <= 20 - cost, "gold reduced by at least the card cost");
});

test("hand and warband limits are enforced", () => {
  const m = newMatch();
  m.start();
  const p = m.state.players[0]!;
  p.gold = 50;
  p.hand = Array.from({ length: DEFAULT_RULES.handLimit }, () => dummy());
  const handFull = m.submit(0, { type: "buy", shopIndex: 0, destination: { to: "hand" } });
  assert.equal(handFull.ok === false && handFull.reason, "hand_full");
  p.warband[2] = dummy();
  const occupied = m.submit(0, { type: "buy", shopIndex: 0, destination: { to: "warband", slot: 2 } });
  assert.equal(occupied.ok === false && occupied.reason, "slot_occupied");
});

test("a frozen shop persists exactly one refresh boundary", () => {
  const m = newMatch(77);
  m.start();
  const p = m.state.players[0]!;
  m.submit(0, { type: "set_freeze", frozen: true });
  const frozenIds = p.shop.map((s) => s.cardId);
  m.advancePhase(); // shop → combat
  m.advancePhase(); // combat → resolve → next shop
  assert.deepEqual(p.shop.map((s) => s.cardId), frozenIds, "frozen shop must be kept across one refresh");
  assert.equal(p.shopFrozen, false, "freeze is consumed after one boundary");
});

test("bots only ever issue legal commands (zero rejections in an all-bot match)", () => {
  const host = new LocalMatchHost({ seed: 321, content, allBots: true });
  host.runToCompletion();
  const rejects = host.getMatch().getEvents().filter((e) => e.kind === "command_rejected");
  assert.equal(rejects.length, 0);
});

test("shop size grows with forge rank", () => {
  for (let rank = 2; rank <= DEFAULT_RULES.maxForgeRank; rank += 1) {
    assert.ok(shopSizeForRank(DEFAULT_RULES, rank) >= shopSizeForRank(DEFAULT_RULES, rank - 1));
  }
  assert.ok(shopSizeForRank(DEFAULT_RULES, 6) > shopSizeForRank(DEFAULT_RULES, 1));
});

test("shared-pool count is conserved across a full match", () => {
  const host = new LocalMatchHost({ seed: 88, content, allBots: true });
  host.runToCompletion();
  const state = host.getMatch().state;
  for (const card of content.collectibleCards) {
    let owned = 0;
    for (const p of state.players) {
      owned += p.shop.filter((s) => s.cardId === card.id).length;
      owned += p.hand.filter((u) => u.cardId === card.id && !u.forged).length;
      owned += p.warband.filter((u) => u !== null && u.cardId === card.id && !u.forged).length;
    }
    // Forged units consumed 3 copies each (removed from circulation), so the
    // conserved quantity is pool + owned-normal + 3×(forged of this card).
    let forged = 0;
    for (const p of state.players) {
      forged += p.warband.filter((u) => u !== null && u.cardId === card.id && u.forged).length;
      forged += p.hand.filter((u) => u.cardId === card.id && u.forged).length;
    }
    const total = (state.pool.counts[card.id] ?? 0) + owned + forged * DEFAULT_RULES.copiesToForge;
    assert.equal(total, card.poolCount, `pool conservation failed for ${card.id}: ${total} != ${card.poolCount}`);
  }
});
