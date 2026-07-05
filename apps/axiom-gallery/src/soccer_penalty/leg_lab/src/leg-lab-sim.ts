/*
 * The lab's tick-driven simulation: fold the gait cycle, the hip-bob spring, and
 * the two-bone IK into one leg pose per tick, with the debug readout attached.
 * TypeScript port of the Rust lab's `leg_lab_sim.rs`.
 *
 * `LegLabSim` holds the only mutable state — the hip-bob spring — and advances one
 * tick at a time. Everything else (`gait.ts`) is a closed form of the tick, so a
 * pose is fully determined by the tick count plus the spring history, and the
 * spring is always driven from tick 0. That is what makes it deterministic and
 * replayable: `LegLabSim.atTick` reconstructs any tick's frame from scratch,
 * identically every time.
 */

import { type Vec3, distance, vec3 } from "./vec3.ts";
import {
  type GaitParams,
  type GaitPhase,
  footTargetWorld,
  gaitPhase,
  hipForwardX,
  hipRawHeight,
  isPlanted,
} from "./gait.ts";
import { HipBobSpring } from "./hip-spring.ts";
import { type LegPose, kneeAngle, solveTwoBone } from "./leg-ik.ts";

/** The forward direction the knee bends toward (walking direction) — passed to the IK so the knee can never flip. */
const BEND_FORWARD: Vec3 = vec3(1, 0, 0);

/** One fully-resolved simulation frame: the pose to draw plus the debug read-out. All positions are WORLD-space. */
export interface LegLabFrame {
  readonly tick: number;
  readonly phase: GaitPhase;
  /** Hip (root) position: forward advance in X, spring-smoothed bob in Y. */
  readonly hipWorld: Vec3;
  /** The gait's RAW desired foot position — what the IK aims at, before reach-clamping. */
  readonly rawTargetWorld: Vec3;
  /** The solved leg pose (hip, knee, foot, reachable). */
  readonly pose: LegPose;
}

/** Whether the foot is planted this frame. */
export const framePlanted = (f: LegLabFrame): boolean => isPlanted(f.phase);

/** How far the solved foot missed the raw target (0 when reachable, as in the tuned lab). */
export const targetError = (f: LegLabFrame): number => distance(f.pose.foot, f.rawTargetWorld);

/** A one-line, human-readable debug read-out: gait phase, plant/swing state, raw target vs solved foot. */
export const describe = (f: LegLabFrame): string => {
  const state = f.phase.phase === "planted" ? "PLANTED" : "SWING  ";
  const fmt = (v: Vec3): string => `(${v.x.toFixed(3)},${v.y.toFixed(3)})`;
  return (
    `t=${f.tick} cycle=${f.phase.cycle} phase=${f.phase.fraction.toFixed(2)} ${state} ` +
    `p%=${f.phase.phaseProgress.toFixed(2)} | raw=${fmt(f.rawTargetWorld)} solved=${fmt(f.pose.foot)} ` +
    `err=${targetError(f).toFixed(4)} knee=${((kneeAngle(f.pose) * 180) / Math.PI).toFixed(1)}deg`
  );
};

/** The stateful leg-lab simulation. Construct it, then `step()` once per frame; or use `atTick` for a stateless scrub. */
export class LegLabSim {
  private readonly params: GaitParams;
  private tickCount: number;
  private readonly bob: HipBobSpring;

  /** A fresh simulation at tick 0, hip resting at its standing height. */
  constructor(params: GaitParams) {
    this.params = params;
    this.tickCount = 0;
    this.bob = new HipBobSpring(params.hipHeight);
  }

  /** The current tick. */
  tick(): number {
    return this.tickCount;
  }

  /** Resolve the frame for the CURRENT tick and spring value (does not advance). */
  frame(): LegLabFrame {
    const phase = gaitPhase(this.tickCount, this.params);
    const hipWorld = vec3(hipForwardX(this.tickCount, this.params), this.bob.current(), 0);
    const rawTargetWorld = footTargetWorld(this.tickCount, this.params);
    const pose = solveTwoBone(hipWorld, rawTargetWorld, this.params.thighLength, this.params.shinLength, BEND_FORWARD);
    return { tick: this.tickCount, phase, hipWorld, rawTargetWorld, pose };
  }

  /** Advance one tick: step the hip-bob spring toward the new raw target, then return the resolved frame. */
  step(): LegLabFrame {
    this.tickCount += 1;
    this.bob.step(hipRawHeight(this.tickCount, this.params), this.params.smoothingStrength);
    return this.frame();
  }

  /** Deterministically reconstruct the frame at `tick` from a fresh sim — the scrub entry point. */
  static atTick(params: GaitParams, tick: number): LegLabFrame {
    const sim = new LegLabSim(params);
    for (let i = 0; i < tick; i += 1) {
      sim.step();
    }
    return sim.frame();
  }
}
