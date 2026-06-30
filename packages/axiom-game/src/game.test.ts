import assert from "node:assert/strict";
import { test } from "node:test";

import { createGame, type GameConfig } from "./game.ts";
import { activeRegistry, onFixedUpdate, onRender } from "./registry.ts";

const config = (): GameConfig => ({ fixedHz: 60, seed: 4660n, surface: "#stage" });

test("createGame starts idle and carries its config", () => {
  const game = createGame(config());
  assert.equal(game.status, "idle");
  assert.equal(game.config.fixedHz, 60);
  assert.equal(game.config.seed, 4660n);
  assert.equal(game.config.surface, "#stage");
});

test("the game lifecycle walks idle -> running -> paused -> running -> stopped", () => {
  const game = createGame(config());
  game.start();
  assert.equal(game.status, "running");
  game.pause();
  assert.equal(game.status, "paused");
  game.resume();
  assert.equal(game.status, "running");
  game.stop();
  assert.equal(game.status, "stopped");
});

test("createGame mints a fresh registry hung on the game and installed as active", () => {
  const game = createGame(config());
  assert.equal(activeRegistry(), game.registry);
  assert.equal(game.registry.fixedUpdates().length, 0);
  assert.equal(game.registry.renders().length, 0);
});

test("the free onFixedUpdate/onRender target the active game's registry", () => {
  const game = createGame(config());
  onFixedUpdate(() => {
    // a registration on this game's registry
  });
  onRender(() => {
    // a render on this game's registry
  });
  assert.equal(game.registry.fixedUpdates().length, 1);
  assert.equal(game.registry.renders().length, 1);
});

test("each createGame mints its own registry; a later game does not clobber the first (SPEC-14 §9)", () => {
  const first = createGame(config());
  onFixedUpdate(() => {
    // lands on the first game's registry
  });
  assert.equal(first.registry.fixedUpdates().length, 1);

  // A second game gets its OWN fresh registry; the first keeps its registrations.
  const second = createGame(config());
  assert.equal(activeRegistry(), second.registry);
  assert.equal(second.registry.fixedUpdates().length, 0);
  assert.equal(first.registry.fixedUpdates().length, 1);

  // The free functions now target the second (active) registry only.
  onFixedUpdate(() => {
    // lands on the second game's registry
  });
  assert.equal(second.registry.fixedUpdates().length, 1);
  assert.equal(first.registry.fixedUpdates().length, 1);
});
