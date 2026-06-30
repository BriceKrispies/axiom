import assert from "node:assert/strict";
import { test } from "node:test";

import { mountScene } from "./scene-runtime.ts";
import { Scene } from "./scene.ts";
import { type SimContext, makeSim } from "./sim.ts";
import { TickPump } from "./pump.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";

const contextOf = (): SimContext => {
  const bridge = new FakeBridge();
  return { bridge, fixedHz: 60, pump: new TickPump(bridge, 60) };
};

// A probe scene that records, on each lifecycle hook, the factory set the runtime
// bound onto it — proving `factoriesFromSim` wires the bridge projections, the free
// SOUND surface, and the CAMERAS stub, all scoped to the running tick.
interface Probe {
  readonly phase: string;
  readonly tick: number;
  readonly dt: number;
  readonly add: string;
  readonly input: string;
  readonly physics: string;
  readonly time: string;
  readonly tweens: string;
  readonly sound: string;
  readonly cameras: string;
}

class ProbeScene extends Scene {
  public readonly seen: Probe[] = [];

  public override preload(): readonly string[] {
    return ["atlas", "music"];
  }

  public override create(): readonly number[] {
    this.record("create", -1, -1);
    return [];
  }

  public override update(tick: number, dt: number): readonly number[] {
    this.record("update", tick, dt);
    return [];
  }

  private record(phase: string, tick: number, dt: number): void {
    this.seen.push({
      add: typeof this.add.sprite,
      cameras: this.cameras.subsystem,
      dt,
      input: typeof this.input.isDown,
      phase,
      physics: typeof this.physics.setConfig,
      sound: typeof this.sound.play,
      tick,
      time: typeof this.time.after,
      tweens: typeof this.tweens.add,
    });
  }
}

test("assets() is empty before start has run", () => {
  const mounted = mountScene(new ProbeScene(), contextOf());
  assert.deepEqual(mounted.assets(), []);
});

test("start binds a tick-0 factory set, records preload, and runs create", () => {
  const scene = new ProbeScene();
  const mounted = mountScene(scene, contextOf());
  mounted.start();

  assert.deepEqual(mounted.assets(), ["atlas", "music"]);
  assert.equal(scene.seen.length, 1);
  const [created] = scene.seen;
  assert.ok(created);
  assert.equal(created.phase, "create");
  // Every projected namespace is wired: bridge surfaces, free sound, camera stub.
  assert.equal(created.add, "function");
  assert.equal(created.input, "function");
  assert.equal(created.physics, "function");
  assert.equal(created.time, "function");
  assert.equal(created.tweens, "function");
  assert.equal(created.sound, "function");
  assert.equal(created.cameras, "cameras");
});

test("tick re-scopes the factory set to the running tick and runs update", () => {
  const scene = new ProbeScene();
  const context = contextOf();
  const mounted = mountScene(scene, context);
  mounted.start();
  mounted.tick(makeSim(context, 7));

  assert.equal(scene.seen.length, 2);
  const [, updated] = scene.seen;
  assert.ok(updated);
  assert.equal(updated.phase, "update");
  assert.equal(updated.tick, 7);
  assert.equal(updated.dt, 1 / 60);
  assert.equal(updated.sound, "function");
  assert.equal(updated.cameras, "cameras");
});
