import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../../sim/content/bundle.ts";
import { TEAM_PRESETS } from "./presets.ts";
import { buildEnemyTeam, hashString, teamPower, unitPower } from "./power.ts";

const content = loadDefaultContent();

test("unitPower rewards forging and defensive keywords", () => {
  // Iron Bulwark is guard+armored; forging adds stats too.
  const normal = unitPower(content, "iron_bulwark", false);
  const forged = unitPower(content, "iron_bulwark", true);
  assert.ok(forged > normal, "forged unit is more powerful");
  // A vanilla body of the same stat line but no keywords is worth less.
  const vanilla = unitPower(content, "iron_vanguard", false); // 5/6, no keywords
  const bulwarkStats = content.card("iron_bulwark");
  const bulwarkBare = bulwarkStats.baseAttack + bulwarkStats.baseHealth; // 4+7 = 11
  assert.equal(normal, bulwarkBare + 2 + 3, "guard(+2) and armored(+3) add to raw stats");
  assert.ok(vanilla < normal, "keywords make the bulwark score above a bare 5/6");
});

test("buildEnemyTeam is deterministic for a given target and seed", () => {
  const a = buildEnemyTeam(content, 80, 12345);
  const b = buildEnemyTeam(content, 80, 12345);
  assert.deepEqual(a, b, "same target + seed reproduce the same team");
  const c = buildEnemyTeam(content, 80, 999);
  assert.notDeepEqual(a, c, "a different seed varies the team");
});

test("buildEnemyTeam matches each preset's power within a fair band", () => {
  for (const preset of TEAM_PRESETS) {
    const target = teamPower(content, preset.units);
    for (let gen = 0; gen < 6; gen += 1) {
      const enemy = buildEnemyTeam(content, target, hashString(preset.id) ^ gen);
      const power = teamPower(content, enemy);
      assert.ok(enemy.length >= 1 && enemy.length <= 7, `${preset.id} enemy fields 1..7 units`);
      // "Roughly equal": within one strong unit's worth of the target.
      const slack = Math.max(10, Math.round(target * 0.18));
      assert.ok(
        Math.abs(power - target) <= slack,
        `${preset.id} gen${gen}: enemy power ${power} should be within ${slack} of ${target}`,
      );
    }
  }
});

test("hashString is stable and non-trivial", () => {
  assert.equal(hashString("ironbound_wall"), hashString("ironbound_wall"));
  assert.notEqual(hashString("ironbound_wall"), hashString("emberkin_blitz"));
});
