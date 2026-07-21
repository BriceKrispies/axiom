import { strict as assert } from "node:assert";
import { test } from "node:test";

import type { CardDefinition, ContentBundle, VisualProfile } from "./schema.ts";
import { validateContent } from "./validate.ts";

const VP: VisualProfile = {
  id: "vp", frame: "", portrait: "", border: "", base: "", idle: "", entrance: "",
  attackTrail: "", impact: "", death: "", aura: "", groupColor: "#fff", particleBudget: 0, soundCues: [],
};

const card = (over: Partial<CardDefinition> & { id: string }): CardDefinition => ({
  name: over.id, rulesText: "", tier: 1, cost: 3, baseAttack: 1, baseHealth: 1, groups: [], keywords: [],
  normal: [], forged: [], forgedStats: { attack: 1, health: 1 }, visualProfile: "vp", forgedVisualProfile: "vp",
  poolCount: 1, collectible: true, contentVersion: 1, ...over,
});

const bundle = (cards: CardDefinition[]): ContentBundle => ({
  version: 1, archetypes: [], keywords: [], groups: [], cards, visualProfiles: [VP],
});

const expectError = (b: ContentBundle, fragment: string): void => {
  const errors = validateContent(b);
  assert.ok(errors.some((e) => e.includes(fragment)), `expected an error containing "${fragment}", got:\n${errors.join("\n")}`);
};

test("valid minimal content passes", () => {
  assert.deepEqual(validateContent(bundle([card({ id: "a" })])), []);
});

test("duplicate card ids are rejected", () => {
  expectError(bundle([card({ id: "dup" }), card({ id: "dup" })]), "duplicate id 'dup'");
});

test("a missing group reference is rejected", () => {
  expectError(bundle([card({ id: "a", groups: ["ghosts"] })]), "missing group 'ghosts'");
});

test("an invalid tier is rejected", () => {
  expectError(bundle([card({ id: "a", tier: 9 as unknown as CardDefinition["tier"] })]), "invalid tier 9");
});

test("negative stats are rejected", () => {
  expectError(bundle([card({ id: "a", baseAttack: -1 })]), "negative base attack");
});

test("a missing visual profile is rejected", () => {
  expectError(bundle([card({ id: "a", visualProfile: "nope" })]), "missing visual profile 'nope'");
});

test("an operation used in the wrong context is rejected", () => {
  // deal_damage is combat-only; here it is under an economy (on_buy) trigger.
  expectError(
    bundle([card({ id: "a", normal: [{ trigger: "on_buy", operations: [{ kind: "deal_damage", target: { kind: "self" }, amount: 1 }] }] })]),
    "not allowed in a economy trigger",
  );
});

test("an over-bound repeat is rejected", () => {
  expectError(
    bundle([card({ id: "a", normal: [{ trigger: "combat_start", operations: [{ kind: "repeat", times: 99, op: { kind: "heal", target: { kind: "self" }, amount: 1 } }] }] })]),
    "repeat times 99 exceeds bound",
  );
});

test("summoning a missing token is rejected", () => {
  expectError(
    bundle([card({ id: "a", normal: [{ trigger: "combat_start", operations: [{ kind: "summon_token", token: "void", at: { kind: "self" }, count: 1 }] }] })]),
    "summons missing token 'void'",
  );
});

test("a recursive token summon cycle is rejected", () => {
  const looper = card({
    id: "loop", collectible: false, poolCount: 0,
    normal: [{ trigger: "combat_start", operations: [{ kind: "summon_token", token: "loop", at: { kind: "self" }, count: 1 }] }],
  });
  expectError(bundle([looper]), "recursive token summon cycle");
});
