/*
 * configuration.test.ts — validation and serialization: win-rate bounds, tier
 * weight rules, JSON export/import round-trips, unknown schema versions fail
 * with a readable issue, and the config hash is order-stable.
 */

import assert from "node:assert/strict";
import test from "node:test";

import type { CasinoGameConfig } from "./schema.ts";
import { baseConfig, CONFIG_SCHEMA_VERSION, DEFAULT_REWARD_TIERS } from "./schema.ts";
import { configHash, exportConfigJson, importConfigJson } from "./serialization.ts";
import { validateConfig } from "./validation.ts";

const valid = (): CasinoGameConfig<Record<string, never>> => baseConfig("test-game", "Test Game", "showcase", {});

test("a well-formed config validates clean", () => {
  assert.deepEqual(validateConfig(valid()), []);
});

test("targetWinRate rejects below zero, above one, NaN, and infinity", () => {
  for (const bad of [-0.01, 1.01, Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
    const issues = validateConfig({ ...valid(), targetWinRate: bad });
    assert.ok(issues.some((issue) => issue.path === "targetWinRate"), `expected an issue for ${bad}`);
  }
});

test("invalid tier weights fail", () => {
  for (const badWeight of [-1, Number.NaN, Number.POSITIVE_INFINITY]) {
    const tiers = [{ ...DEFAULT_REWARD_TIERS[0]!, weight: badWeight }, ...DEFAULT_REWARD_TIERS.slice(1)];
    const issues = validateConfig({ ...valid(), rewardTiers: tiers });
    assert.ok(issues.some((issue) => issue.path.includes("weight")), `expected weight issue for ${badWeight}`);
  }
});

test("wins possible but no winnable tier fails", () => {
  const tiers = DEFAULT_REWARD_TIERS.map((tier) => ({ ...tier, countsAsWin: false }));
  const issues = validateConfig({ ...valid(), rewardTiers: tiers });
  assert.ok(issues.some((issue) => issue.path === "rewardTiers"));
});

test("duplicate tier ids fail", () => {
  const tiers = [DEFAULT_REWARD_TIERS[0]!, DEFAULT_REWARD_TIERS[0]!];
  const issues = validateConfig({ ...valid(), rewardTiers: tiers });
  assert.ok(issues.some((issue) => issue.message.includes("unique")));
});

test("JSON export/import round-trips", () => {
  const config = valid();
  const result = importConfigJson<Record<string, never>>(exportConfigJson(config), "test-game");
  assert.deepEqual(result.issues, []);
  assert.deepEqual(result.config, config);
});

test("unknown schema versions fail clearly", () => {
  const config = { ...valid(), schemaVersion: CONFIG_SCHEMA_VERSION + 1 };
  const result = importConfigJson(JSON.stringify(config), "test-game");
  assert.equal(result.config, null);
  assert.ok(result.issues.some((issue) => issue.path === "schemaVersion" && issue.message.includes("unknown schema version")));
});

test("import rejects malformed JSON and wrong game ids", () => {
  assert.equal(importConfigJson("{nope", "test-game").config, null);
  const wrongId = importConfigJson(exportConfigJson(valid()), "another-game");
  assert.equal(wrongId.config, null);
  assert.ok(wrongId.issues.some((issue) => issue.path === "gameId"));
});

test("configHash is stable under key order and sensitive to values", () => {
  const config = valid();
  const reordered = JSON.parse(exportConfigJson(config)) as CasinoGameConfig<unknown>;
  assert.equal(configHash(config), configHash(reordered));
  assert.notEqual(configHash(config), configHash({ ...config, targetWinRate: 0.5 }));
});
