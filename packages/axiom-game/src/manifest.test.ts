import assert from "node:assert/strict";
import { test } from "node:test";

import {
  component,
  defineApp,
  resource,
  scene,
  system,
  type AppManifest,
  type ComponentDef,
  type MaterialValue,
} from "./manifest.ts";
import type { FixedUpdate, Render } from "./loop-core.ts";
import type { GameConfig } from "./game.ts";

const CONFIG: GameConfig = { fixedHz: 60, seed: 1n, surface: "c" };
const noopFixed: FixedUpdate = () => {
  // deterministic fixed-update behaviour under test
};
const noopRender: Render = () => {
  // render behaviour under test
};
const migrate = (bytes: Uint8Array): Uint8Array => bytes;
const mount = (): void => {
  // a mount hook
};
const dispose = (): void => {
  // a dispose hook
};
const buildScene = (): void => {
  // a scene author
};

test("system builds a fixed-update definition keyed by id, preserving the spec", () => {
  const def = system("ball.physics", { phase: "fixedUpdate", run: noopFixed });
  assert.equal(def.id, "ball.physics");
  assert.equal(def.spec.phase, "fixedUpdate");
  assert.equal(def.spec.run, noopFixed);
});

test("system carries the optional order and lifecycle hooks when supplied", () => {
  const def = system("orb.spin", { dispose, mount, order: 5, phase: "fixedUpdate", run: noopFixed });
  assert.equal(def.spec.order, 5);
  assert.equal(def.spec.mount, mount);
  assert.equal(def.spec.dispose, dispose);
});

test("system builds a render definition", () => {
  const def = system("orb.draw", { phase: "render", run: noopRender });
  assert.equal(def.spec.phase, "render");
  assert.equal(def.spec.run, noopRender);
});

test("resource builds a material definition keyed by id", () => {
  const material: MaterialValue = { baseColor: [1, 0, 0, 1] };
  const def = resource("grass.material", { kind: "material", material });
  assert.equal(def.id, "grass.material");
  assert.equal(def.spec.kind, "material");
  assert.deepEqual(def.spec.material.baseColor, [1, 0, 0, 1]);
});

test("component builds a schema and forwards an absent migrator", () => {
  const def: ComponentDef = component("health", { version: 1 });
  assert.equal(def.id, "health");
  assert.equal(def.version, 1);
  assert.equal(def.migrate, undefined);
});

test("component forwards a supplied migrator", () => {
  const def = component("health", { migrate, version: 2 });
  assert.equal(def.version, 2);
  assert.equal(def.migrate, migrate);
});

test("scene builds a versioned definition keyed by id", () => {
  const def = scene("arena", { build: buildScene, version: 3 });
  assert.equal(def.id, "arena");
  assert.equal(def.version, 3);
  assert.equal(def.build, buildScene);
});

test("defineApp returns the manifest unchanged", () => {
  const manifest: AppManifest = {
    config: CONFIG,
    id: "orbs",
    systems: [system("orb.spin", { phase: "fixedUpdate", run: noopFixed })],
  };
  assert.equal(defineApp(manifest), manifest);
});
