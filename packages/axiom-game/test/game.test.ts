import assert from "node:assert/strict";
import { test } from "node:test";

import { createGame, type GameConfig } from "../src/game.ts";
import { defaultRegistry, onFixedUpdate, onRender } from "../src/registry.ts";

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

test("creating a game resets the default authoring registry", () => {
  const log: number[] = [];
  onFixedUpdate(() => {
    log.push(1);
  });
  onRender(() => {
    log.push(2);
  });
  assert.ok(defaultRegistry.fixedUpdates().length > 0);
  createGame(config());
  assert.equal(defaultRegistry.fixedUpdates().length, 0);
  assert.equal(defaultRegistry.renders().length, 0);
});
