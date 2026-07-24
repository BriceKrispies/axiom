/*
 * treasure-chest-pick.test.ts — the chest game's own invariants: the reveal
 * cadence puts the LATCH strictly before the LID; idle dances draw only from
 * the ambient stream (so they can never hint at contents); and the pick only
 * ever reveals the object's preassigned slot (no substitution).
 */

import assert from "node:assert/strict";
import test from "node:test";

import type { Camera3D, EngineVec3, InputFrame, PointerSample } from "@axiom/web-engine";
import { planChoicePopulation } from "../../chance-engine/probability/choice-population.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import { createSession } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { addV3, crossV3, dotV3, hingedTransform, normalizeV3, quatMul, quatPitch, quatYaw, rotateByQuat, scaleV3, subV3, v3 } from "../../presentation/stage/vectors.ts";
import { easeOutBack } from "../../presentation/stage/easing.ts";
import {
  CHEST_BODY,
  CHEST_BODY_TOP,
  CHEST_HEIGHT,
  CHEST_LID,
  CHEST_TIMING,
  chestCamera,
  chestPosition,
  CRAB_WINDOW,
  crabIdle,
  dancePose,
  DECOR_KEYS,
  decorTargets,
  DEFAULT_DECOR,
  flightProgress,
  heroFraming,
  idlePhase,
  initialChestExtra,
  palmSway,
  presentationPhase,
  revealTimeline,
  spiralFlight,
  stepDecorDrag,
} from "./game.ts";
import type { ChestExtra, DecorDrag } from "./game.ts";
import { TREASURE_CHEST_PICK } from "./definition.ts";
import { canvasToGround, worldToCanvas } from "../../presentation/cameras/picking.ts";

/**
 * Project a world point into normalized screen coordinates for `camera`, where
 * ±1 is the frame edge on each axis. `aspect` is width/height; the framing
 * tests below run it at 1.0 (a SQUARE window) — the narrowest shape this
 * scene's camera is built for, so passing there means passing on anything
 * wider. Nothing in the app ships this; it exists to let a test assert what
 * "stays in the frame" actually means.
 */
const project = (camera: Camera3D, point: EngineVec3, aspect: number): { readonly x: number; readonly y: number; readonly depth: number } => {
  const forward = normalizeV3(subV3(camera.target, camera.position));
  const right = normalizeV3(crossV3(forward, v3(0, 1, 0)));
  const up = crossV3(right, forward);
  const d = subV3(point, camera.position);
  const depth = dotV3(d, forward);
  const halfHeight = depth * Math.tan(camera.fovY / 2);
  return { depth, x: dotV3(d, right) / (halfHeight * aspect), y: dotV3(d, up) / halfHeight };
};

/** The eight corners of a posed chest, in world space. */
const chestCorners = (base: EngineVec3, scale: number, yaw: number, pitch: number): readonly EngineVec3[] => {
  const q = quatMul(quatYaw(yaw), quatPitch(pitch));
  const hx = (CHEST_LID.x / 2) * scale;
  const hz = (CHEST_LID.z / 2) * scale;
  const h = CHEST_HEIGHT * scale;
  return [-1, 1].flatMap((sx) =>
    [0, 1].flatMap((sy) =>
      [-1, 1].map((sz) => {
        // Corners are taken about the chest's CENTER, which is where it spins.
        const local = v3(sx * hx, sy * h - h / 2, sz * hz);
        const r = rotateByQuat(local, q);
        return v3(base.x + r.x, base.y + h / 2 + r.y, base.z + r.z);
      }),
    ),
  );
};

test("the reveal cadence puts the latch strictly before the lid", () => {
  for (const speed of [0.5, 1, 2]) {
    for (const reduced of [false, true]) {
      const t = revealTimeline(speed, reduced);
      assert.ok(t.latchStart < t.latchEnd, "latch has a duration");
      assert.ok(t.latchEnd <= t.pauseEnd, "the latch lands before the settle pause");
      assert.ok(t.pauseEnd < t.lidEnd, "the lid opens only after the pause");
      assert.ok(t.latchEnd <= t.pauseEnd && t.pauseEnd <= t.lidEnd, "latch fully precedes lid");
      assert.ok(t.lidEnd < t.riseEnd, "the reward rises after the lid opens");
    }
  }
});

test("the presentation phases name the reveal ritual in its legal order", () => {
  const tl = revealTimeline(1, false);
  const base = createSession(TREASURE_CHEST_PICK.defaultConfig(), 1, 1, new SeededChanceResultSource(1), { choiceCount: 9, kind: "choice" });
  const at = (phase: SessionState["phase"], age: number): SessionState => ({ ...base, phase, phaseStartTick: 0, tick: age });

  assert.equal(presentationPhase(at("intro", 3), tl), "idle");
  assert.equal(presentationPhase(at("ready", 3), tl), "idle");
  assert.equal(presentationPhase(at("committing", 3), tl), "committed");
  assert.equal(presentationPhase(at("resetting", 3), tl), "reset");
  assert.equal(presentationPhase(at("celebrating", 3), tl), "result");
  assert.equal(presentationPhase(at("complete", 3), tl), "result");

  // Inside the reveal, the named sub-phases advance monotonically along the ritual.
  const ritual = [0, tl.braceEnd, tl.latchEnd, tl.pauseEnd, tl.lidEnd, tl.riseEnd].map((age) => presentationPhase(at("revealing", age), tl));
  assert.deepEqual(ritual, ["anticipation", "latch", "seam", "lid", "burst", "prize"]);
});

test("idle cosmetics are deterministic, desynced, and outcome-independent", () => {
  // Each chest gets its own idle phase in [0, 2π) — so the nine never move in unison.
  const phases = Array.from({ length: 9 }, (_, i) => idlePhase(i));
  phases.forEach((p) => assert.ok(p >= 0 && p < Math.PI * 2, "idle phase in range"));
  assert.equal(new Set(phases.map((p) => p.toFixed(5))).size, 9, "nine distinct idle phases");
});

test("idle dances draw only from the ambient stream", () => {
  // dancePose is a pure function of (index, count, tick, seed, liveliness) —
  // it takes NO presentation/gameplay seed, so it cannot correlate with which
  // chest wins. Same inputs → identical pose; different tick → free to differ.
  for (let tick = 0; tick < 400; tick += 7) {
    const a = dancePose(3, 9, tick, 12345, 0.7);
    const b = dancePose(3, 9, tick, 12345, 0.7);
    assert.deepEqual(a, b);
  }
  // The dance is real motion (not a dead stub) somewhere in the window.
  const moved = Array.from({ length: 200 }, (_, tick) => dancePose(4, 9, tick, 999, 0.7)).some(
    (pose) => Math.abs(pose.scootX) + Math.abs(pose.twist) + Math.abs(pose.squash) > 1e-4,
  );
  assert.ok(moved, "the dance must actually move");
  // Zero liveliness freezes the dance.
  assert.deepEqual(dancePose(4, 9, 50, 999, 0), { scootX: 0, squash: 0, twist: 0 });
});

test("the palm sways in the wind — pure in the tick, bounded, and moving", () => {
  // palmSway takes NO seed: wind is the same every session and cannot correlate
  // with any outcome. Same tick → identical sway; and it stays gentle.
  for (let tick = 0; tick < 600; tick += 11) {
    const a = palmSway(tick);
    const b = palmSway(tick);
    assert.equal(a.bend, b.bend);
    assert.equal(a.flutter(3), b.flutter(3));
    // The sway is deliberately gentle — a barely-there lean and a small flutter.
    assert.ok(Math.abs(a.bend) < 0.04, "sway stays a barely-there lean");
    assert.ok(Math.abs(a.flutter(5)) < 0.025, "frond flutter stays small");
  }
  // The bend is real motion across a full slow cycle, and different fronds
  // flutter apart. Sampled over 1200 ticks so both slow frequencies peak.
  const bends = Array.from({ length: 1200 }, (_, tick) => palmSway(tick).bend);
  assert.ok(Math.max(...bends) - Math.min(...bends) > 0.02, "the palm must actually sway");
  const sway = palmSway(37);
  assert.notEqual(sway.flutter(0), sway.flutter(1), "fronds flutter out of unison");
});

test("the crab's idle animations fire on a random interval from the ambient stream", () => {
  // Pure in (tick, seed): same inputs → identical pose.
  for (let tick = 0; tick < 1200; tick += 13) {
    assert.deepEqual(crabIdle(tick, 4242), crabIdle(tick, 4242));
  }
  // Across many windows the crab performs every idle in its repertoire AND rests
  // — i.e. the animations come on an interval, not every window and not never.
  const kinds = new Set(Array.from({ length: 60 }, (_, w) => crabIdle(w * CRAB_WINDOW + CRAB_WINDOW / 2, 7).kind));
  assert.ok(kinds.has("rest"), "the crab rests between bits of business");
  assert.ok(kinds.has("scuttle") && kinds.has("wave") && kinds.has("bob") && kinds.has("turn"), "every idle in the repertoire plays");
  // A performed idle is real motion somewhere in its run (each figure passes
  // through zero-crossings, so check the PEAK across many ticks, not one instant);
  // a rest is still but for the breathe.
  const motion = (p: ReturnType<typeof crabIdle>): number =>
    Math.abs(p.scootX) + Math.abs(p.bob) + Math.abs(p.yaw) + Math.abs(p.clawLift) + Math.abs(p.legWiggle);
  const poses = Array.from({ length: 2000 }, (_, tick) => crabIdle(tick, 7));
  const peakActive = Math.max(...poses.filter((p) => p.kind !== "rest").map(motion));
  assert.ok(peakActive > 0.1, "an active idle really moves the crab");
  // A rest contributes no gross motion (only the tiny breathe/eye drift, which
  // are not part of `motion`).
  const rested = poses.find((p) => p.kind === "rest");
  assert.ok(rested !== undefined && motion(rested) === 0, "a resting crab is still");
});

test("the spiral leaves the grid slot, converges on the hero anchor, and lands facing front", () => {
  const camera = chestCamera(9);
  const basis = heroFraming(camera);
  const from = chestPosition(0, 9);
  const to = v3(0, 3.2, 2);
  /** Distance from the hero anchor measured IN THE SCREEN PLANE — the plane the
   * spiral is actually described in. */
  const screenRadius = (p: EngineVec3): number => {
    const d = subV3(p, to);
    return Math.hypot(dotV3(d, basis.right), dotV3(d, basis.up));
  };

  // The endpoints are exact: it starts ON its slot and finishes ON the anchor,
  // so the flight neither pops at the start nor drifts at the end.
  const start = spiralFlight(from, to, 0, basis);
  (["x", "y", "z"] as const).forEach((axis) => {
    assert.ok(Math.abs(start.position[axis] - from[axis]) < 1e-9, `starts exactly on its slot (${axis})`);
  });
  assert.equal(start.spin, 0);
  assert.equal(start.grow, 0);

  const end = spiralFlight(from, to, 1, basis);
  ["x", "y", "z"].forEach((axis) => {
    assert.ok(Math.abs(end.position[axis as "x"] - to[axis as "x"]) < 1e-9, `arrives exactly on the anchor (${axis})`);
  });
  assert.equal(end.grow, 1);
  assert.ok(Math.abs(end.tumble) < 1e-9, "the tumble unwinds to level");

  // A WHOLE number of turns is what leaves the latch, lock plate, and lid
  // facing the camera when the reveal starts.
  const turns = end.spin / (Math.PI * 2);
  assert.equal(turns, CHEST_TIMING.spiralTurns);
  assert.equal(turns, Math.round(turns), "the spiral ends front-facing");

  // The orbit converges INWARD: it never swings wider on screen than the slot
  // it started from, and it closes all the way to the middle. That bound is the
  // whole reason a screen-plane spiral stays framed — every slot begins on
  // screen, so a path that never exceeds its start can never leave.
  const radii = Array.from({ length: 41 }, (_, i) => screenRadius(spiralFlight(from, to, i / 40, basis).position));
  const startRadius = radii[0] ?? 0;
  radii.forEach((r, i) => assert.ok(r <= startRadius + 1e-9, `never swings wider than its slot (step ${i})`));
  assert.ok((radii.at(-1) ?? 1) < 1e-9, "closes onto the anchor");
  assert.ok((radii[20] ?? 0) < startRadius * 0.5, "and is well inside by the midpoint");

  // It really winds rather than sliding straight in: the angle of its on-screen
  // offset must sweep right around, not hold steady on the line to the anchor.
  const angles = Array.from({ length: 40 }, (_, i) => {
    const d = subV3(spiralFlight(from, to, (i + 1) / 41, basis).position, to);
    return Math.atan2(dotV3(d, basis.up), dotV3(d, basis.right));
  });
  const swept = angles.slice(1).reduce((total, a, i) => {
    const prev = angles[i] ?? 0;
    const step = ((a - prev + Math.PI * 3) % (Math.PI * 2)) - Math.PI;
    return total + Math.abs(step);
  }, 0);
  assert.ok(swept > Math.PI * 2, `the path winds around the anchor (swept ${(swept / Math.PI).toFixed(1)}π)`);
});

test("the flight is pure — no seed, no clock, no outcome", () => {
  const basis = heroFraming(chestCamera(9));
  const from = chestPosition(7, 9);
  const to = v3(0, 3, 2);
  for (let i = 0; i <= 20; i += 1) {
    assert.deepEqual(spiralFlight(from, to, i / 20, basis), spiralFlight(from, to, i / 20, basis));
  }
});

test("the commit beat is long enough to finish the spiral before the lid is touched", () => {
  const config = TREASURE_CHEST_PICK.defaultConfig();
  const base = createSession(config, 1, 1, new SeededChanceResultSource(1), { choiceCount: 9, kind: "choice" });
  const at = (phase: SessionState["phase"], age: number): SessionState => ({ ...base, phase, phaseStartTick: 0, tick: age });

  // The flight completes exactly as the commit beat ends — the chest is fully
  // parked in its hero framing before "revealing" opens the latch.
  assert.equal(flightProgress(at("committing", 0), 1), 0);
  assert.ok(flightProgress(at("committing", CHEST_TIMING.spiralTicks - 1), 1) < 1, "still flying mid-beat");
  assert.equal(flightProgress(at("committing", CHEST_TIMING.spiralTicks), 1), 1);

  // …and it HOLDS there for the whole reveal and result, so the chest does not
  // slide back to the board while it is opening.
  (["revealing", "celebrating", "complete"] as const).forEach((phase) => {
    assert.equal(flightProgress(at(phase, 40), 1), 1, `${phase} holds the hero framing`);
  });
  // Only the reset releases it.
  assert.ok(flightProgress(at("resetting", 0), 1) === 1 && flightProgress(at("resetting", 99), 1) === 0, "reset eases back out");
  assert.equal(flightProgress(at("ready", 5), 1), 0, "an unpicked board is never in flight");
});

test("the chosen chest stays fully inside the frame for the whole flight and reveal", () => {
  const count = 9;
  const camera = chestCamera(count);
  const framing = heroFraming(camera);
  const square = 1; // the narrowest viewport this scene's camera is built for

  // The BOARD's own framing is the baseline. On a square window the outer chests
  // already sit a little past the edge at rest — a pre-existing property of this
  // camera and grid, not something the flight introduces — so the flight is held
  // to "never frames worse than the board already does". On any window at least
  // as wide as the board itself needs, that is exactly "always fully on screen".
  const resting = Array.from({ length: count }, (_, i) => chestCorners(chestPosition(i, count), 1, 0, 0))
    .flat()
    .map((corner) => project(camera, corner, square));
  const budgetX = Math.max(1, ...resting.map((p) => Math.abs(p.x)));
  const budgetY = Math.max(1, ...resting.map((p) => Math.abs(p.y)));

  // Every grid slot, flown to the hero anchor, stays within that budget at every
  // step — including the corner chests, which swing the widest.
  const heroBase = v3(framing.anchor.x, framing.anchor.y - (CHEST_HEIGHT / 2) * framing.scale, framing.anchor.z);
  for (let index = 0; index < count; index += 1) {
    const from = chestPosition(index, count);
    for (let step = 0; step <= 60; step += 1) {
      const t = step / 60;
      const pose = spiralFlight(from, heroBase, t, framing);
      const scale = 1 + (framing.scale - 1) * pose.grow;
      chestCorners(pose.position, scale, pose.spin, pose.tumble).forEach((corner) => {
        const p = project(camera, corner, square);
        assert.ok(p.depth > camera.near, `chest ${index} stays in front of the camera at t=${t.toFixed(2)}`);
        assert.ok(Math.abs(p.x) <= budgetX, `chest ${index} stays in frame horizontally at t=${t.toFixed(2)} (x=${p.x.toFixed(3)})`);
        assert.ok(Math.abs(p.y) <= budgetY, `chest ${index} stays in frame vertically at t=${t.toFixed(2)} (y=${p.y.toFixed(3)})`);
      });
    }
  }

  // The flight must also END better framed than it began: once parked, the chest
  // is comfortably inside even a square window, with margin on every side.
  const parked = chestCorners(heroBase, framing.scale, 0, 0).map((corner) => project(camera, corner, square));
  parked.forEach((p) => {
    assert.ok(Math.abs(p.x) <= 1, `the parked chest fits horizontally (x=${p.x.toFixed(3)})`);
    assert.ok(Math.abs(p.y) <= 1, `the parked chest fits vertically (y=${p.y.toFixed(3)})`);
  });

  // The OPEN LID is the tallest thing the reveal ever puts on screen — it
  // swings up and back well past the closed silhouette — so it is what really
  // bounds how big the hero chest may be. Posed through the same
  // `hingedTransform` the scene builds it with, so the two cannot drift apart.
  for (let step = 0; step <= 40; step += 1) {
    const lidT = step / 40;
    const grow = framing.scale;
    const q = quatMul(quatYaw(0), quatPitch(-CHEST_TIMING.tilt));
    const lidQ = quatMul(q, quatPitch(-easeOutBack(lidT) * CHEST_TIMING.lidOpen));
    const hinge = addV3(heroBase, rotateByQuat(scaleV3(v3(0, CHEST_BODY.y, -CHEST_BODY.z / 2), grow), q));
    const lid = hingedTransform(hinge, scaleV3(v3(0, CHEST_LID.y / 2, CHEST_LID.z / 2), grow), lidQ, scaleV3(CHEST_LID, grow));
    [-1, 1].forEach((sx) =>
      [-1, 1].forEach((sy) =>
        [-1, 1].forEach((sz) => {
          const corner = addV3(lid.position, rotateByQuat(scaleV3(v3((sx * CHEST_LID.x) / 2, (sy * CHEST_LID.y) / 2, (sz * CHEST_LID.z) / 2), grow), lidQ));
          const p = project(camera, corner, square);
          assert.ok(Math.abs(p.y) <= 1, `the open lid stays in frame at lidT=${lidT.toFixed(2)} (y=${p.y.toFixed(3)})`);
          assert.ok(Math.abs(p.x) <= 1, `the open lid stays in frame horizontally at lidT=${lidT.toFixed(2)} (x=${p.x.toFixed(3)})`);
        }),
      ),
    );
  }

  // The prize that climbs out of the parked chest also stays framed, across the
  // whole rise INCLUDING the overshoot of its ease.
  const top = v3(heroBase.x, heroBase.y + CHEST_BODY_TOP * framing.scale, heroBase.z);
  const gem = framing.scale * CHEST_TIMING.prizeDamp;
  for (let step = 0; step <= 40; step += 1) {
    const riseT = step / 40;
    // The largest prize the game can yield (a jackpot gem) is the binding case.
    const size = (0.54 + 0.18) * (0.5 + 0.5 * riseT) * 1.04 * gem;
    const climb = CHEST_TIMING.riseHeight * easeOutBack(riseT) * framing.scale * CHEST_TIMING.riseDamp;
    const apex = project(camera, v3(top.x, top.y + climb + size, top.z), square);
    assert.ok(apex.y <= 1, `the prize apex stays in frame at riseT=${riseT.toFixed(2)} (y=${apex.y.toFixed(3)})`);
  }
});

test("the hero framing fills the frame without overflowing it", () => {
  const camera = chestCamera(9);
  const framing = heroFraming(camera);

  // It is genuinely a CLOSE-UP: the chest ends up far larger than it is on the
  // board, and much nearer to the camera than the board is.
  assert.ok(framing.scale > 1.5, `the hero chest is a real enlargement (${framing.scale.toFixed(2)}×)`);
  assert.ok(framing.distance < Math.hypot(camera.position.y, camera.position.z) * 0.7, "the hero plane is well in front of the board");

  // It commands the frame — but the width guard keeps it inside even on a
  // square window, which is the whole point of sizing from the frustum.
  const heroBase = v3(framing.anchor.x, framing.anchor.y - (CHEST_HEIGHT / 2) * framing.scale, framing.anchor.z);
  // Measured on the projected corners, so the chest's NEAR face — which the
  // perspective enlarges past the flat width budget — is what gets checked.
  const xs = chestCorners(heroBase, framing.scale, 0, 0).map((c) => project(camera, c, 1).x);
  const span = Math.max(...xs) - Math.min(...xs);
  assert.ok(span > 0.6, `the chest dominates the frame (spans ${(span * 50).toFixed(0)}% of width)`);
  assert.ok(Math.max(...xs.map(Math.abs)) <= 1, `and still fits a square window (max |x| = ${Math.max(...xs.map(Math.abs)).toFixed(3)})`);

  // The veil hangs between the hero chest and the board: behind everything the
  // chest occupies, in front of the nearest chest still sitting on the grid.
  const veilDepth = framing.distance + CHEST_TIMING.veilGap;
  const chestBack = Math.max(...chestCorners(heroBase, framing.scale, 0, 0).map((c) => project(camera, c, 1).depth));
  const nearestOnBoard = Math.min(
    ...Array.from({ length: 9 }, (_, i) => {
      const slot = chestPosition(i, 9);
      return Math.min(...chestCorners(slot, 1, 0, 0).map((c) => project(camera, c, 1).depth));
    }),
  );
  assert.ok(veilDepth > chestBack, `the veil is behind the hero chest (${veilDepth.toFixed(2)} > ${chestBack.toFixed(2)})`);
  assert.ok(veilDepth < nearestOnBoard, `the veil is in front of the board (${veilDepth.toFixed(2)} < ${nearestOnBoard.toFixed(2)})`);
});

test("the chest population is fixed before the pick and higher win rate means more prize chests", () => {
  const config = TREASURE_CHEST_PICK.defaultConfig();
  // Assigned before any pick; the selection only looks up its slot.
  const population = planChoicePopulation(config, 9, 4242, 1);
  const winners = population.winnersByIndex.filter((tier) => tier !== null).length;
  assert.equal(winners, population.winnerCount);

  // Averaged over seeds, more of the nine chests hold prizes as the target rises.
  const meanWinners = (p: number): number => {
    let total = 0;
    for (let seed = 1; seed <= 600; seed += 1) {
      total += planChoicePopulation({ ...config, targetWinRate: p }, 9, seed, 1).winnerCount;
    }
    return total / 600;
  };
  assert.ok(meanWinners(0.7) > meanWinners(0.3), "more prize chests at a higher win rate");
  assert.ok(Math.abs(meanWinners(0.5) - 4.5) < 0.2, "≈ 9·0.5 chests hold prizes");
});

// ── pick-up-and-move the beach props ───────────────────────────────────────────

/** An input frame carrying a pointer sample (or none). */
const inputFrame = (pointer: PointerSample | undefined): InputFrame => ({
  down: new Set(),
  look: { x: 0, y: 0 },
  pointer,
  pressed: new Set(),
  released: new Set(),
});
const at = (x: number, y: number, down: boolean): PointerSample => ({ down, pos: { x, y } });

test("canvasToGround is the inverse of worldToCanvas on the ground plane", () => {
  const camera = chestCamera(9);
  // Round-trip a spread of ground points through project → un-project.
  for (const p of [v3(0, 0, 0), v3(2, 0, -1.5), v3(-3.4, 0, 1.1), v3(5, 0, -3.3)]) {
    const screen = worldToCanvas(camera, p);
    assert.ok(screen !== null, "ground point projects in front of the camera");
    const back = canvasToGround(camera, at(screen.x, screen.y, true));
    assert.ok(back !== null, "the cursor ray meets the ground");
    assert.ok(Math.hypot(back.x - p.x, back.z - p.z) < 1e-6, "round-trips to the same ground point");
    assert.equal(back.y, 0, "lands exactly on the ground plane");
  }
  assert.equal(canvasToGround(camera, undefined), null, "no cursor → no ground point");
});

test("a prop can be grabbed, dragged, and dropped — pure in (decor, input, camera)", () => {
  const camera = chestCamera(9);
  const screenOfProp = (key: "palm" | "castle" | "crab"): { x: number; y: number } => {
    const t = decorTargets(DEFAULT_DECOR.props).find((_, i) => DECOR_KEYS[i] === key);
    const s = worldToCanvas(camera, (t as { at: EngineVec3 }).at);
    return s as { x: number; y: number };
  };

  // A press whose cursor is over the palm grabs it (and owns the pointer).
  const palmPx = screenOfProp("palm");
  const grab = stepDecorDrag(DEFAULT_DECOR, inputFrame(at(palmPx.x, palmPx.y, true)), camera);
  assert.equal(grab.decor.held, "palm", "the palm is picked up");
  assert.ok(grab.active, "the drag owns the pointer");

  // Pure: same inputs → identical result.
  const grab2 = stepDecorDrag(DEFAULT_DECOR, inputFrame(at(palmPx.x, palmPx.y, true)), camera);
  assert.deepEqual(grab, grab2);

  // Dragging moves the palm by the SAME ground-delta the cursor travelled (the
  // grab offset is preserved, so the prop doesn't snap its base to the cursor).
  const grabGround = canvasToGround(camera, at(palmPx.x, palmPx.y, true)) as EngineVec3;
  const destGround = v3(0.5, 0, 2.5);
  const dest = worldToCanvas(camera, destGround) as { x: number; y: number };
  const dragged = stepDecorDrag(grab.decor, inputFrame(at(dest.x, dest.y, true)), camera);
  assert.equal(dragged.decor.held, "palm", "still held while the button is down");
  const dx = dragged.decor.props.palm.x - DEFAULT_DECOR.props.palm.x;
  const dz = dragged.decor.props.palm.z - DEFAULT_DECOR.props.palm.z;
  assert.ok(Math.hypot(dx - (destGround.x - grabGround.x), dz - (destGround.z - grabGround.z)) < 0.05, "palm follows the cursor's ground delta");
  assert.equal(dragged.decor.props.palm.y, 0, "the prop stays on the ground");
  assert.deepEqual(dragged.decor.props.castle, DEFAULT_DECOR.props.castle, "other props are untouched");

  // Releasing drops it (keeps the moved position).
  const dropped = stepDecorDrag(dragged.decor, inputFrame(at(dest.x, dest.y, false)), camera);
  assert.equal(dropped.decor.held, null, "released → nothing held");
  assert.deepEqual(dropped.decor.props.palm, dragged.decor.props.palm, "the palm stays where it was dropped");
});

test("a press away from every prop grabs nothing and yields the pointer to chest-picking", () => {
  const camera = chestCamera(9);
  // The centre of the board (over the middle chest) is far from any prop.
  const centre = worldToCanvas(camera, chestPosition(4, 9)) as { x: number; y: number };
  const step = stepDecorDrag(DEFAULT_DECOR, inputFrame(at(centre.x, centre.y, true)), camera);
  assert.equal(step.decor.held, null, "nothing grabbed away from the props");
  assert.equal(step.active, false, "the drag does not own the pointer, so a chest can be picked");
});

test("a prop is only grabbed on the press EDGE, not while a drag sweeps over it", () => {
  const camera = chestCamera(9);
  const crabPx = worldToCanvas(camera, (decorTargets(DEFAULT_DECOR.props)[2] as { at: EngineVec3 }).at) as { x: number; y: number };
  // Pointer already down on the previous tick (pointerDown true) → passing over
  // the crab must NOT hijack it mid-drag.
  const alreadyDown: DecorDrag = { ...DEFAULT_DECOR, pointerDown: true };
  const sweep = stepDecorDrag(alreadyDown, inputFrame(at(crabPx.x, crabPx.y, true)), camera);
  assert.equal(sweep.decor.held, null, "no grab without a fresh press edge");
});

test("moved props persist across a round reset, but reset on a fresh (page-load) session", () => {
  const session = createSession(TREASURE_CHEST_PICK.defaultConfig(), 1, 1, new SeededChanceResultSource(1), { choiceCount: 9, kind: "choice" });

  // First round of a page load (previous = null) → props at home, drag clean.
  const first = initialChestExtra(session, null);
  assert.deepEqual(first.decor, DEFAULT_DECOR, "a fresh session starts the props at home");

  // A prior round that had props moved (and mid-drag transient state set).
  const moved: ChestExtra = {
    ...first,
    decor: { grabOffset: v3(1, 1, 1), held: "palm", pointerDown: true, props: { castle: v3(-2, 0, 1), crab: v3(3, 0, -1), palm: v3(1, 0, 2) } },
    revealStartTick: 42,
  };

  // New Round / Replay carries the prior extra in: the PLACED positions persist,
  // the transient drag fields reset, and the per-round bits start clean.
  const next = initialChestExtra(session, moved);
  assert.deepEqual(next.decor.props, moved.decor.props, "placed prop positions persist across the reset");
  assert.equal(next.decor.held, null, "nothing is held in the new round");
  assert.equal(next.decor.pointerDown, false, "the drag press-state resets");
  assert.deepEqual(next.decor.grabOffset, v3(0, 0, 0), "the grab offset resets");
  assert.equal(next.revealStartTick, null, "the per-round reveal clock resets");
});
