/*
 * registry.test.ts — the registry contract over the REAL, ACTIVE game
 * definitions: exactly the active ids registered exactly once, no duplicate
 * stable ids, every default configuration valid (register() enforces it —
 * this suite re-checks explicitly), every definition's pure controller
 * pieces usable (a session can be created and advanced for each game), and
 * machine games declare the machine-interior camera.
 *
 * `games/index.ts` registers only the seven currently active games — the
 * rest of the original twenty-game catalog was removed from the repository
 * rather than merely disabled. This suite is a regression guardrail on the
 * active set: it must change only when a game is deliberately added back or
 * retired from `games/index.ts`.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { validateConfig } from "../chance-engine/configuration/validation.ts";
import { SeededChanceResultSource } from "../chance-engine/outcomes/result-source.ts";
import { createSession, transition } from "../chance-engine/sessions/session.ts";
import { CasinoGameRegistry } from "../chance-engine/registry/registry.ts";
import { ALL_GAMES, buildRegistry, mechanicInitFor } from "./index.ts";

/** The currently ACTIVE catalog, registered in `games/index.ts`. The other
 * thirteen games from the original catalog have been removed from the
 * repository — see `active-catalog.test.ts`. */
const REQUIRED_IDS = [
  "treasure-chest-pick",
  "prize-wheel",
  "dice-vault",
  "scratch-reveal",
  "present-pop",
  "fishing-cast",
  "gem-mine",
] as const;

test("exactly the active game ids are registered, each exactly once", () => {
  const registry = buildRegistry();
  const ids = registry.all().map((definition) => definition.id);
  assert.equal(ids.length, REQUIRED_IDS.length);
  assert.deepEqual(new Set(ids), new Set(REQUIRED_IDS));
  assert.equal(new Set(ids).size, ids.length, "no two games may share a stable id");
});

test("registering a duplicate id throws", () => {
  const registry = new CasinoGameRegistry();
  const first = ALL_GAMES[0]!;
  registry.register(first);
  assert.throws(() => registry.register(first), /duplicate game id/);
});

test("every definition has a valid default configuration", () => {
  for (const definition of ALL_GAMES) {
    const config = definition.defaultConfig();
    const issues = [...validateConfig(config), ...definition.validateSpec(config.gameSpecific as never)];
    assert.deepEqual(issues, [], `default config for ${definition.id} must validate`);
    assert.equal(config.gameId, definition.id);
  }
});

test("every game's session can be created and advanced from its default config", () => {
  for (const definition of ALL_GAMES) {
    const config = definition.defaultConfig();
    const source = new SeededChanceResultSource(4242);
    const session = createSession(config, 4242, 1, source, mechanicInitFor(definition.id, config));
    assert.equal(session.phase, "intro", definition.id);
    const ready = transition(session, "ready");
    assert.equal(ready.phase, "ready", definition.id);
  }
});

test("machine games use the machine-interior camera preset", () => {
  for (const definition of ALL_GAMES) {
    if (definition.machineInterior) {
      assert.equal(definition.defaultConfig().cameraPreset, "machine-interior", definition.id);
    }
  }
});

test("catalog metadata is complete for every game", () => {
  for (const definition of ALL_GAMES) {
    assert.ok(definition.displayName.length > 0, definition.id);
    assert.ok(definition.shortDescription.length > 8, definition.id);
    assert.ok(definition.instruction.length > 4, definition.id);
    assert.ok(definition.interaction.length > 2, definition.id);
    assert.ok(definition.categories.length > 0, definition.id);
    assert.ok(["2d", "3d"].includes(definition.renderMode), definition.id);
  }
});
