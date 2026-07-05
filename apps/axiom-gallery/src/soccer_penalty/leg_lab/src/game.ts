/*
 * THE LEG LAB — the whole thing, authored in TypeScript on `@axiom/game`.
 *
 * The smallest deterministic scene that proves one procedural leg can move
 * smoothly: a repeating tick-based gait (foot plants to a fixed world point while
 * the hip advances, then swings along a smooth arc to the next contact), a 2-bone
 * IK solve whose knee always bends forward, and a critically-damped hip-bob spring
 * — all in `gait.ts` / `leg-ik.ts` / `hip-spring.ts` / `leg-lab-sim.ts`, ported
 * 1-to-1 from the Rust lab. This file wires that sim to the engine's retained 3D
 * scene and steps it once per fixed tick.
 *
 * The scene is built on the FIRST tick (it needs the host channel, which `boot`
 * binds before the first advance), then each tick advances the gait and moves the
 * leg nodes. `readDebug` exposes the live gait state for the harness's DOM HUD.
 */

import { onFixedUpdate } from "@axiom/game";

import { LegLabSim, describe, framePlanted } from "./leg-lab-sim.ts";
import { kneeAngle } from "./leg-ik.ts";
import { gaitParamsFor, kickerLeg } from "./leg-rig.ts";
import { type LegLabScene, buildScene, setView, updateScene } from "./scene.ts";

const rig = kickerLeg();
const params = gaitParamsFor(rig);

let sim: LegLabSim | undefined;
let scene: LegLabScene | undefined;
let latest = LegLabSim.atTick(params, 0);

onFixedUpdate((): void => {
  if (sim === undefined || scene === undefined) {
    // First tick: build the scene at pose 0 (the host channel is now bound).
    sim = new LegLabSim(params);
    scene = buildScene(rig, sim.frame());
    setView();
    latest = sim.frame();
    return;
  }
  const frame = sim.step();
  updateScene(scene, frame);
  latest = frame;
});

/** The live gait state the harness reads each frame to update the DOM HUD. */
export const readDebug = (): {
  tick: number;
  phase: string;
  planted: boolean;
  kneeDeg: number;
  line: string;
} => ({
  tick: latest.tick,
  phase: latest.phase.phase === "planted" ? "PLANTED" : "SWING",
  planted: framePlanted(latest),
  kneeDeg: Math.round((kneeAngle(latest.pose) * 180) / Math.PI),
  line: describe(latest),
});
