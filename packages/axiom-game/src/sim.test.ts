import assert from "node:assert/strict";
import { test } from "node:test";

import { type SimContext, makeFrame, makeSim } from "./sim.ts";
import type { EmitterConfig, GradientStop, LineStyle, PathStyle, ShapeStyle, SpriteOpts, TextOpts } from "./draw2d-binding.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";
import { FakeHost } from "./fake-host.testkit.ts";
import { ROOT_STREAM } from "./rng.ts";
import { TickPump } from "./pump.ts";
import { bindNative } from "./host-binding.ts";

const FIXED_HZ = 60;
const TICK = 7;

// One context over a fresh fake bridge + pump, the way GameLoop builds a per-tick Sim.
const makeContext = (): { bridge: FakeBridge; pump: TickPump; context: SimContext } => {
  const bridge = new FakeBridge();
  const pump = new TickPump(bridge, FIXED_HZ);
  return { bridge, context: { bridge, fixedHz: FIXED_HZ, pump }, pump };
};

test("makeSim carries the running tick and the constant fixed timestep", () => {
  const { context } = makeContext();
  const sim = makeSim(context, TICK);
  assert.equal(sim.tick, TICK);
  assert.equal(sim.dt, 1 / FIXED_HZ);
});

test("sim.rng is the root stream projection over the bridge", () => {
  const { bridge, context } = makeContext();
  bridge.units = [0.5];
  const sim = makeSim(context, TICK);
  assert.equal(sim.rng.next(), 0.5);
  assert.equal(bridge.lastUnitStream, ROOT_STREAM);
});

test("sim.input binds this tick's snapshot", () => {
  const { bridge, context } = makeContext();
  bridge.down.add(`${TICK}|fire`);
  const sim = makeSim(context, TICK);
  assert.equal(sim.input.isDown("fire"), true);
  // A different tick's snapshot is not this input's.
  assert.equal(makeSim(context, TICK + 1).input.isDown("fire"), false);
});

test("sim.world projects the retained ECS store", () => {
  const { context } = makeContext();
  const sim = makeSim(context, TICK);
  const entity = sim.world.spawn({ kind: "node" });
  assert.deepEqual(sim.world.query("node"), [entity]);
});

test("sim.add spawns retained game objects into the world", () => {
  const { bridge, context } = makeContext();
  const sim = makeSim(context, TICK);
  const sprite = sim.add.sprite("hero", 3, 4);
  assert.equal(sprite.x, 3);
  assert.equal(sprite.y, 4);
  assert.deepEqual(bridge.worldQuery(["Sprite"]), [sprite.entity]);
});

test("sim.physics configures the world and attaches bodies", () => {
  const { bridge, context } = makeContext();
  const sim = makeSim(context, TICK);
  sim.physics.setConfig({ angularDamping: 0.2, gravity: { x: 0, y: -9.8, z: 0 }, linearDamping: 0.1 });
  assert.deepEqual(bridge.config, [0, -9.8, 0, 0.1, 0.2]);
  const body = sim.physics.add.dynamic(sim.add.sprite("a", 0, 0));
  assert.deepEqual(
    bridge.bodies.map(([, kind]) => kind),
    ["dynamic"],
  );
  assert.equal(body.handle, 1);
});

test("sim.time schedules tick-driven timers into the loop pump", () => {
  const { context, pump } = makeContext();
  const sim = makeSim(context, TICK);
  let fired = 0;
  sim.time.after(2, (): void => {
    fired += 1;
  });
  // Registered at tick 7 with delay 2 -> fires on tick 9 when the pump runs it.
  pump.pump(TICK + 1);
  assert.equal(fired, 0);
  pump.pump(TICK + 2);
  assert.equal(fired, 1);
});

test("sim.tweens registers tick-sampled tweens into the loop pump", () => {
  const { context, pump } = makeContext();
  const sim = makeSim(context, TICK);
  const samples: number[] = [];
  const id = sim.tweens.add({
    duration: 1 / FIXED_HZ,
    from: 0,
    onUpdate: (value): void => {
      samples.push(value);
    },
    to: 10,
  });
  assert.equal(typeof id, "number");
  // One-tick tween registered at tick 7 -> samples/completes on tick 8.
  pump.pump(TICK + 1);
  assert.deepEqual(samples, [10]);
});

test("makeFrame presents the latest tick and forwards every draw verb to the bound host", () => {
  const host = new FakeHost();
  bindNative(host);
  const frame = makeFrame(3);
  assert.equal(frame.tick, 3);

  const fill: ShapeStyle = { fill: [1, 0, 0, 1] };
  const lineStyle: LineStyle = { color: [0, 1, 0, 1], width: 2 };
  const emitterConfig: EmitterConfig = {
    colorEnd: [0, 0, 0, 0],
    colorStart: [1, 1, 1, 1],
    count: 4,
    lifetimeSeconds: 1,
    size: 2,
    speed: 5,
    spread: 0.5,
  };

  frame.camera2D({ center: { x: 1, y: 2 }, zoom: 4 });
  frame.rect({ height: 8, width: 6, x: 0, y: 0 }, fill);
  frame.circle({ x: 5, y: 6 }, 3, fill);
  frame.ellipse({ x: 7, y: 8 }, { rx: 2, ry: 4 }, fill);
  frame.line({ x: 0, y: 0 }, { x: 9, y: 9 }, lineStyle);
  const emitter = frame.createEmitter(emitterConfig);
  frame.emit(emitter, { x: 1, y: 1 }, { x: 0, y: 1 });
  frame.advanceParticles(0.016);

  assert.deepEqual(host.draw2dCameras, [{ center: { x: 1, y: 2 }, zoom: 4 }]);
  assert.deepEqual(host.draw2dRects, [{ bounds: { height: 8, width: 6, x: 0, y: 0 }, style: fill }]);
  assert.deepEqual(host.draw2dCircles, [{ center: { x: 5, y: 6 }, radius: 3, style: fill }]);
  assert.deepEqual(host.draw2dEllipses, [{ center: { x: 7, y: 8 }, radii: { rx: 2, ry: 4 }, style: fill }]);
  assert.deepEqual(host.draw2dLines, [{ from: { x: 0, y: 0 }, style: lineStyle, to: { x: 9, y: 9 } }]);
  assert.deepEqual(host.draw2dEmitters, [emitterConfig]);
  assert.deepEqual(host.draw2dEmits, [{ at: { x: 1, y: 1 }, direction: { x: 0, y: 1 }, id: emitter }]);
  assert.deepEqual(host.draw2dAdvances, [0.016]);
});

test("makeFrame forwards path + gradient verbs to the bound host", () => {
  const host = new FakeHost();
  bindNative(host);
  const frame = makeFrame(9);

  const pathStyle: PathStyle = { closed: true, fill: [0, 0, 1, 1] };
  const stops: readonly GradientStop[] = [
    { color: [0, 0, 0, 1], offset: 0 },
    { color: [1, 1, 1, 1], offset: 1 },
  ];
  const points = [{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 1 }];

  frame.path(points, pathStyle);
  const lin = frame.linearGradient({ x: 0, y: 0 }, { x: 4, y: 0 }, stops);
  const rad = frame.radialGradient({ x: 2, y: 2 }, 3, stops);

  assert.deepEqual(host.draw2dPaths, [{ points, style: pathStyle }]);
  assert.deepEqual(host.draw2dLinearGradients, [{ from: { x: 0, y: 0 }, stops, to: { x: 4, y: 0 } }]);
  assert.deepEqual(host.draw2dRadialGradients, [{ center: { x: 2, y: 2 }, radius: 3, stops }]);
  // Each registered paint returns a distinct, non-zero handle from the bound host.
  assert.notEqual(lin, rad);
});

test("makeFrame forwards sprite / text / measureText to the bound host", () => {
  const host = new FakeHost();
  host.measureTextReturn = { height: 16, width: 40 };
  bindNative(host);
  const frame = makeFrame(5);

  const spriteOpts: SpriteOpts = { pos: { x: 10, y: 20 }, scale: { x: 2, y: 2 } };
  const textOpts: TextOpts = { color: [1, 1, 1, 1], font: { family: "monospace", size: 16 }, pos: { x: 1, y: 2 } };
  frame.sprite(7, spriteOpts);
  frame.text("HP", textOpts);
  const metrics = frame.measureText("HP", { family: "monospace", size: 16 });

  assert.deepEqual(host.draw2dSpriteCalls, [{ opts: spriteOpts, texture: 7 }]);
  assert.deepEqual(host.draw2dTextCalls, [{ opts: textOpts, value: "HP" }]);
  assert.deepEqual(host.measureTextCalls, [{ font: { family: "monospace", size: 16 }, value: "HP" }]);
  assert.deepEqual(metrics, { height: 16, width: 40 });
});

test("makeFrame brackets a render target and reports the finished command list", () => {
  const host = new FakeHost();
  host.draw2dFinishReturn = [1, 0, 42];
  bindNative(host);
  const frame = makeFrame(11);

  const target = frame.createRenderTarget(64, 32);
  assert.deepEqual(host.draw2dTargets, [{ height: 32, width: 64 }]);

  let innerTick = -1;
  frame.drawTo(target, (inner): void => {
    innerTick = inner.tick;
    inner.advanceParticles(0.5);
  });
  // drawTo brackets the author's draws with begin/end and hands them the same tick.
  assert.equal(innerTick, 11);
  assert.deepEqual(host.draw2dBegins, [target]);
  assert.equal(host.draw2dEnds, 1);
  assert.deepEqual(host.draw2dAdvances, [0.5]);

  assert.equal(frame.targetTexture(target), target);
  assert.deepEqual(frame.finish(), [1, 0, 42]);
});
