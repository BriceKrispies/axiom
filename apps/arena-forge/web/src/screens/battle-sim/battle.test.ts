import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../../sim/content/bundle.ts";
import { reconstructFrame } from "../../presentation/combat-playback.ts";
import { TEAM_PRESETS } from "./presets.ts";
import { buildEnemyTeam, teamPower } from "./power.ts";
import { forgedById, runBattle } from "./battle.ts";

const content = loadDefaultContent();
const preset = TEAM_PRESETS[0]!; // Ironbound Wall

test("runBattle is deterministic for a given seed", () => {
  const enemy = buildEnemyTeam(content, teamPower(content, preset.units), 42);
  const a = runBattle(content, preset.units, enemy, 7);
  const b = runBattle(content, preset.units, enemy, 7);
  assert.equal(a.result.winnerSide, b.result.winnerSide);
  assert.equal(a.stream.length, b.stream.length);
  assert.deepEqual(a.stream, b.stream, "same seed reproduces the event stream byte-for-byte");
});

test("runBattle produces a real, decided combat", () => {
  const enemy = buildEnemyTeam(content, teamPower(content, preset.units), 1);
  const battle = runBattle(content, preset.units, enemy, 3);
  assert.ok(battle.stream.length > 0, "combat emits an event stream");
  assert.equal(battle.stream[0]!.kind, "combat_begin");
  assert.equal(battle.stream[battle.stream.length - 1]!.kind, "combat_end");
  // Player side is "a", enemy "b".
  assert.ok(["a", "b", null].includes(battle.result.winnerSide));
});

test("instance ids are disjoint across the two sides", () => {
  const enemy = buildEnemyTeam(content, teamPower(content, preset.units), 5);
  const battle = runBattle(content, preset.units, enemy, 9);
  const aIds = battle.snapA.slots.flatMap((s) => (s === null ? [] : [s.instanceId]));
  const bIds = battle.snapB.slots.flatMap((s) => (s === null ? [] : [s.instanceId]));
  const overlap = aIds.filter((id) => bIds.includes(id));
  assert.equal(overlap.length, 0, "no instance id appears on both sides");
});

test("the final playback frame agrees with the decided result", () => {
  const enemy = buildEnemyTeam(content, teamPower(content, preset.units), 8);
  const battle = runBattle(content, preset.units, enemy, 11);
  const frame = reconstructFrame(battle.snapA, battle.snapB, battle.stream, battle.stream.length);
  const aliveA = frame.units.filter((u) => u.side === "a" && u.alive).length;
  const aliveB = frame.units.filter((u) => u.side === "b" && u.alive).length;
  if (battle.result.winnerSide === "a") {
    assert.ok(aliveA > 0 && aliveB === 0, "side a wins with survivors, b wiped");
  } else if (battle.result.winnerSide === "b") {
    assert.ok(aliveB > 0 && aliveA === 0, "side b wins with survivors, a wiped");
  }
});

test("forgedById maps forged units from both snapshots", () => {
  // Ironbound Wall forges iron_bulwark and iron_colossus.
  const enemy = buildEnemyTeam(content, teamPower(content, preset.units), 2);
  const battle = runBattle(content, preset.units, enemy, 4);
  const map = forgedById(battle);
  const forgedCount = [...map.values()].filter(Boolean).length;
  assert.ok(forgedCount >= 2, "the preset's forged units are flagged");
});
