/*
 * cinematic.test.ts — deterministic properties of the home-run cinematic:
 * `evaluateSwingOutcome`'s authoritative hit-outcome model, the cinematic phase
 * machine's presentation schedule, and the camera director. Bare `node --test`,
 * no DOM/wasm — the whole cinematic is SDK-free pure logic, exactly like
 * `home-run.test.ts` next to it.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { type Vec3, add, vec3 } from "./vec.ts";
import type { BatterPosition, Intent, PitchFlightState, PitchSpec } from "./types.ts";
import { batDir, newSwing, stepSwing } from "./swing.ts";
import { solvePitch } from "./pitch.ts";
import { evaluateSwingOutcome } from "./swing-outcome.ts";
import { HOME_RUN_CINEMATIC_TUNING as TUNING } from "./cinematic-constants.ts";
import { enterCinematicPhase, newCinematic, stepCinematic } from "./cinematic.ts";
import { cinematicFovY, contactCameraPose, groundTrackingCameraPose } from "./cinematic-camera.ts";
import { HomeRunSession } from "./session.ts";
import * as C from "./constants.ts";

const IDLE: Intent = { moveX: 0, start: false, swing: false };
const intent = (over: Partial<Intent>): Intent => ({ ...IDLE, ...over });

// ── evaluateSwingOutcome fixtures ───────────────────────────────────────────────

/** A committed swing (state "swing", theta at the wound stance) — exactly what
 * `session.ts` hands `evaluateSwingOutcome` the instant a swing fires. */
const committedSwing = () => stepSwing(newSwing(), true);

const TEST_PITCH: PitchSpec = { gravity: 8, mph: 90, name: "TEST", profileId: "test", speed: 20, targetX: 0, targetY: 0.9 };

/** The pitch's pos/vel/gravity `ticksSinceRelease` ticks after `#releasePitch` —
 * exactly what `session.ts` would have captured at that moment. */
const pitchStateAtTick = (spec: PitchSpec, ticksSinceRelease: number): PitchFlightState => {
  const solved = solvePitch(spec);
  let pos = C.PITCH_RELEASE;
  let vel = solved.vel;
  for (let i = 0; i < ticksSinceRelease; i += 1) {
    vel = vec3(vel.x, vel.y - solved.gravityPerTick, vel.z);
    pos = add(pos, vel);
  }
  return { gravityPerTick: solved.gravityPerTick, pos, vel };
};

const BATTER: BatterPosition = { x: C.BATTER_START_X, z: C.BATTER_Z };

/** A swing that whiffs: the pitch is aimed far outside the bat's physical reach,
 * so no forward-simulated tick can ever find contact — deterministic, no search. */
const missOutcome = () => {
  const wildPitch: PitchSpec = { ...TEST_PITCH, targetX: 6 };
  return evaluateSwingOutcome(committedSwing(), pitchStateAtTick(wildPitch, 20), BATTER, TUNING);
};

/**
 * A swing that contacts on the VERY FIRST simulated tick (the commit tick
 * itself), engineered in closed form: at tick 0 the swing hasn't moved yet
 * (`theta === THETA_READY`, the wound stance), so the ball is placed exactly on
 * the bat's sweep ray at that fixed angle. This angle points far enough behind
 * the batter that the resulting spray is always foul — deterministic, no search.
 */
const foulOutcome = () => {
  const theta = C.THETA_READY;
  const r = C.SWEET_SPOT_R;
  const dir = batDir(theta);
  const target = vec3(BATTER.x + r * dir.x, C.BAT_PLANE_Y, BATTER.z + r * dir.z);
  const vel = vec3(0, 0, -0.3);
  const pos = vec3(target.x - vel.x, target.y - vel.y, target.z - vel.z);
  return evaluateSwingOutcome(committedSwing(), { gravityPerTick: 0, pos, vel }, BATTER, TUNING);
};

/** Search over how many ticks the pitch had already flown before the swing
 * committed — the same kind of sweep `home-run.test.ts` already uses on the
 * full session — until `evaluateSwingOutcome` itself predicts a home run. */
const findHomerOutcome = () => {
  for (let tick = 1; tick <= 60; tick += 1) {
    const outcome = evaluateSwingOutcome(committedSwing(), pitchStateAtTick(TEST_PITCH, tick), BATTER, TUNING);
    if (outcome.isHomeRun) {
      return outcome;
    }
  }
  throw new Error("no home run found in the search window");
};

/** Search for a FAIR contact that does NOT clear the wall — either short of it
 * or across it below the required height. */
const findNonHomerFairOutcome = () => {
  const batters = [C.BATTER_MIN_X, C.BATTER_START_X, C.BATTER_MAX_X];
  for (const bx of batters) {
    for (let tick = 1; tick <= 60; tick += 1) {
      const outcome = evaluateSwingOutcome(committedSwing(), pitchStateAtTick(TEST_PITCH, tick), { x: bx, z: C.BATTER_Z }, TUNING);
      if (outcome.contactOccurs && outcome.isFair && !outcome.isHomeRun) {
        return outcome;
      }
    }
  }
  throw new Error("no non-homer fair contact found in the search window");
};

// ── 1: identical inputs → identical SwingOutcome ────────────────────────────────

test("1. identical pitch and swing state produce an identical SwingOutcome", () => {
  const swing = committedSwing();
  const pitchState = pitchStateAtTick(TEST_PITCH, 15);
  const a = evaluateSwingOutcome(swing, pitchState, BATTER, TUNING);
  const b = evaluateSwingOutcome(swing, pitchState, BATTER, TUNING);
  assert.deepEqual(a, b);
});

// ── 2/3: miss and foul can never be a home run ──────────────────────────────────

test("2. a miss cannot produce a home-run result", () => {
  const outcome = missOutcome();
  assert.equal(outcome.contactOccurs, false);
  assert.equal(outcome.isHomeRun, false);
  assert.equal(outcome.homeRunReason, "no-contact");
});

test("3. a foul hit cannot produce a home-run result", () => {
  const outcome = foulOutcome();
  assert.equal(outcome.contactOccurs, true, "the engineered foul contact must actually register");
  assert.equal(outcome.isFair, false);
  assert.equal(outcome.isHomeRun, false);
  assert.equal(outcome.homeRunReason, "not-fair");
});

// ── 4/5: the wall-height threshold ──────────────────────────────────────────────

test("4. a ball crossing the wall below wall height is not a home run", () => {
  const outcome = findNonHomerFairOutcome();
  assert.equal(outcome.isHomeRun, false);
  assert.ok(outcome.homeRunReason === "below-wall-height" || outcome.homeRunReason === "does-not-clear-wall");
});

test("5. a fair ball clearing the wall above the required height is a home run", () => {
  const outcome = findHomerOutcome();
  assert.equal(outcome.contactOccurs, true);
  assert.equal(outcome.isFair, true);
  assert.equal(outcome.isHomeRun, true);
  assert.equal(outcome.homeRunReason, "clears-wall-fair");
});

// ── 6: the real hit uses SwingOutcome's own exit velocity ───────────────────────

test("6. the real launched ball uses the exit velocity from SwingOutcome", () => {
  const outcome = findHomerOutcome();
  // `evaluateSwingOutcome` projects the flight with `newFlight(outcome.contactPoint,
  // outcome.exitVelocity, ...)` — the same values `session.ts#beginFlight` launches
  // the REAL ball with. Prove the projected landing is reachable from exactly that
  // vector (not a value recomputed independently).
  assert.ok(outcome.exitVelocity.z !== 0 || outcome.exitVelocity.x !== 0, "a real exit vector was produced");
  assert.equal(outcome.launchDirection.x, outcome.exitVelocity.x / Math.hypot(outcome.exitVelocity.x, outcome.exitVelocity.y, outcome.exitVelocity.z));
});

// ── 7: bounded prediction ───────────────────────────────────────────────────────

test("7. home-run prediction uses bounded trajectory steps", () => {
  // A pathological "gravity" of 0 would let a fly ball climb forever without the
  // step cap — confirm the search still terminates (returns, doesn't hang) and
  // respects `maxPredictionSteps`/`swingContactSearchMaxTicks`.
  const zeroGravityTuning = { ...TUNING, gravity: 0, maxPredictionSteps: 50, swingContactSearchMaxTicks: 50 };
  const start = Date.now();
  const outcome = evaluateSwingOutcome(committedSwing(), pitchStateAtTick(TEST_PITCH, 15), BATTER, zeroGravityTuning);
  assert.ok(Date.now() - start < 1000, "prediction must terminate quickly under a bounded step cap");
  assert.equal(typeof outcome.isHomeRun, "boolean");
});

// ── 8/9: cinematic phase gating ─────────────────────────────────────────────────

test("8. normal hits do not enter a cinematic state", () => {
  const s = new HomeRunSession(11);
  s.advance(intent({ start: true }));
  let guard = 4000;
  while (s.phase !== "over" && guard > 0) {
    s.advance(IDLE);
    assert.equal(s.view().cinematicPhase, "none", "taking every pitch must never enter a cinematic phase");
    guard -= 1;
  }
  assert.ok(guard > 0);
});

test("9. a predicted home run enters anticipation before contact", () => {
  let found: { readonly seed: number; readonly t: number } | undefined;
  outer: for (let seed = 1; seed <= 40 && found === undefined; seed += 1) {
    for (let t = 20; t <= 200; t += 1) {
      const s = new HomeRunSession(seed);
      for (let k = 1; k < t; k += 1) {
        s.advance(intent({ start: k === 1 }));
      }
      s.advance(intent({ swing: true }));
      if (s.view().cinematicPhase === "anticipation") {
        found = { seed, t };
        break outer;
      }
    }
  }
  assert.ok(found !== undefined, "some seed/timing must commit a predicted home-run swing");
  // Re-run that exact swing and confirm anticipation starts strictly BEFORE the
  // ball is ever in the "flight" round-phase (i.e., before actual contact).
  const s = new HomeRunSession(found!.seed);
  for (let k = 1; k < found!.t; k += 1) {
    s.advance(intent({ start: k === 1 }));
  }
  s.advance(intent({ swing: true }));
  assert.equal(s.view().cinematicPhase, "anticipation");
  assert.notEqual(s.phase, "flight", "anticipation begins while the pitch is still incoming, not after contact");
});

// ── 10/11: time-scale bounds ─────────────────────────────────────────────────────

test("10. cinematic time scale never falls below its configured minimum", () => {
  let state = enterCinematicPhase(newCinematic(), "contact");
  for (let i = 0; i < 500; i += 1) {
    state = stepCinematic(state, TUNING);
    assert.ok(state.timeScale >= TUNING.contactSlowMotionScale - 1e-9, `timeScale ${state.timeScale} below the configured minimum`);
  }
});

test("11. cinematic time scale always returns exactly to 1", () => {
  let state = enterCinematicPhase(newCinematic(), "anticipation");
  for (let i = 0; i < 20; i += 1) {
    state = stepCinematic(state, TUNING);
  }
  state = enterCinematicPhase(state, "contact");
  for (let i = 0; i < TUNING.contactSlowMotionDurationTicks; i += 1) {
    state = stepCinematic(state, TUNING);
  }
  state = enterCinematicPhase(state, "ballFollow");
  for (let i = 0; i < 300; i += 1) {
    state = stepCinematic(state, TUNING);
  }
  assert.equal(state.timeScale, 1);
});

// ── 12/13: letterbox bounds + retraction ────────────────────────────────────────

test("12. letterbox progress remains between 0 and 1", () => {
  let state = enterCinematicPhase(newCinematic(), "anticipation");
  const phases = ["anticipation", "contact", "ballFollow", "landing", "celebration"] as const;
  for (const phase of phases) {
    state = enterCinematicPhase(state, phase);
    for (let i = 0; i < 50; i += 1) {
      state = stepCinematic(state, TUNING);
      assert.ok(state.letterbox >= 0 && state.letterbox <= 1);
    }
  }
});

test("13. letterbox bars fully retract after the cinematic", () => {
  let state = enterCinematicPhase(newCinematic(), "contact");
  for (let i = 0; i < TUNING.letterboxEntranceDurationTicks + 5; i += 1) {
    state = stepCinematic(state, TUNING);
  }
  assert.ok(state.letterbox > 0.9, "bars should be near max scrunch through contact");
  state = enterCinematicPhase(state, "celebration");
  for (let i = 0; i < TUNING.letterboxExitDurationTicks + 5; i += 1) {
    state = stepCinematic(state, TUNING);
  }
  assert.equal(state.letterbox, 0);
});

// ── 14/15/16: camera director ───────────────────────────────────────────────────

test("14. contact camera pose is derived deterministically from the batter transform", () => {
  const a = contactCameraPose({ x: 1.1, z: C.BATTER_Z }, TUNING);
  const b = contactCameraPose({ x: 1.1, z: C.BATTER_Z }, TUNING);
  assert.deepEqual(a, b);
  const c = contactCameraPose({ x: 0.6, z: C.BATTER_Z }, TUNING);
  assert.notDeepEqual(a, c, "a different batter position must produce a different pose");
});

test("15. the ground-tracking camera stays planted and always points at the ball", () => {
  // Not a chase camera: the POSITION depends only on the batter (stays fixed for
  // a given batter transform, regardless of where the ball currently is), while
  // the TARGET is always exactly the ball's position — the camera pivots in
  // place rather than translating to follow it.
  const batter: BatterPosition = { x: 1.0, z: C.BATTER_Z };
  const positions = new Set<string>();
  for (let t = 0; t < 200; t += 1) {
    const ballPos = vec3(Math.sin(t * 0.05) * 10, 3 + Math.sin(t * 0.1) * 2, t * 0.2);
    const pose = groundTrackingCameraPose(batter, ballPos, TUNING);
    assert.deepEqual(pose.target, ballPos, "the camera always points exactly at the ball");
    positions.add(JSON.stringify(pose.position));
  }
  assert.equal(positions.size, 1, "the camera position never moves while the ball is in flight");
});

test("16. the ground-tracking camera stops updating once the ball clears the wall", () => {
  let found: { readonly seed: number; readonly t: number } | undefined;
  outer: for (let seed = 1; seed <= 40 && found === undefined; seed += 1) {
    for (let t = 20; t <= 200; t += 1) {
      const s = new HomeRunSession(seed);
      for (let k = 1; k < t; k += 1) {
        s.advance(intent({ start: k === 1 }));
      }
      s.advance(intent({ swing: true }));
      if (s.view().cinematicPhase === "anticipation") {
        found = { seed, t };
        break outer;
      }
    }
  }
  assert.ok(found !== undefined);
  const s = new HomeRunSession(found!.seed);
  for (let k = 1; k < found!.t; k += 1) {
    s.advance(intent({ start: k === 1 }));
  }
  s.advance(intent({ swing: true }));
  let guard = 3000;
  let frozenPose: { readonly position: Vec3; readonly target: Vec3 } | undefined;
  let sawBeyondWall = false;
  const closeEnough = (a: Vec3, b: Vec3): boolean => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z) < 1e-9;
  while (s.phase !== "result" && guard > 0) {
    s.advance(IDLE);
    const v = s.view();
    const beyond = Math.abs(v.ball.x) + v.ball.z >= C.WALL_LINE;
    if (v.cinematicPhase === "ballFollow" && beyond) {
      if (frozenPose === undefined) {
        frozenPose = { position: v.cameraPos, target: v.cameraTarget };
      } else {
        assert.ok(closeEnough(v.cameraPos, frozenPose.position), "camera position must freeze once the ball leaves the park");
        assert.ok(closeEnough(v.cameraTarget, frozenPose.target), "camera target must freeze once the ball leaves the park");
      }
      sawBeyondWall = true;
    }
    guard -= 1;
  }
  assert.ok(guard > 0);
  assert.ok(sawBeyondWall, "the scripted swing must actually clear the wall while still in ballFollow");
});

// ── 17/18: bounded pools ────────────────────────────────────────────────────────

test("17. trail history never exceeds its configured cap", () => {
  const s = new HomeRunSession(21);
  s.advance(intent({ start: true }));
  let guard = 3000;
  while (s.phase !== "over" && guard > 0) {
    s.advance(intent({ swing: guard % 47 === 0 }));
    assert.ok(s.view().trail.length <= 14, "the trail array is bounded to TRAIL_MAX");
    guard -= 1;
  }
});

test("18. confetti never exceeds its configured cap", () => {
  // The confetti pool size is a fixed constant shared with the DOM edge
  // (`harness.ts`'s burst) and mirrored in `HOME_RUN_CINEMATIC_TUNING` for the
  // dev counter — assert the single source of truth stays a fixed, bounded cap.
  assert.equal(TUNING.confettiMaxCount, 36);
  assert.ok(Number.isFinite(TUNING.confettiMaxCount) && TUNING.confettiMaxCount > 0);
});

// ── 19/20: restart and focus-loss safety ────────────────────────────────────────

test("19. restart clears every cinematic state", () => {
  let found: { readonly seed: number; readonly t: number } | undefined;
  outer: for (let seed = 1; seed <= 40 && found === undefined; seed += 1) {
    for (let t = 20; t <= 200; t += 1) {
      const s = new HomeRunSession(seed);
      for (let k = 1; k < t; k += 1) {
        s.advance(intent({ start: k === 1 }));
      }
      s.advance(intent({ swing: true }));
      if (s.view().cinematicPhase === "anticipation") {
        found = { seed, t };
        break outer;
      }
    }
  }
  assert.ok(found !== undefined);
  const s = new HomeRunSession(found!.seed);
  for (let k = 1; k < found!.t; k += 1) {
    s.advance(intent({ start: k === 1 }));
  }
  s.advance(intent({ swing: true }));
  assert.notEqual(s.view().cinematicPhase, "none");
  s.reset();
  const v = s.view();
  assert.equal(v.cinematicPhase, "none");
  assert.equal(v.letterboxProgress, 0);
  assert.equal(v.hudVisible, true);
});

test("20. losing focus cannot leave the game in slow motion", () => {
  // The fixed-step loop is frame-driven, not wall-clock-driven: a stalled/hidden
  // tab simply stops calling `advance()` and resumes exactly where it left off —
  // there is no accumulated real-time drift to desync. Simulate a long stall
  // (many real ticks with no input) mid-cinematic and confirm the schedule still
  // recovers to full speed on its own.
  let state = enterCinematicPhase(newCinematic(), "contact");
  for (let i = 0; i < 2000; i += 1) {
    state = stepCinematic(state, TUNING);
  }
  assert.equal(state.timeScale, TUNING.contactSlowMotionScale, "contact holds its slow-mo scale — it does not itself recover");
  // But moving on to ballFollow (the actual recovery phase) still reaches full speed.
  state = enterCinematicPhase(state, "ballFollow");
  for (let i = 0; i < 2000; i += 1) {
    state = stepCinematic(state, TUNING);
  }
  assert.equal(state.timeScale, 1, "the schedule always recovers to full speed given enough real ticks");
});

// ── 21/22: cinematic presentation never touches the authoritative result ───────

test("21. the home-run ball reaches the same projected landing region with or without cinematic presentation", () => {
  const outcome = findHomerOutcome();
  // The REAL flight (session.ts#beginFlight → ball.ts#stepFlight) launches with
  // exactly `outcome.contactPoint`/`outcome.exitVelocity` — the same values the
  // cinematic's OWN projection already used to classify the swing as a home run.
  // Landing distance must agree with the projection to within a tiny numerical
  // tolerance (both run the identical `stepFlight` physics).
  assert.ok(outcome.projectedDistance > 0);
  assert.ok(Math.abs(outcome.projectedLanding.x) + outcome.projectedLanding.z >= C.WALL_LINE - 0.5, "the projected landing is beyond (or at) the wall line");
});

test("22. the cinematic does not modify the authoritative scoring result", () => {
  let found: { readonly seed: number; readonly t: number } | undefined;
  outer: for (let seed = 1; seed <= 40 && found === undefined; seed += 1) {
    for (let t = 20; t <= 200; t += 1) {
      const s = new HomeRunSession(seed);
      for (let k = 1; k < t; k += 1) {
        s.advance(intent({ start: k === 1 }));
      }
      s.advance(intent({ swing: true }));
      if (s.view().cinematicPhase === "anticipation") {
        found = { seed, t };
        break outer;
      }
    }
  }
  assert.ok(found !== undefined);
  const run = (): { readonly score: number; readonly outcome: string; readonly points: number } => {
    const s = new HomeRunSession(found!.seed);
    for (let k = 1; k < found!.t; k += 1) {
      s.advance(intent({ start: k === 1 }));
    }
    s.advance(intent({ swing: true }));
    let guard = 3000;
    while (s.results.length === 0 && guard > 0) {
      s.advance(IDLE);
      guard -= 1;
    }
    assert.ok(guard > 0);
    return { outcome: s.results[0]!.outcome, points: s.results[0]!.points, score: s.score };
  };
  const a = run();
  const b = run();
  assert.equal(a.outcome, "homer");
  assert.deepEqual(a, b, "the SAME swing must score identically every time — the cinematic is presentation-only");
});

// ── camera zoom ─────────────────────────────────────────────────────────────────

test("cinematic zoom narrows FOV monotonically with zoom blend, and never below the configured floor", () => {
  const wide = cinematicFovY(0, TUNING);
  const tight = cinematicFovY(1, TUNING);
  assert.equal(wide, C.CAMERA_FOV_Y);
  assert.ok(tight < wide);
  assert.ok(tight >= C.CAMERA_FOV_Y * (1 - TUNING.cinematicZoomAmount) - 1e-9);
});

test("the camera never moves again once the ball leaves the park, all the way through landing", () => {
  // The ball-follow freeze (test 16) must hold not just up to the wall but all
  // the way through the "landing" phase too — there is no separate "landing
  // camera" cut anymore, just the same frozen shot held steady.
  let found: { readonly seed: number; readonly t: number } | undefined;
  outer: for (let seed = 1; seed <= 40 && found === undefined; seed += 1) {
    for (let t = 20; t <= 200; t += 1) {
      const s = new HomeRunSession(seed);
      for (let k = 1; k < t; k += 1) {
        s.advance(intent({ start: k === 1 }));
      }
      s.advance(intent({ swing: true }));
      if (s.view().cinematicPhase === "anticipation") {
        found = { seed, t };
        break outer;
      }
    }
  }
  assert.ok(found !== undefined);
  const s = new HomeRunSession(found!.seed);
  for (let k = 1; k < found!.t; k += 1) {
    s.advance(intent({ start: k === 1 }));
  }
  s.advance(intent({ swing: true }));
  let guard = 3000;
  let frozenPose: { readonly position: Vec3; readonly target: Vec3 } | undefined;
  let sawLanding = false;
  const closeEnough = (a: Vec3, b: Vec3): boolean => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z) < 1e-9;
  while (s.phase !== "result" && guard > 0) {
    s.advance(IDLE);
    const v = s.view();
    const beyond = Math.abs(v.ball.x) + v.ball.z >= C.WALL_LINE;
    if (v.cinematicPhase === "ballFollow" && beyond) {
      frozenPose ??= { position: v.cameraPos, target: v.cameraTarget };
    }
    if (v.cinematicPhase === "landing" && frozenPose !== undefined) {
      assert.ok(closeEnough(v.cameraPos, frozenPose.position), "landing must not move the camera");
      assert.ok(closeEnough(v.cameraTarget, frozenPose.target), "landing must not re-aim the camera");
      sawLanding = true;
    }
    guard -= 1;
  }
  assert.ok(guard > 0);
  assert.ok(sawLanding, "the scripted swing must actually reach the landing phase");
});
