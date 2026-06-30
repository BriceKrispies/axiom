import assert from "node:assert/strict";
import { test } from "node:test";

import { Scene } from "./scene.ts";
import { mountScene } from "./scene-runtime.ts";
import { TickPump } from "./pump.ts";
import type { SimContext } from "./sim.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";

// A SimContext over a fresh fake bridge + standalone pump, the way the loop builds
// one — used to drive a real mount so the scene's `this.*` getters resolve.
const contextOf = (): SimContext => {
  const bridge = new FakeBridge();
  return { bridge, fixedHz: 60, pump: new TickPump(bridge, 60) };
};

test("the default lifecycle hooks author nothing", () => {
  const scene = new Scene();
  assert.deepEqual(scene.preload(), []);
  assert.deepEqual(scene.create(), []);
  assert.deepEqual(scene.update(0, 1 / 60), []);
});

test("reading any factory before the runtime mounts the scene throws", () => {
  const scene = new Scene();
  // Every getter fails loudly rather than leaking `undefined` before a mount.
  assert.throws(() => scene.add, /before the runtime mounted/u);
  assert.throws(() => scene.input, /before the runtime mounted/u);
  assert.throws(() => scene.physics, /before the runtime mounted/u);
  assert.throws(() => scene.time, /before the runtime mounted/u);
  assert.throws(() => scene.tweens, /before the runtime mounted/u);
  assert.throws(() => scene.sound, /before the runtime mounted/u);
  assert.throws(() => scene.cameras, /before the runtime mounted/u);
});

test("a mounted scene exposes the real per-tick factories through every getter", () => {
  const scene = new Scene();
  const mounted = mountScene(scene, contextOf());
  mounted.start(); // binds the factories from a tick-0 Sim via bindFactories
  assert.equal(typeof scene.add.sprite, "function");
  assert.equal(typeof scene.input.isDown, "function");
  assert.equal(typeof scene.physics.setConfig, "function");
  assert.equal(typeof scene.time.after, "function");
  assert.equal(typeof scene.tweens.add, "function");
  assert.equal(typeof scene.sound.play, "function");
  // `cameras` is the documented deferred stub (no 2D camera projection yet).
  assert.equal(scene.cameras.subsystem, "cameras");
});

test("a subclass authors through the bound factories and the runtime surfaces its assets", () => {
  let spawned = -1;
  class MyScene extends Scene {
    public override preload(): readonly string[] {
      return ["atlas"];
    }

    public override create(): readonly number[] {
      const object = this.add.rectangle(0, 0, { color: 0, height: 1, width: 1 });
      spawned = object.entity;
      return [object.entity];
    }
  }
  const scene = new MyScene();
  const mounted = mountScene(scene, contextOf());
  mounted.start();
  assert.deepEqual(mounted.assets(), ["atlas"]);
  assert.ok(spawned >= 0);
});
