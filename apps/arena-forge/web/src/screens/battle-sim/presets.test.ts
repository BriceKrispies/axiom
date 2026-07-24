import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../../sim/content/bundle.ts";
import { WARBAND_SLOTS } from "../../sim/model.ts";
import { TEAM_PRESETS } from "./presets.ts";
import { teamPower } from "./power.ts";

const content = loadDefaultContent();

test("every preset references only real, collectible cards", () => {
  for (const preset of TEAM_PRESETS) {
    for (const unit of preset.units) {
      const card = content.card(unit.cardId); // throws if unknown
      assert.equal(card.collectible, true, `${unit.cardId} in ${preset.id} must be collectible`);
      assert.ok(card.poolCount > 0, `${unit.cardId} in ${preset.id} must have pool copies`);
    }
  }
});

test("preset lineups fit a warband and are non-empty", () => {
  for (const preset of TEAM_PRESETS) {
    assert.ok(preset.units.length >= 1, `${preset.id} must field at least one unit`);
    assert.ok(preset.units.length <= WARBAND_SLOTS, `${preset.id} must fit ${WARBAND_SLOTS} slots`);
  }
});

test("preset ids are unique and every preset has positive power", () => {
  const ids = new Set<string>();
  for (const preset of TEAM_PRESETS) {
    assert.equal(ids.has(preset.id), false, `duplicate preset id ${preset.id}`);
    ids.add(preset.id);
    assert.ok(teamPower(content, preset.units) > 0, `${preset.id} must have positive power`);
  }
});
