import assert from "node:assert/strict";
import { test } from "node:test";

import { Scene } from "../src/scene.ts";

test("the base Scene exposes every factory namespace as a typed stub", () => {
  const scene = new Scene();
  assert.equal(scene.add.subsystem, "add");
  assert.equal(scene.input.subsystem, "input");
  assert.equal(scene.time.subsystem, "time");
  assert.equal(scene.tweens.subsystem, "tweens");
  assert.equal(scene.sound.subsystem, "sound");
  assert.equal(scene.cameras.subsystem, "cameras");
  assert.equal(scene.physics.subsystem, "physics");
});

test("the default lifecycle hooks author nothing", () => {
  const scene = new Scene();
  assert.deepEqual(scene.preload(), []);
  assert.deepEqual(scene.create(), []);
  assert.deepEqual(scene.update(0, 1 / 60), []);
});

test("a subclass overrides the hooks and reaches the factories", () => {
  class MyScene extends Scene {
    public override preload(): readonly string[] {
      return ["atlas"];
    }

    public override create(): readonly number[] {
      // A created object is a handle wrapping an entity (retained-ECS framing).
      return [this.add.subsystem.length];
    }
  }
  const scene = new MyScene();
  assert.deepEqual(scene.preload(), ["atlas"]);
  assert.deepEqual(scene.create(), ["add".length]);
});
