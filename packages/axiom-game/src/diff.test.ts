import assert from "node:assert/strict";
import { test } from "node:test";

import { classifyUpdate, diffManifest } from "./diff.ts";
import { component, resource, scene, system, type AppManifest, type SystemHook } from "./manifest.ts";
import type { FixedUpdate, Render } from "./loop-core.ts";

const buildScene = (): void => {
  // a scene author (no-op for the diff test)
};

const CONFIG = { fixedHz: 60, seed: 1n, surface: "c" } as const;
const runA: FixedUpdate = () => {
  // behaviour A
};
const runB: FixedUpdate = () => {
  // behaviour B (distinct closure)
};
const noopRender: Render = () => {
  // a render behaviour
};
const hook: SystemHook = () => {
  // a lifecycle hook
};
const identityMigrate = (bytes: Uint8Array): Uint8Array => bytes;

const manifest = (over: Partial<AppManifest>): AppManifest => ({
  config: CONFIG,
  id: "app",
  ...over,
});

test("an identical manifest produces an empty diff and classifies as hot_patch", () => {
  const base = manifest({ systems: [system("a", { phase: "fixedUpdate", run: runA })] });
  const diff = diffManifest(base, base);
  assert.deepEqual(diff.systems.added, []);
  assert.deepEqual(diff.systems.removed, []);
  assert.deepEqual(diff.systems.runSwapped, []);
  assert.deepEqual(diff.systems.remounted, []);
  assert.equal(diff.configChanged, false);
  assert.equal(classifyUpdate(diff), "hot_patch");
});

test("a changed system body is a runSwapped hot_patch", () => {
  const prev = manifest({ systems: [system("a", { phase: "fixedUpdate", run: runA })] });
  const next = manifest({ systems: [system("a", { phase: "fixedUpdate", run: runB })] });
  const diff = diffManifest(prev, next);
  assert.deepEqual(diff.systems.runSwapped.map((d) => d.id), ["a"]);
  assert.deepEqual(diff.systems.remounted, []);
  assert.equal(classifyUpdate(diff), "hot_patch");
});

test("a changed mount hook is remounted, not runSwapped", () => {
  const prev = manifest({ systems: [system("a", { phase: "fixedUpdate", run: runA })] });
  const next = manifest({ systems: [system("a", { mount: hook, phase: "fixedUpdate", run: runB })] });
  const diff = diffManifest(prev, next);
  assert.deepEqual(diff.systems.remounted.map((d) => d.id), ["a"]);
  assert.deepEqual(diff.systems.runSwapped, []);
});

test("a changed dispose hook is remounted", () => {
  const prev = manifest({ systems: [system("a", { phase: "fixedUpdate", run: runA })] });
  const next = manifest({ systems: [system("a", { dispose: hook, phase: "fixedUpdate", run: runA })] });
  const diff = diffManifest(prev, next);
  assert.deepEqual(diff.systems.remounted.map((d) => d.id), ["a"]);
});

test("added and removed systems are reported by id", () => {
  const prev = manifest({ systems: [system("a", { phase: "fixedUpdate", run: runA })] });
  const next = manifest({ systems: [system("b", { phase: "render", run: noopRender })] });
  const diff = diffManifest(prev, next);
  assert.deepEqual(diff.systems.added.map((d) => d.id), ["b"]);
  assert.deepEqual(diff.systems.removed, ["a"]);
  assert.equal(classifyUpdate(diff), "hot_patch");
});

test("a changed resource value is patched", () => {
  const prev = manifest({ resources: [resource("m", { kind: "material", material: { baseColor: [1, 0, 0, 1] } })] });
  const next = manifest({ resources: [resource("m", { kind: "material", material: { baseColor: [0, 1, 0, 1] } })] });
  const diff = diffManifest(prev, next);
  assert.deepEqual(diff.resources.patched.map((d) => d.id), ["m"]);
  assert.equal(classifyUpdate(diff), "hot_patch");
});

test("an unchanged resource is not patched; added/removed resources are reported", () => {
  const same = resource("m", { kind: "material", material: { baseColor: [1, 0, 0, 1] } });
  const prev = manifest({ resources: [same] });
  const next = manifest({ resources: [same, resource("n", { kind: "material", material: { baseColor: [0, 0, 1, 1] } })] });
  const diff = diffManifest(prev, next);
  assert.deepEqual(diff.resources.patched, []);
  assert.deepEqual(diff.resources.added.map((d) => d.id), ["n"]);
  const back = diffManifest(next, prev);
  assert.deepEqual(back.resources.removed, ["n"]);
});

test("a component version bump with a migrator classifies as soft_app_reload", () => {
  const prev = manifest({ components: [component("h", { version: 1 })] });
  const next = manifest({ components: [component("h", { migrate: identityMigrate, version: 2 })] });
  const diff = diffManifest(prev, next);
  assert.deepEqual(diff.components.migrated.map((d) => d.id), ["h"]);
  assert.deepEqual(diff.components.unmigratable, []);
  assert.equal(classifyUpdate(diff), "soft_app_reload");
});

test("a component version bump without a migrator classifies as full_page_reload", () => {
  const prev = manifest({ components: [component("h", { version: 1 })] });
  const next = manifest({ components: [component("h", { version: 2 })] });
  const diff = diffManifest(prev, next);
  assert.deepEqual(diff.components.unmigratable, ["h"]);
  assert.equal(classifyUpdate(diff), "full_page_reload");
});

test("an added component classifies as soft_app_reload; an unchanged version does not", () => {
  const prev = manifest({ components: [component("h", { version: 1 })] });
  const next = manifest({ components: [component("h", { version: 1 }), component("mana", { version: 1 })] });
  const diff = diffManifest(prev, next);
  assert.deepEqual(diff.components.added.map((d) => d.id), ["mana"]);
  assert.deepEqual(diff.components.migrated, []);
  assert.equal(classifyUpdate(diff), "soft_app_reload");
  const back = diffManifest(next, prev);
  assert.deepEqual(back.components.removed, ["mana"]);
});

test("a scene is changed only when new or its version bumps; a same-version re-import is not", () => {
  // Same version (a fresh `build` closure on re-import) ⇒ NOT changed: a system edit
  // must not spuriously re-author the scene.
  const prev = manifest({ scenes: [scene("arena", { build: buildScene, version: 1 })] });
  const sameVersion = manifest({ scenes: [scene("arena", { build: buildScene, version: 1 })] });
  assert.deepEqual(diffManifest(prev, sameVersion).scenes.changed, []);

  // Bumped version ⇒ changed (the reconciler re-authors it); classifies as hot_patch.
  const bumped = manifest({ scenes: [scene("arena", { build: buildScene, version: 2 })] });
  const diff = diffManifest(prev, bumped);
  assert.deepEqual(diff.scenes.changed.map((d) => d.id), ["arena"]);
  assert.equal(classifyUpdate(diff), "hot_patch");

  // A brand-new scene ⇒ changed.
  const added = manifest({ scenes: [scene("arena", { build: buildScene, version: 1 }), scene("hud", { build: buildScene, version: 1 })] });
  assert.deepEqual(diffManifest(prev, added).scenes.changed.map((d) => d.id), ["hud"]);
});

test("a config change classifies as engine_restart_required and outranks everything", () => {
  const prev = manifest({ components: [component("h", { version: 1 })] });
  const next: AppManifest = {
    components: [component("h", { version: 2 })],
    config: { fixedHz: 30, seed: 1n, surface: "c" },
    id: "app",
  };
  const diff = diffManifest(prev, next);
  assert.equal(diff.configChanged, true);
  assert.equal(classifyUpdate(diff), "engine_restart_required");
});
