import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "./content/bundle.ts";
import { EventSink } from "./events.ts";
import type { PlayerState, UnitInstance } from "./model.ts";
import { resolveForges } from "./forge.ts";
import type { ForgeEnv } from "./forge.ts";
import { DEFAULT_RULES } from "./tuning.ts";

const content = loadDefaultContent();
const card = content.collectibleCards[0]!;

const makeEnv = (): ForgeEnv => {
  let n = 5000;
  return { rules: DEFAULT_RULES, content, events: new EventSink(), allocate: () => (n += 1) };
};

let idc = 1;
const copy = (): UnitInstance => ({
  instanceId: (idc += 1), cardId: card.id, forged: false, attack: card.baseAttack, health: card.baseHealth, grantedKeywords: [], visualStage: 0,
});

const player = (warband: (UnitInstance | null)[], hand: UnitInstance[] = []): PlayerState => ({
  id: 0, name: "", health: 30, gold: 0, forgeRank: 1, shop: [], shopFrozen: false, hand,
  warband: [...warband, ...Array<null>(7 - warband.length).fill(null)], eliminated: false, placement: 0,
  lastOpponent: null, opponentHistory: [], combatResult: null, presentationStage: "workshop", isBot: false,
});

const forgedUnits = (p: PlayerState): UnitInstance[] => [...p.warband, ...p.hand].filter((u): u is UnitInstance => u !== null && u.forged);
const normalUnits = (p: PlayerState): UnitInstance[] => [...p.warband, ...p.hand].filter((u): u is UnitInstance => u !== null && !u.forged);

test("three normal copies forge into exactly one forged unit at a deterministic slot", () => {
  const env = makeEnv();
  const p = player([copy(), copy(), copy()]);
  resolveForges(env, p);
  assert.equal(forgedUnits(p).length, 1);
  assert.equal(normalUnits(p).length, 0);
  const forged = p.warband[0];
  assert.ok(forged !== null && forged.forged, "forged unit lands at the leftmost consumed slot (0)");
  assert.equal(forged.attack, card.baseAttack + card.forgedStats.attack);
  assert.equal(forged.health, card.baseHealth + card.forgedStats.health);
  assert.equal(p.warband[1], null);
  assert.equal(p.warband[2], null);
  const events = env.events as EventSink;
  assert.ok(events.all().some((e) => e.kind === "unit_forged"));
  assert.ok(events.all().some((e) => e.kind === "forge_reward_granted"));
});

test("forging consumes exactly three copies (a fourth remains normal)", () => {
  const p = player([copy(), copy(), copy(), copy()]);
  resolveForges(makeEnv(), p);
  assert.equal(forgedUnits(p).length, 1);
  assert.equal(normalUnits(p).length, 1);
});

test("forge destination is deterministic across hand + warband (leftmost warband copy)", () => {
  const p = player([null, copy(), null, copy()], [copy()]);
  resolveForges(makeEnv(), p);
  const forged = p.warband[1];
  assert.ok(forged !== null && forged.forged, "destination is the leftmost warband copy's slot (1)");
  assert.equal(forgedUnits(p).length, 1);
  assert.equal(p.hand.length, 0, "the hand copy was consumed");
});
