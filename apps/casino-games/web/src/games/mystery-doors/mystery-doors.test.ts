/*
 * mystery-doors.test.ts — the reveal-ordering and fairness proofs for Mystery
 * Doors:
 *
 *  1. REVEAL ORDERING — the door cracks a few degrees (swing > 0) and the
 *     colored light spills through the gap BEFORE the door swings wide; the
 *     spill is zero before the crack begins, and the full swing only arrives
 *     after the pause. All from a pure function of reveal age.
 *  2. AMBIENT-ONLY RATTLE — the idle rattle pose is a pure function of the
 *     AMBIENT (root) seed alone. It takes no presentation seed by construction,
 *     so a round's committed outcome can never move a door in idle; two draws
 *     with the same ambient seed are byte-equal, and liveliness 0 is still.
 *
 * Runs under bare `node --test` (no DOM); casino-mount.ts is never imported.
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { PresentationSettings } from "../../chance-engine/registry/definition.ts";
import type { InputFrame, TickContext } from "@axiom/web-engine";
import type { CasinoMountSpec, RoundEnvironment } from "../round-state.ts";
import { foldRoundTick, freshRoundState } from "../round-state.ts";
import type { DoorsExtra, DoorsSpec } from "./game.ts";
import {
  DOOR_CRACK,
  DOOR_MAX_SWING,
  doorDance,
  doorOpenPose,
  doorTimeline,
  initialDoorsExtra,
  revealAgeOf,
  stepDoors,
} from "./game.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: false,
  highContrast: false,
  masterVolume: 0,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 0,
};

const config = (): CasinoGameConfig<DoorsSpec> =>
  baseConfig("mystery-doors", "Mystery Doors", "showcase", { breatheLiveliness: 0.8 }, { choiceCount: 3, targetWinRate: 0.42 });

const emptyInput = (): InputFrame => ({
  down: new Set(),
  look: { x: 0, y: 0 },
  pointer: undefined,
  pressed: new Set(),
  released: new Set(),
});

const withPress = (code: string): InputFrame => ({ ...emptyInput(), pressed: new Set([code]) });

const rig = (seed: number, round: number): { readonly env: RoundEnvironment; readonly spec: CasinoMountSpec<DoorsExtra> } => {
  const cfg = config();
  const source = new SeededChanceResultSource(seed);
  const runtime = { config: cfg, onHud: (): void => {}, round, seed, settings: SETTINGS, source };
  const env: RoundEnvironment = { config: cfg, seed, settings: SETTINGS, source };
  const spec: CasinoMountSpec<DoorsExtra> = {
    initExtra: initialDoorsExtra,
    mechanic: { choiceCount: 3, kind: "choice" },
    resources: { materials: {}, meshes: {} },
    step: (state, input, ctx) => stepDoors(runtime, state, input, ctx),
    viewScene: () => ({ camera: { far: 1, fovY: 1, near: 0.1, position: { x: 0, y: 0, z: 0 }, target: { x: 0, y: 0, z: 0 } }, instances: [], lights: [] }),
  };
  return { env, spec };
};

test("crack-and-spill strictly precede the full swing", () => {
  const tl = doorTimeline(1, false);

  // Before the crack window, nothing has opened and no light spills.
  const closed = doorOpenPose(tl.knobEnd, tl);
  assert.equal(closed.swing, 0);
  assert.equal(closed.spill, 0);

  // Mid-crack: the door is ajar (swing > 0), light spills, but the door is not
  // yet near its full swing.
  const midCrack = doorOpenPose(Math.round((tl.knobEnd + tl.crackEnd) / 2), tl);
  assert.ok(midCrack.swing > 0, "the door has cracked open");
  assert.ok(midCrack.spill > 0, "light spills as it cracks");
  assert.ok(midCrack.swing <= DOOR_CRACK + 1e-9, "still only cracked, not swung");
  assert.ok(midCrack.swing < DOOR_MAX_SWING - 0.5, "far from the full swing");

  // Through the pause the door holds at the crack; the wide swing is later.
  const atPause = doorOpenPose(tl.pauseEnd, tl);
  assert.ok(Math.abs(atPause.swing - DOOR_CRACK) < 1e-9, "held at the crack through the pause");
  assert.equal(atPause.spill, 1, "the spill has fully opened by the pause");

  // The full swing arrives by the end of the swing window.
  const wide = doorOpenPose(tl.swingEnd, tl);
  assert.ok(Math.abs(wide.swing - DOOR_MAX_SWING) < 1e-6, "the door reaches its full swing");
  assert.ok(wide.swing > atPause.swing, "the swing opens strictly beyond the crack");
});

test("idle rattle is a pure function of the ambient seed alone", () => {
  // Pure and deterministic for a fixed (index, tick, ambient seed).
  const a = doorDance(1, 3, 57, 0xa11ce, 1);
  const b = doorDance(1, 3, 57, 0xa11ce, 1);
  assert.deepEqual(a, b);

  // Liveliness 0 freezes every door.
  assert.deepEqual(doorDance(1, 3, 57, 0xa11ce, 0), { rattle: 0, sway: 0 });

  // A rattle genuinely occurs somewhere in the ambient timeline (the effect is
  // real, not a constant zero) — and it is driven only by the ambient seed.
  let sawRattle = false;
  for (let tick = 0; tick < 500; tick += 1) {
    for (let index = 0; index < 3; index += 1) {
      if (Math.abs(doorDance(index, 3, tick, 0xa11ce, 1).rattle) > 1e-4) {
        sawRattle = true;
      }
    }
  }
  assert.ok(sawRattle, "an idle rattle occurs across the ambient timeline");

  // A different ambient seed yields a different idle timeline (independence),
  // while doorDance has no presentation-seed parameter at all — so a round's
  // outcome cannot perturb the idle pose.
  const seedA = doorDance(0, 3, 40, 1, 1);
  const seedB = doorDance(0, 3, 40, 2, 1);
  assert.notDeepEqual(seedA, seedB);
});

test("a driven round only swings the door wide after commitment", () => {
  const { env, spec } = rig(0xd00d5, 2);
  const tl = doorTimeline(1, false);
  let state = freshRoundState(env, spec, 2, false);
  let tick = 0;
  const advance = (input: InputFrame): void => {
    tick += 1;
    const ctx: TickContext = { dt: 1 / 60, tick };
    state = foldRoundTick(env, spec, state, input, ctx);
  };

  let guard = 0;
  while (state.session.phase === "intro" && guard < 200) {
    advance(emptyInput());
    guard += 1;
  }
  advance(withPress("primary"));

  guard = 0;
  while (state.session.phase !== "complete" && guard < 3000) {
    const pose = doorOpenPose(revealAgeOf(state.session, tl.total), tl);
    if (pose.swing > DOOR_CRACK + 1e-6) {
      assert.equal(pose.spill, 1, "the door only swings wide once the crack has fully spilled light");
      assert.ok(state.session.committed !== null, "no wide swing without a committed outcome");
    }
    advance(emptyInput());
    guard += 1;
  }
  assert.equal(state.session.phase, "complete");
});
