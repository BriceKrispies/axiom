import { strict as assert } from "node:assert";
import { test } from "node:test";

import { LoadedContent } from "../content/load.ts";
import type { CardDefinition, ContentBundle, VisualProfile } from "../content/schema.ts";
import { EventSink } from "../events.ts";
import { InstanceIdAllocator } from "../ids.ts";
import type { UnitInstance, WarbandSnapshot } from "../model.ts";
import { WARBAND_SLOTS } from "../model.ts";
import { DEFAULT_RULES } from "../tuning.ts";
import { runCombat } from "./engine.ts";

// ── minimal fixture content ───────────────────────────────────────────────────
const VP: VisualProfile = {
  id: "vp", frame: "", portrait: "", border: "", base: "", idle: "", entrance: "",
  attackTrail: "", impact: "", death: "", aura: "", groupColor: "#fff", particleBudget: 0, soundCues: [],
};

const mkCard = (over: Partial<CardDefinition> & { id: string }): CardDefinition => ({
  name: over.id, rulesText: "", tier: 1, cost: 3, baseAttack: 1, baseHealth: 1, groups: [], keywords: [],
  normal: [], forged: [], forgedStats: { attack: 1, health: 1 }, visualProfile: "vp", forgedVisualProfile: "vp",
  poolCount: 1, collectible: true, contentVersion: 1, ...over,
});

const mkContent = (cards: CardDefinition[]): LoadedContent => {
  const bundle: ContentBundle = {
    version: 1, archetypes: [],
    keywords: [{ id: "guard", name: "Guard", description: "" }, { id: "armored", name: "Armored", description: "" }],
    groups: [], cards, visualProfiles: [VP],
  };
  return new LoadedContent(bundle);
};

let idc = 100;
const unit = (cardId: string, attack: number, health: number, keywords: string[] = [], forged = false): UnitInstance => ({
  instanceId: (idc += 1), cardId, forged, attack, health, grantedKeywords: keywords, visualStage: 0,
});

const snap = (ownerId: number, slots: (UnitInstance | null)[], ghost = false): WarbandSnapshot => ({
  ownerId, forgeRank: 1, ghost, slots: [...slots, ...Array<null>(WARBAND_SLOTS - slots.length).fill(null)],
});

const config = (content: LoadedContent, seed: number, events = new EventSink()) => {
  const alloc = new InstanceIdAllocator(1);
  return { rules: DEFAULT_RULES, content, events, combatId: 0, seed, allocate: () => alloc.allocate() };
};

// ── tests ─────────────────────────────────────────────────────────────────────
test("guard is targeted before other units", () => {
  const content = mkContent([mkCard({ id: "atk", baseAttack: 5, baseHealth: 9 }), mkCard({ id: "def" })]);
  const events = new EventSink();
  const nonGuard = unit("def", 1, 5);
  const guard = unit("def", 0, 5, ["guard"]);
  runCombat(config(content, 1, events), snap(0, [unit("atk", 5, 9)]), snap(1, [nonGuard, guard]));
  const firstAttack = events.all().find((e) => e.kind === "attack_started" && e.side === "a");
  assert.ok(firstAttack && firstAttack.kind === "attack_started");
  assert.equal(firstAttack.defender, guard.instanceId, "attacker must hit the guard, not the leftmost non-guard");
});

test("attack damage is simultaneous — two equal units both die (draw)", () => {
  const content = mkContent([mkCard({ id: "u", baseAttack: 3, baseHealth: 3 })]);
  const result = runCombat(config(content, 7), snap(0, [unit("u", 3, 3)]), snap(1, [unit("u", 3, 3)]));
  assert.equal(result.aVerdict, "draw");
  assert.equal(result.survivors, 0);
});

test("armored mitigates one point of each incoming hit", () => {
  const content = mkContent([mkCard({ id: "u" })]);
  const events = new EventSink();
  // 3-attack hits an armored 1/3 → 2 damage, not 3.
  runCombat(config(content, 3, events), snap(0, [unit("u", 3, 5)]), snap(1, [unit("u", 1, 3, ["armored"])]));
  const dmg = events.all().find((e) => e.kind === "unit_damaged" && e.side === "b");
  assert.ok(dmg && dmg.kind === "unit_damaged");
  assert.equal(dmg.amount, 2, "armored should reduce the 3-damage hit to 2");
});

test("position materially changes the combat result", () => {
  const content = mkContent([
    mkCard({ id: "big", baseAttack: 7, baseHealth: 1 }),
    mkCard({ id: "tank", baseAttack: 1, baseHealth: 6 }),
    mkCard({ id: "mid", baseAttack: 3, baseHealth: 3 }),
  ]);
  // Fixed instance ids so the only difference between runs is A's slot order.
  const big = unit("big", 7, 1);
  const tank = unit("tank", 1, 6);
  const bSnap = snap(1, [unit("mid", 3, 3), unit("mid", 3, 3)]);
  const streamOf = (aSlots: (UnitInstance | null)[]): string => {
    const events = new EventSink();
    runCombat(config(content, 9, events), snap(0, aSlots), bSnap);
    return JSON.stringify(events.all().filter((e) => e.kind === "unit_died" || e.kind === "combat_end"));
  };
  assert.notEqual(streamOf([big, tank]), streamOf([tank, big]), "reordering the same units should change the outcome");
});

test("death effects resolve before the next attack (deathrattle summon)", () => {
  const content = mkContent([
    mkCard({ id: "token", collectible: false, poolCount: 0, baseAttack: 2, baseHealth: 2 }),
    mkCard({
      id: "hatcher", baseAttack: 1, baseHealth: 1, tokens: ["token"],
      normal: [{ trigger: "on_death", operations: [{ kind: "summon_token", token: "token", at: { kind: "self" }, count: 1 }] }],
    }),
    mkCard({ id: "killer", baseAttack: 5, baseHealth: 9 }),
  ]);
  const events = new EventSink();
  runCombat(config(content, 2, events), snap(0, [unit("killer", 5, 9)]), snap(1, [unit("hatcher", 1, 1)]));
  const died = events.all().find((e) => e.kind === "unit_died");
  const summoned = events.all().find((e) => e.kind === "unit_summoned");
  assert.ok(died && summoned, "the hatcher must die and its deathrattle must summon");
  assert.ok(summoned.seq > died.seq, "summon (death effect) must resolve after the death, before combat continues");
});

test("a stalled combat (no damage possible) terminates as a diagnostic draw", () => {
  const content = mkContent([mkCard({ id: "wall", baseAttack: 0, baseHealth: 5 })]);
  const events = new EventSink();
  const result = runCombat(config(content, 4, events), snap(0, [unit("wall", 0, 5)]), snap(1, [unit("wall", 0, 5)]));
  assert.equal(result.bound, true);
  assert.equal(result.aVerdict, "draw");
  assert.ok(events.all().some((e) => e.kind === "diagnostic"), "a bounded combat must emit a diagnostic");
});

test("the same seed + snapshots reproduce the same combat event stream", () => {
  const content = mkContent([mkCard({ id: "u", baseAttack: 2, baseHealth: 4 })]);
  // Build the snapshots ONCE so instance ids are identical across both runs.
  const a = snap(0, [unit("u", 2, 4), unit("u", 3, 2)]);
  const b = snap(1, [unit("u", 2, 5)]);
  const run = (): string => {
    const events = new EventSink();
    runCombat(config(content, 55, events), a, b);
    return JSON.stringify(events.all());
  };
  assert.equal(run(), run());
});
