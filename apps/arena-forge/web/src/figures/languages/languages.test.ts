import { strict as assert } from "node:assert";
import { test } from "node:test";

import { allLanguages, languageFor } from "./index.ts";

test("every authored per-role roughness is a legal 0..1 value", () => {
  for (const lang of allLanguages()) {
    const roles = lang.roughnessRoles ?? {};
    for (const [role, value] of Object.entries(roles)) {
      assert.ok(value !== undefined && value >= 0 && value <= 1, `${lang.id}.${role} roughness ${value} out of 0..1`);
    }
  }
});

test("Ironbound authors glossy metal/plate and matte cape/accent", () => {
  const iron = languageFor("ironbound").roughnessRoles;
  assert.ok(iron !== undefined, "ironbound authors roughness");
  assert.ok((iron?.metal ?? 1) < 0.35, "bare metal reads glossy");
  assert.ok((iron?.accent ?? 0) > (iron?.metal ?? 1), "cape/accent trim is matter than metal");
});

test("Bloomtide organics read matte; Echowisp crystal reads glossy", () => {
  assert.ok((languageFor("bloomtide").roughnessRoles?.primary ?? 0) > 0.7, "plant matter is matte");
  assert.ok((languageFor("echowisp").roughnessRoles?.primary ?? 1) < 0.4, "arcane crystal is glossy");
});
