/*
 * The `Sim` handed to every fixed update and the `Frame` handed to every render
 * (SPEC-00 §4.2). `Sim` exposes no wall-clock accessor — elapsed simulated time
 * is `tick * dt`, constant per game. Its members are the real subsystem
 * projections built over the `NativeBridge` / the loop's `TickPump`:
 *   - `rng` (SPEC-01) is the game's root stream;
 *   - `input` (SPEC-05) is bound to the running tick's snapshot;
 *   - `world` (SPEC-02) is the retained ECS surface;
 *   - `add` (SPEC-14) spawns retained game objects;
 *   - `physics` (SPEC-10) attaches bodies and configures the world;
 *   - `time` (SPEC-07) schedules tick-driven timers + state machines;
 *   - `tweens` (SPEC-09) registers tick-sampled tweens.
 *
 * `time`/`tweens` register into the loop-owned `TickPump` so the callbacks they
 * schedule fire/sample on the fixed tick the loop pumps. The durable per-game
 * inputs — the bridge, the fixed rate, and that pump — are grouped into a
 * `SimContext` so `makeSim(context, tick)` separates "what is constant for the
 * game" from "which tick is running".
 */

import { type Add, makeAdd } from "./game-object.ts";
import type { EllipseRadii, EmitterConfig, LineStyle, ShapeStyle } from "./draw2d-binding.ts";
import type { Handle, Rect, Vec2 } from "./vocabulary.ts";
import { type Input, makeInput } from "./input.ts";
import { type Physics, makePhysics } from "./physics.ts";
import { type Rng, makeRng } from "./rng.ts";
import { type Time, makeTime } from "./time.ts";
import { type Tweens, makeTweens } from "./tweens.ts";
import { type World, makeWorld } from "./world.ts";
import type { NativeBridge } from "./native-bridge.ts";
import type { TickPump } from "./pump.ts";
import { boundHost } from "./host-binding.ts";

/** The deterministic simulation view handed to a fixed update. */
export interface Sim {
  /** The monotonic fixed-tick index this update runs at. */
  readonly tick: number;
  /** The constant fixed timestep in seconds (`1 / fixedHz`). */
  readonly dt: number;
  /** The game's root deterministic RNG stream (SPEC-01). */
  readonly rng: Rng;
  /** Input over this tick's snapshot (SPEC-05). */
  readonly input: Input;
  /** The retained ECS world (SPEC-02). */
  readonly world: World;
  /** The retained game-object factory (SPEC-14). */
  readonly add: Add;
  /** Physics bodies + world config (SPEC-10). */
  readonly physics: Physics;
  /** Tick-driven timers + state machines (SPEC-07). */
  readonly time: Time;
  /** Tick-sampled tweens (SPEC-09). */
  readonly tweens: Tweens;
}

/*
 * The presentation view handed to a render — interpolated with `alpha`. Beyond the
 * latest `tick`, it is the author's 2D drawing surface (SPEC-04 §10): an
 * immediate-mode, presentation-class facade whose every verb forwards to the native
 * `axiom-draw2d` builder (through the installed `HostBridge`), so nothing is
 * rasterized in TS. The surface is only legal from `onRender`; it never feeds sim.
 *
 * Today's verbs are those the Wave-2.5 `draw2d*` exports back: the 2D `camera2D`;
 * filled / stroked `rect`, `circle`, and `ellipse`; the self-coloured `line`; the
 * particle system (`createEmitter`/`emit`/`advanceParticles`); and render targets
 * (`createRenderTarget`/`drawTo`/`targetTexture`), plus `finish` to drain the
 * layer-sorted command list. `sprite`/`text`/`measureText`, `path`, and
 * gradient/shadow fills (SPEC-04 §4.2) await their draw2d exports.
 */
export interface Frame {
  /** The latest completed fixed tick this frame presents. */
  readonly tick: number;
  /** Set the 2D camera — world `center` + `zoom` (SPEC-04 §10). */
  readonly camera2D: (view: { center: Vec2; zoom: number }) => void;
  /** Draw a filled / stroked rectangle (SPEC-04 §10). */
  readonly rect: (bounds: Rect, style: ShapeStyle) => void;
  /** Draw a filled / stroked circle centred at `center` (SPEC-04 §10). */
  readonly circle: (center: Vec2, radius: number, style: ShapeStyle) => void;
  /** Draw a filled / stroked (optionally rotated) ellipse (SPEC-04 §10). */
  readonly ellipse: (center: Vec2, radii: EllipseRadii, style: ShapeStyle) => void;
  /** Draw a straight line segment of its own colour + width (SPEC-04 §10). */
  readonly line: (from: Vec2, to: Vec2, style: LineStyle) => void;
  /** Register a particle emitter, returning its handle (SPEC-04 §10.1). */
  readonly createEmitter: (config: EmitterConfig) => Handle;
  /** Spawn a particle burst from `id` at `at` flying along `direction` (SPEC-04 §10.1). */
  readonly emit: (id: Handle, at: Vec2, direction: Vec2) => void;
  /** Step live particles by the presentation delta and append their quads (SPEC-04 §10.1). */
  readonly advanceParticles: (dtSeconds: number) => void;
  /** Create an off-screen render target, returning its handle (SPEC-04 §10.3). */
  readonly createRenderTarget: (width: number, height: number) => Handle;
  /** Route the draws made inside `draw` into `target` (SPEC-04 §10.3). */
  readonly drawTo: (target: Handle, draw: (frame: Frame) => void) => void;
  /** The texture handle naming `target`'s off-screen surface (SPEC-04 §10.3). */
  readonly targetTexture: (target: Handle) => Handle;
  /** Finish the frame: the layer-sorted neutral command list `[kind, layer, submission, …]`. */
  readonly finish: () => readonly number[];
}

/** One second expressed in seconds — the numerator of `dt = 1 second / fixedHz`. */
const ONE_SECOND_IN_SECONDS = 1;

/** The durable per-game inputs every per-tick `Sim` is built from. */
export interface SimContext {
  /** The native fixed-step runtime (RNG / ECS / input / bodies). */
  readonly bridge: NativeBridge;
  /** The fixed simulation rate, so `dt = 1 / fixedHz`. */
  readonly fixedHz: number;
  /** The loop-owned per-tick pump backing `time` / `tweens`. */
  readonly pump: TickPump;
}

/** Build the deterministic `Sim` for `tick` from the game's `context`. */
export const makeSim = (context: SimContext, tick: number): Sim => ({
  add: makeAdd(context.bridge),
  dt: ONE_SECOND_IN_SECONDS / context.fixedHz,
  input: makeInput(context.bridge, tick),
  physics: makePhysics(context.bridge),
  rng: makeRng(context.bridge),
  tick,
  time: makeTime(context.pump, tick),
  tweens: makeTweens(context.pump, tick),
  world: makeWorld(context.bridge),
});

/*
 * Build the presentation `Frame` for the latest completed `tick`. The 2D draw
 * verbs read the installed `HostBridge` at call time (the presentation channel
 * `bindNative` installs), exactly as the free `sound`/`scene3d` surfaces do, so
 * `makeFrame` needs no bridge argument. `drawTo` brackets the author's draws with
 * `beginTarget`/`endTarget` and hands them the same per-tick surface.
 */
export const makeFrame = (tick: number): Frame => ({
  advanceParticles: (dtSeconds: number): void => {
    boundHost().draw2dAdvanceParticles(dtSeconds);
  },
  camera2D: (view: { center: Vec2; zoom: number }): void => {
    boundHost().draw2dCamera2d(view.center, view.zoom);
  },
  circle: (center: Vec2, radius: number, style: ShapeStyle): void => {
    boundHost().draw2dCircle(center, radius, style);
  },
  createEmitter: (config: EmitterConfig): Handle => boundHost().draw2dCreateEmitter(config),
  createRenderTarget: (width: number, height: number): Handle => boundHost().draw2dCreateRenderTarget(width, height),
  drawTo: (target: Handle, draw: (frame: Frame) => void): void => {
    boundHost().draw2dBeginTarget(target);
    draw(makeFrame(tick));
    boundHost().draw2dEndTarget();
  },
  ellipse: (center: Vec2, radii: EllipseRadii, style: ShapeStyle): void => {
    boundHost().draw2dEllipse(center, radii, style);
  },
  emit: (id: Handle, at: Vec2, direction: Vec2): void => {
    boundHost().draw2dEmit(id, at, direction);
  },
  finish: (): readonly number[] => boundHost().draw2dFinish(),
  line: (from: Vec2, to: Vec2, style: LineStyle): void => {
    boundHost().draw2dLine(from, to, style);
  },
  rect: (bounds: Rect, style: ShapeStyle): void => {
    boundHost().draw2dRect(bounds, style);
  },
  targetTexture: (target: Handle): Handle => boundHost().draw2dTargetTexture(target),
  tick,
});
