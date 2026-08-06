/*
 * treasure-chest-pick.test.ts — the chest game's own invariants: the reveal
 * cadence puts the LATCH strictly before the LID; idle dances draw only from
 * the ambient stream (so they can never hint at contents); and the pick only
 * ever reveals the object's preassigned slot (no substitution).
 */

import assert from "node:assert/strict";
import test from "node:test";

import type { Camera3D, EngineVec3, InputFrame, PointerSample, SceneInstance } from "@axiom/web-engine";
import { planChoicePopulation } from "../../chance-engine/probability/choice-population.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import { createSession } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { RARITIES } from "../../chance-engine/configuration/schema.ts";
import { addV3, crossV3, dotV3, hingedTransform, normalizeV3, quatMul, quatPitch, quatYaw, QUAT_IDENTITY, rotateByQuat, scaleV3, subV3, v3 } from "../../presentation/stage/vectors.ts";
import { PRIZE_EXTENT, PRIZE_KINDS, PRIZE_SIZE, prizeExtentOf, prizeInstances, prizeKindOf } from "./prizes/index.ts";
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
  defaultChestSlots,
  DRAG_THRESHOLD_PX,
  chestTargetsAt,
  commitBeatTicks,
  crabJourney,
  flightProgress,
  initialChestDrag,
  stepChestDrag,
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
import { chestScene, WATER_RADIUS } from "./scene.ts";
import { TREASURE_CHEST_PICK } from "./definition.ts";
import { canvasToGround, pickAt, worldToCanvas } from "../../presentation/cameras/picking.ts";

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

/*
 * The beach props stand ON THE SAND — nothing they are made of hangs over the lagoon.
 *
 * This is measured on the REAL emitted scene rather than on a copy of either prop's
 * geometry: the test asks `chestScene` for the frame, takes every instance a prop
 * contributed, and walks the eight corners of each one. That way a change to the
 * palm's frond length, or to how far the crab's `turn` idle swings his pennant, is
 * caught by the same assertion that catches a change to a home position — there is
 * no second description of either prop here to drift out of step with the first.
 *
 * The quantity checked is the corner's APPARENT radius, not its footprint radius,
 * and that distinction is the whole test. A frond tip is 2.5 units in the air under
 * a camera pitched 54.5° down, so what the player sees it sitting over is not the
 * ground point beneath it: it is where the camera's ray through it meets the lagoon
 * plane, which is pushed AWAY FROM THE CAMERA by roughly the tip's height. Which
 * way that helps depends on which shore the prop stands on — it pushes the palm
 * (far side) outward and the crab (near side) toward the pool, which is why the
 * crab's raised flag clipped the water from a position his shell cleared easily.
 * Only the ray answers what the frame shows.
 */
const SHORE_MARGIN = 0.15;

/** Session seeds sampled for the crab. His idle repertoire is elected from the
 * ambient stream, so WHICH figure plays — and therefore how far the pennant swings
 * — depends on the seed; the reach varies by ~0.05 units across seeds. The palm
 * takes no seed at all (`palmSway` is pure in the tick), so it is unaffected. */
const PROP_SEEDS = [1, 7, 470573198];

test("the beach props stand clear of the lagoon", () => {
  const config = TREASURE_CHEST_PICK.defaultConfig();
  const runtime = {
    config,
    onHud: (): void => {},
    round: 1,
    seed: 1,
    settings: { cameraShake: true, highContrast: false, masterVolume: 0, particleScale: 1, reducedMotion: false, sfxVolume: 0 },
    source: new SeededChanceResultSource(1),
  };
  const camera = chestCamera(9);

  /** Where `point` appears to sit on the lagoon plane: the radius at which the
   * camera ray through it crosses y = 0. */
  const apparentRadius = (point: EngineVec3): number => {
    const d = subV3(point, camera.position);
    const hit = addV3(camera.position, scaleV3(d, -camera.position.y / d.y));
    return Math.hypot(hit.x, hit.z);
  };

  const CORNERS = [-0.5, 0.5].flatMap((sx) => [-0.5, 0.5].flatMap((sy) => [-0.5, 0.5].map((sz) => v3(sx, sy, sz))));
  const cornersOf = (instance: SceneInstance): readonly EngineVec3[] => {
    const t = instance.transform;
    return CORNERS.map((c) =>
      addV3(t.position, rotateByQuat(v3(c.x * t.scale.x, c.y * t.scale.y, c.z * t.scale.z), t.rotation)),
    );
  };

  /** Every frame the props are sampled in. The window is long enough to cover
   * `palmSway`'s slowest term (~1050 ticks) and several of the crab's 150-tick idle
   * slots, since each prop only reaches its furthest at one phase of its cycle. */
  const reach = (seed: number, prefix: string): number => {
    const session = createSession(config, seed, 1, new SeededChanceResultSource(1), { choiceCount: 9, kind: "choice" });
    const extra: ChestExtra = initialChestExtra(session, null);
    return Array.from({ length: 145 }, (_, i) => i * 11)
      .map((tick) => {
        const scene = chestScene(runtime, { extra, pendingContext: null, pendingReset: null, session: { ...session, tick } });
        const parts = scene.instances.filter((instance) => instance.key.startsWith(prefix));
        assert.ok(parts.length > 5, `the ${prefix} prop really is in the scene being measured`);
        return Math.min(...parts.flatMap((instance) => cornersOf(instance).map(apparentRadius)));
      })
      .reduce((a, b) => Math.min(a, b));
  };

  // The palm: a headless scene is the FRUGAL arm (`gpuDetail()` is false with no
  // renderer mounted), whose fronds are single boards. The hardware arm replaces
  // each board with a slim midrib plus leaflets that splay ~0.04·length further out
  // to the side than the board's own half-width — about 0.08 world units at these
  // frond lengths. `SHORE_MARGIN` is set above that gap, so clearing it here clears
  // it on both arms. (The crab is identical on every arm, so his figure is exact.)
  //
  // Both props are checked against the same floor and both are on the same sand
  // band, so one loop covers them: what differs is only which part of each reaches
  // furthest — the palm's leeward frond, and the crab's pennant.
  for (const prefix of ["palm:", "crab:"]) {
    for (const seed of PROP_SEEDS) {
      const worst = reach(seed, prefix);
      assert.ok(
        worst >= WATER_RADIUS + SHORE_MARGIN,
        `at seed ${seed}, "${prefix}" reaches an apparent radius of ${worst.toFixed(2)}, inside the lagoon's shore ` +
          `at ${WATER_RADIUS} (+${SHORE_MARGIN} of required daylight) — it would read as hanging over the water. ` +
          `Move the prop's home further out (DEFAULT_DECOR.props) rather than shrinking the prop; note the left ` +
          `frame edge is what caps how far out either one can go.`,
      );
    }
  }
});

test("the crab's idle animations fire on a random interval from the ambient stream", () => {
  // Pure in (tick, seed): same inputs → identical pose.
  for (let tick = 0; tick < 1200; tick += 13) {
    assert.deepEqual(crabIdle(tick, 4242), crabIdle(tick, 4242));
  }
  // Across many windows the crab performs every idle in its repertoire AND rests
  // — i.e. the animations come on an interval, not every window and not never.
  const kinds = new Set(Array.from({ length: 60 }, (_, w) => crabIdle(w * CRAB_WINDOW + CRAB_WINDOW / 2, 7).kind));
  // The repertoire is EXACTLY rest + the three in-place figures. Asserted as an
  // equality rather than three `has` checks, because the thing that matters is as
  // much what is absent: no idle may translate the crab off the mark the player
  // set him on (the side-scuttle that used to do exactly that is gone — see
  // `CrabIdleKind`), and only an exact set catches one coming back.
  assert.deepEqual([...kinds].sort(), ["bob", "rest", "turn", "wave"], "rest plus the three in-place figures, and nothing that travels");
  // A performed idle is real motion somewhere in its run (each figure passes
  // through zero-crossings, so check the PEAK across many ticks, not one instant);
  // a rest is still but for the breathe.
  const motion = (p: ReturnType<typeof crabIdle>): number =>
    Math.abs(p.bob) + Math.abs(p.yaw) + Math.abs(p.clawLift) + Math.abs(p.legWiggle);
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

  // The commit beat is TWO beats: the crab walks to the chest, and only then does
  // the chest fly. So nothing moves for the length of the approach, and the flight
  // completes exactly as the whole beat ends — the chest is fully parked in its
  // hero framing before "revealing" opens the latch.
  assert.equal(flightProgress(at("committing", 0), 1), 0, "nothing flies while the crab is still walking");
  assert.equal(flightProgress(at("committing", CHEST_TIMING.approachTicks), 1), 0, "the flight begins as he arrives");
  assert.ok(flightProgress(at("committing", CHEST_TIMING.approachTicks + 1), 1) > 0, "and is under way a tick later");
  assert.ok(flightProgress(at("committing", commitBeatTicks - 1), 1) < 1, "still flying mid-beat");
  assert.equal(flightProgress(at("committing", commitBeatTicks), 1), 1, "landing exactly as the commit beat ends");

  // …and it HOLDS there for the whole reveal and result, so the chest does not
  // slide back to the board while it is opening.
  (["revealing", "celebrating", "complete"] as const).forEach((phase) => {
    assert.equal(flightProgress(at(phase, 40), 1), 1, `${phase} holds the hero framing`);
  });
  // Only the reset releases it.
  assert.ok(flightProgress(at("resetting", 0), 1) === 1 && flightProgress(at("resetting", 99), 1) === 0, "reset eases back out");
  assert.equal(flightProgress(at("ready", 5), 1), 0, "an unpicked board is never in flight");
});

test("the crab walks to the chest, rides it, and hops as the lid lands", () => {
  const config = TREASURE_CHEST_PICK.defaultConfig();
  const base = createSession(config, 1, 1, new SeededChanceResultSource(1), { choiceCount: 9, kind: "choice" });
  const at = (phase: SessionState["phase"], age: number): SessionState => ({ ...base, phase, phaseStartTick: 0, tick: age });
  const tl = revealTimeline(1, false);
  const journey = (phase: SessionState["phase"], age: number): ReturnType<typeof crabJourney> => crabJourney(at(phase, age), 1, tl);

  // At rest he is on his own patch of sand and has hold of nothing.
  assert.deepEqual(journey("ready", 30), { approach: 0, grip: 0, hop: 0, riding: false });
  assert.deepEqual(journey("intro", 5), { approach: 0, grip: 0, hop: 0, riding: false });

  // The walk: he sets off from a standstill, is genuinely mid-crossing partway
  // through, and only takes hold at the far end.
  assert.equal(journey("committing", 0).approach, 0, "he starts where he was standing");
  const mid = journey("committing", Math.floor(CHEST_TIMING.approachTicks / 2));
  assert.ok(mid.approach > 0.1 && mid.approach < 0.9, `mid-walk (${mid.approach.toFixed(2)})`);
  assert.equal(mid.riding, false, "still on the sand");
  assert.equal(journey("committing", CHEST_TIMING.approachTicks).riding, true, "aboard as the flight begins");
  // The claws are already on the rail by the time the chest leaves the ground —
  // he cannot be carrying something he has not gripped.
  assert.equal(journey("committing", CHEST_TIMING.approachTicks).grip, 1, "gripped before lift-off");

  // He holds on through the flight, the reveal, and the result.
  (["revealing", "celebrating", "complete"] as const).forEach((phase) => {
    assert.equal(journey(phase, 5).riding, true, `${phase} keeps him aboard`);
    assert.equal(journey(phase, 5).approach, 1);
  });

  // The hop is ONE spike at the moment the lid lands open — nothing before, a
  // real jump at the mark, and settled again afterwards.
  assert.equal(journey("revealing", tl.lidEnd - 1).hop, 0, "no hop before the lid lands");
  const hopPeak = journey("revealing", tl.lidEnd + Math.floor(CHEST_TIMING.crabHopTicks / 2)).hop;
  assert.ok(hopPeak > 0.8, `he really jumps (${hopPeak.toFixed(2)})`);
  assert.equal(journey("revealing", tl.lidEnd + CHEST_TIMING.crabHopTicks + 1).hop, 0, "and lands again");

  // The reset walks him home, and lets go on the way.
  assert.equal(journey("resetting", 0).approach, 1, "he starts the reset still at the chest");
  assert.equal(journey("resetting", 0).riding, false, "but has let go");
  assert.equal(journey("resetting", CHEST_TIMING.approachTicks).approach, 0, "and ends up home");
});

test("the crab's errand cannot leak what is in the chest", () => {
  // He reacts to WHICH CHEST THE PLAYER CHOSE — the player's own input — and to
  // nothing else. The same independence the idle dance has, and it has to hold
  // here too: this crab walks up to the chest and opens it, so if his errand
  // varied with the contents it would be a tell in the most-watched moment of the
  // game. Two sessions identical but for their committed outcome must produce a
  // byte-identical journey at every tick of it.
  const config = TREASURE_CHEST_PICK.defaultConfig();
  const base = createSession(config, 1, 1, new SeededChanceResultSource(1), { choiceCount: 9, kind: "choice" });
  const tl = revealTimeline(1, false);
  const withPlan = (tierId: string, win: boolean): SessionState => ({
    ...base,
    committed: { presentationSeed: 99, reward: null, tierId, win },
    phase: "revealing",
    phaseStartTick: 0,
  });

  for (let age = 0; age < tl.total; age += 3) {
    const jackpot = crabJourney({ ...withPlan("wedding-ring", true), tick: age }, 1, tl);
    const coin = crabJourney({ ...withPlan("gold-coin", true), tick: age }, 1, tl);
    const empty = crabJourney({ ...withPlan("gold-coin", false), tick: age }, 1, tl);
    assert.deepEqual(jackpot, coin, `tier cannot change the errand (age ${age})`);
    assert.deepEqual(jackpot, empty, `winning cannot change the errand (age ${age})`);
  }
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

  // The treasure that climbs out of the parked chest also stays framed, across
  // the whole rise INCLUDING the overshoot of its ease.
  //
  // The budget is read from the catalog rather than copied out of it: every
  // prize is authored inside the same unit box at the same `PRIZE_SIZE`, and
  // `PRIZE_EXTENT` is the reach of whichever of the five is widest. So this
  // assertion re-binds itself the day a prize grows — which is exactly what a
  // framing test is for, and is why the prizes declare an `extent` at all.
  const top = v3(heroBase.x, heroBase.y + CHEST_BODY_TOP * framing.scale, heroBase.z);
  const prizeScale = framing.scale * CHEST_TIMING.prizeDamp;
  for (let step = 0; step <= 40; step += 1) {
    const riseT = step / 40;
    // 1.04 is the top of the settled size breathe in `heroPrize`.
    const reach = PRIZE_SIZE * PRIZE_EXTENT * (0.5 + 0.5 * riseT) * 1.04 * prizeScale;
    const climb = CHEST_TIMING.riseHeight * easeOutBack(riseT) * framing.scale * CHEST_TIMING.riseDamp;
    const apex = project(camera, v3(top.x, top.y + climb + reach, top.z), square);
    assert.ok(apex.y <= 1, `the prize apex stays in frame at riseT=${riseT.toFixed(2)} (y=${apex.y.toFixed(3)})`);
  }
});

// ── the prize catalog ──────────────────────────────────────────────────────────

test("every reward tier this game ships names a real treasure", () => {
  // The tier ids ARE the prize kinds — that identity is the whole binding
  // between the committed outcome and what the player sees rise out of the
  // chest. If a tier is ever renamed without its prize, this fails rather than
  // silently falling through to the rarity default.
  const tiers = TREASURE_CHEST_PICK.defaultConfig().rewardTiers;
  assert.equal(tiers.length, PRIZE_KINDS.length, "one tier per treasure");
  tiers.forEach((tier) => {
    assert.ok(PRIZE_KINDS.includes(tier.id as (typeof PRIZE_KINDS)[number]), `tier "${tier.id}" names a treasure`);
    assert.equal(prizeKindOf(tier.id, tier.rarity), tier.id, `tier "${tier.id}" resolves to its own treasure`);
    assert.ok(tier.countsAsWin, `tier "${tier.id}" pops out of the chest`);
  });
  // Every treasure is reachable — no prize is modelled but unwinnable.
  assert.deepEqual(new Set(tiers.map((t) => t.id)), new Set(PRIZE_KINDS));
});

test("an unknown tier still yields a real treasure, chosen by rarity", () => {
  // The reward ladder is editable from the Set Up panel, so a config naming
  // tiers this catalog has never heard of is a normal state, not a bug: the
  // chest must still open onto an object rather than onto nothing.
  const byRarity = RARITIES.map((rarity) => prizeKindOf("some-custom-tier", rarity));
  byRarity.forEach((kind, i) => assert.ok(PRIZE_KINDS.includes(kind), `${RARITIES[i]} falls back to a real treasure`));
  assert.equal(new Set(byRarity).size, RARITIES.length, "each rarity has its own canonical treasure");
  // A null tier id (a win with no tier recorded) is handled the same way.
  assert.ok(PRIZE_KINDS.includes(prizeKindOf(null, "common")));
});

test("every treasure is built inside the box it declares, deterministically", () => {
  const frame = { center: v3(0, 0, 0), settle: 1, size: 1, spin: QUAT_IDENTITY, tick: 0 };
  PRIZE_KINDS.forEach((kind) => {
    // Pure in the frame: same inputs → identical geometry, so a replay of a
    // round yields a byte-identical prize.
    assert.deepEqual(prizeInstances(kind, "reward", frame), prizeInstances(kind, "reward", frame), `${kind} is deterministic`);

    // Sampled across the tick, no part of any treasure may reach past the
    // `extent` it declares — that declaration is what the framing budget above
    // is computed from, so an under-reported prize would quietly break framing.
    const reach = Array.from({ length: 24 }, (_, i) =>
      prizeInstances(kind, "reward", { ...frame, tick: i * 17 }).flatMap((inst) => {
        const p = inst.transform.position;
        const s = inst.transform.scale;
        // A rotated box's corner can reach its half-diagonal from its centre,
        // whatever the rotation — the bound that holds without re-deriving each
        // part's orientation.
        return [Math.hypot(p.x, p.y, p.z) + Math.hypot(s.x, s.y, s.z) / 2];
      }),
    ).flat();
    assert.ok(Math.max(...reach) <= prizeExtentOf(kind) * ROTATION_SLACK, `${kind} stays inside its declared extent (reached ${Math.max(...reach).toFixed(2)} vs ${prizeExtentOf(kind)})`);

    // A treasure is a real object, not a stub.
    assert.ok(prizeInstances(kind, "reward", frame).length >= 4, `${kind} is actually modelled`);
  });
});

/**
 * How far past its declared `extent` a prize's corner-bound may reach.
 *
 * The check above bounds every part by `|centre| + |scale|/2` — the half-DIAGONAL
 * of its box, which is the only bound that holds without re-deriving each part's
 * orientation. That is deliberately pessimistic: a flat slab lying square to the
 * axes reaches nothing like its diagonal, and a prize made of slabs (a coin's
 * denticles, a bar's flanks, a clam's ribs) accumulates that pessimism. The
 * slack keeps the check meaningful — it still catches a prize that has genuinely
 * outgrown its declaration — without demanding every author bound a rotation
 * they never applied.
 */
const ROTATION_SLACK = 1.6;

/**
 * Crabigail's pink bow is a MIRRORED PAIR, and the plane it mirrors about is the
 * BOW's own — the one its cock defines — not crab-local vertical.
 *
 * That distinction has been this bow's one recurring defect, and every other
 * check here is blind to it: the wings were symmetric in POSITION about
 * crab-local X while being symmetric in ROLL about the cocked axis, so one
 * ribbon read as flowing out of the knot and the other as kinked back into it.
 * A mismatch of mirror PLANES cannot be retuned away by changing the angles, so
 * this asserts the mirror itself, measured in the bow's own frame.
 */
test("the crab bride's bow is a mirrored pair about the bow's own axis", () => {
  const parts = prizeInstances("crab-bride", "prize", { center: v3(0, 0, 0), settle: 1, size: 1, spin: QUAT_IDENTITY, tick: 0 });
  const partAt = (suffix: string): SceneInstance => {
    const found = parts.find((inst) => inst.key === `prize:${suffix}`);
    assert.ok(found !== undefined, `${suffix} is drawn`);
    return found;
  };
  const knot = partAt("bowknot");
  // The bow's own frame: the axis its wings step OUT along, and which way is UP
  // across it. Both come off the KNOT, which is the bow's origin by construction.
  const axis = rotateByQuat(v3(1, 0, 0), knot.transform.rotation);
  const across = rotateByQuat(v3(0, 1, 0), knot.transform.rotation);
  const inBowFrame = (v: EngineVec3): { out: number; up: number } => ({ out: dotV3(v, axis), up: dotV3(v, across) });
  const wings = [-1, 1].map((s) => partAt(`bow${s}`));

  // Position: the wings step out to equal and opposite distances ALONG the bow's
  // axis, and neither drifts off it. A nonzero `up` is exactly the old defect.
  const offsets = wings.map((w) => inBowFrame(subV3(w.transform.position, knot.transform.position)));
  assert.ok(Math.abs(offsets[0].out) > 0.02, "the wings stand clear of the knot");
  assert.ok(Math.abs(offsets[0].out + offsets[1].out) < 1e-9, "equal and opposite along the bow's axis");
  offsets.forEach((o, i) => assert.ok(Math.abs(o.up) < 1e-9, `wing ${i} sits ON the bow's axis, not above or below it`));

  // Roll: each ribbon leans out of the knot by the same amount and splays to the
  // opposite side across the bow — which is what makes the two read as one bow.
  const ribbons = wings.map((w) => inBowFrame(rotateByQuat(v3(1, 0, 0), w.transform.rotation)));
  assert.ok(Math.abs(ribbons[0].out - ribbons[1].out) < 1e-9, "both ribbons lean out by the same amount");
  assert.ok(Math.abs(ribbons[0].up) > 0.05, "the ribbons actually splay");
  assert.ok(Math.abs(ribbons[0].up + ribbons[1].up) < 1e-9, "and they splay to opposite sides");
});

/**
 * Where the result banner sits, as a fraction of frame height from the top.
 *
 * Not a guess: `#result-banner` is centred at `top: 74%` for this game
 * specifically (`styles/marquee.css`, scoped by `body[data-active-game]`), and
 * stands roughly 6% of the stage tall either side of that. This is the top edge
 * of the band it claims — the line the treasure must never cross, because
 * everything below it is chrome drawn OVER the canvas.
 */
const BANNER_TOP_FRACTION = 0.66;

/** That line in normalized screen space, where +1 is the top of frame. */
const BANNER_TOP_NDC = 1 - 2 * BANNER_TOP_FRACTION;

test("the settled treasure owns the upper frame and never sits behind the banner", () => {
  // THE composition invariant of the reveal, and the reason `heroFill`,
  // `heroDrop`, `riseHeight`, `riseDamp` and `prizeDamp` are tuned together
  // rather than one at a time: the chest is the plinth in the lower half, the
  // treasure owns the air above it, and the result banner lands on the chest's
  // body — never on the prize.
  //
  // Before this was pinned, the prize hovered in the chest's mouth at ~0.45 of
  // frame height and the banner covered it outright. A framing test that only
  // asked "is it on screen?" could not see that, because it never was off
  // screen — it was simply behind the text.
  const camera = chestCamera(9);
  const framing = heroFraming(camera);
  const heroBase = v3(framing.anchor.x, framing.anchor.y - (CHEST_HEIGHT / 2) * framing.scale, framing.anchor.z);
  const mouth = v3(heroBase.x, heroBase.y + CHEST_BODY_TOP * framing.scale, heroBase.z);
  const prizeScale = framing.scale * CHEST_TIMING.prizeDamp;
  const climb = CHEST_TIMING.riseHeight * CHEST_TIMING.riseDamp * framing.scale;

  // Measured on the SMALLEST treasure, since it is the one whose lowest point
  // hangs closest to the chest — if the coin clears, everything clears.
  const smallest = Math.min(...PRIZE_KINDS.map((kind) => prizeExtentOf(kind)));
  const lowest = project(camera, v3(mouth.x, mouth.y + climb - PRIZE_SIZE * smallest * prizeScale, mouth.z), 1);
  assert.ok(lowest.y > BANNER_TOP_NDC, `the settled treasure clears the banner band (y=${lowest.y.toFixed(3)} > ${BANNER_TOP_NDC.toFixed(2)})`);

  // And it has genuinely LEFT the chest rather than hovering in its mouth: its
  // lowest point sits clear above the open chest's rim.
  const rim = project(camera, mouth, 1);
  assert.ok(lowest.y > rim.y, `the treasure rises clear of the chest mouth (${lowest.y.toFixed(3)} > ${rim.y.toFixed(3)})`);

  // The treasure's centre sits in the UPPER half of the frame — the poster
  // composition, with the chest reading as the plinth beneath it.
  const centre = project(camera, v3(mouth.x, mouth.y + climb, mouth.z), 1);
  assert.ok(centre.y > 0, `the treasure is staged in the upper frame (y=${centre.y.toFixed(3)})`);

  // The chest, meanwhile, must NOT have crept up into the treasure's air.
  const chestTop = Math.max(...chestCorners(heroBase, framing.scale, 0, 0).map((c) => project(camera, c, 1).y));
  assert.ok(chestTop < centre.y, "the closed chest silhouette stays below the treasure");
});

test("the hero framing fills the frame without overflowing it", () => {
  const camera = chestCamera(9);
  const framing = heroFraming(camera);

  // It is genuinely a CLOSE-UP: the chest ends up far bigger ON SCREEN than any
  // chest still on the board, and much nearer to the camera than the board is.
  // The bar is 2.5× rather than the 3× this once asked for, and deliberately:
  // the reveal was recomposed so the TREASURE owns the frame and the chest
  // reads as the plinth under it (see the composition test above and the note
  // on `heroFill`). The enlargement budget moved to the prize; the chest is
  // still unmistakably a close-up, just no longer the whole subject.
  // Measured on the PROJECTION, not on `framing.scale` — a longer lens buys the
  // same shot with less world scale at more distance, so world scale alone says
  // nothing about how large the chest reads.
  assert.ok(framing.distance < Math.hypot(camera.position.y, camera.position.z) * 0.7, "the hero plane is well in front of the board");

  // It commands the frame — but the width guard keeps it inside even on a
  // square window, which is the whole point of sizing from the frustum.
  const heroBase = v3(framing.anchor.x, framing.anchor.y - (CHEST_HEIGHT / 2) * framing.scale, framing.anchor.z);
  // Measured on the projected corners, so the chest's NEAR face — which the
  // perspective enlarges past the flat width budget — is what gets checked.
  const xs = chestCorners(heroBase, framing.scale, 0, 0).map((c) => project(camera, c, 1).x);
  const span = Math.max(...xs) - Math.min(...xs);
  const boardSpan = Math.max(
    ...Array.from({ length: 9 }, (_, i) => {
      const slot = chestCorners(chestPosition(i, 9), 1, 0, 0).map((c) => project(camera, c, 1).x);
      return Math.max(...slot) - Math.min(...slot);
    }),
  );
  assert.ok(span > boardSpan * 2.5, `the hero chest is a real enlargement (${(span / boardSpan).toFixed(2)}× the widest chest on the board)`);
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

// ── draggable chests ──────────────────────────────────────────────────────────

test("a press on a chest is a CLICK until it travels — then it becomes a drag", () => {
  // The whole disambiguation, pinned. A chest is both the thing you open and the
  // thing you can pick up, so a press commits to nothing until the cursor moves:
  // release in place and it was a click (and must NOT own the pointer, or the pick
  // it was would be swallowed); travel past the threshold and it is a drag (and
  // must own the pointer, or it would land as a pick on release).
  const camera = chestCamera(9);
  const start = initialChestDrag(9);
  const home = worldToCanvas(camera, defaultChestSlots(9)[4] as EngineVec3) as { x: number; y: number };

  // A fresh press lands on chest 4 and is remembered — but owns nothing yet.
  const pressed = stepChestDrag(start, inputFrame(at(home.x, home.y, true)), camera);
  assert.equal(pressed.drag.grab?.index, 4, "the press is remembered against the chest under it");
  assert.equal(pressed.drag.grab?.dragging, false, "and has not committed to dragging");
  assert.equal(pressed.active, false, "so it does NOT own the pointer — the click can still happen");
  assert.deepEqual(pressed.drag.slots, start.slots, "and nothing has moved");

  // Jitter short of the threshold is still a click.
  const jitter = stepChestDrag(pressed.drag, inputFrame(at(home.x + DRAG_THRESHOLD_PX - 1, home.y, true)), camera);
  assert.equal(jitter.drag.grab?.dragging, false, "a wobble inside the threshold is not a drag");
  assert.equal(jitter.active, false);
  assert.deepEqual(jitter.drag.slots, start.slots, "and still nothing has moved");

  // Releasing in place ends the grab without ever owning the pointer, so the
  // choice step sees the release and opens the chest exactly as it always did.
  const clicked = stepChestDrag(jitter.drag, inputFrame(at(home.x, home.y, false)), camera);
  assert.equal(clicked.drag.grab, null, "the grab is released");
  assert.equal(clicked.active, false, "a click never owns the pointer");

  // Travelling past the threshold commits, and from then on the chest follows.
  const far = stepChestDrag(pressed.drag, inputFrame(at(home.x + 60, home.y + 30, true)), camera);
  assert.equal(far.drag.grab?.dragging, true, "past the threshold it is a drag");
  assert.equal(far.active, true, "and it owns the pointer, so no pick can land");
  assert.notDeepEqual(far.drag.slots[4], start.slots[4], "chest 4 has moved");
  assert.equal(far.drag.slots[4]?.y, 0, "and stays on the ground plane");

  // Committed stays committed: coming back to the press point does not turn a
  // drag back into a click mid-press.
  const returned = stepChestDrag(far.drag, inputFrame(at(home.x, home.y, true)), camera);
  assert.equal(returned.drag.grab?.dragging, true, "a drag that returns home is still a drag");

  // The release tick of a real drag DOES own the pointer — that is what stops it
  // being read as the click it stopped being.
  const dropped = stepChestDrag(returned.drag, inputFrame(at(home.x, home.y, false)), camera);
  assert.equal(dropped.active, true, "the drop tick owns the pointer");
  assert.equal(dropped.drag.grab, null, "and the grab is over");
});

test("only the grabbed chest moves, and it keeps the grab offset", () => {
  const camera = chestCamera(9);
  const start = initialChestDrag(9);
  // Press OFF-CENTRE on chest 0 so the offset is non-zero, then drag: the chest
  // must travel by the cursor's ground delta rather than snapping its base to it.
  const home = worldToCanvas(camera, defaultChestSlots(9)[0] as EngineVec3) as { x: number; y: number };
  const press = at(home.x + 18, home.y + 10, true);
  const grabbed = stepChestDrag(start, inputFrame(press), camera);
  const grabGround = canvasToGround(camera, press) as EngineVec3;
  const dest = worldToCanvas(camera, v3(1.5, 0, 2.5)) as { x: number; y: number };
  const moved = stepChestDrag(grabbed.drag, inputFrame(at(dest.x, dest.y, true)), camera);
  const destGround = canvasToGround(camera, at(dest.x, dest.y, true)) as EngineVec3;

  const before = start.slots[0] as EngineVec3;
  const after = moved.drag.slots[0] as EngineVec3;
  const dx = after.x - before.x;
  const dz = after.z - before.z;
  assert.ok(Math.hypot(dx - (destGround.x - grabGround.x), dz - (destGround.z - grabGround.z)) < 0.05, "the chest follows the cursor's ground delta");
  // Every other chest is untouched.
  moved.drag.slots.slice(1).forEach((slot, i) => assert.deepEqual(slot, start.slots[i + 1], `chest ${i + 1} did not move`));
  // Pure in (drag, input, camera).
  assert.deepEqual(moved, stepChestDrag(grabbed.drag, inputFrame(at(dest.x, dest.y, true)), camera));
});

test("a dragged chest carries its contents — a pick still resolves to the same chest", () => {
  // The fairness property. The population is assigned BY INDEX before the pick
  // (`winnersByIndex`), and dragging moves where a chest IS, never which index it
  // is. So a click at a moved chest's new screen position must resolve to that
  // same index — the prize travels with the chest, and no amount of rearranging
  // can shuffle contents between chests.
  const camera = chestCamera(9);
  const config = TREASURE_CHEST_PICK.defaultConfig();
  const population = planChoicePopulation(config, 9, 4242, 1);

  const start = initialChestDrag(9);
  const home = worldToCanvas(camera, defaultChestSlots(9)[6] as EngineVec3) as { x: number; y: number };
  const grabbed = stepChestDrag(start, inputFrame(at(home.x, home.y, true)), camera);
  const dest = worldToCanvas(camera, v3(-3.2, 0, 3.4)) as { x: number; y: number };
  const moved = stepChestDrag(grabbed.drag, inputFrame(at(dest.x, dest.y, true)), camera).drag;

  // Hit-testing the LIVE layout at the chest's new home finds chest 6 again.
  const hit = pickAt(camera, chestTargetsAt(moved.slots), at(dest.x, dest.y, false));
  assert.equal(hit, 6, "the moved chest is still chest 6");
  // And what chest 6 holds is untouched by the move — the population never saw it.
  assert.deepEqual(planChoicePopulation(config, 9, 4242, 1).winnersByIndex, population.winnersByIndex);
});

test("rearranged chests persist across a round reset, and re-deal when the count changes", () => {
  const config = TREASURE_CHEST_PICK.defaultConfig();
  const session = createSession(config, 1, 1, new SeededChanceResultSource(1), { choiceCount: 9, kind: "choice" });

  const first = initialChestExtra(session, null);
  assert.deepEqual(first.chests.slots, defaultChestSlots(9), "a fresh session deals the grid");

  // A prior round with a rearranged board and a live grab.
  const scattered = defaultChestSlots(9).map((slot, i) => (i === 3 ? v3(4, 0, 4) : slot));
  const prior: ChestExtra = {
    ...first,
    chests: { grab: { dragging: true, from: { x: 1, y: 2 }, index: 3, offset: v3(0, 0, 0) }, pointerDown: true, slots: scattered },
  };
  const next = initialChestExtra(session, prior);
  assert.deepEqual(next.chests.slots, scattered, "the arrangement survives the reset");
  assert.equal(next.chests.grab, null, "the transient grab does not");
  assert.equal(next.chests.pointerDown, false);

  // …but a changed chest count re-deals, because a nine-slot arrangement dealt
  // onto a six-chest board would leave chests stacked or missing.
  const sixSession = createSession({ ...config, choiceCount: 6 }, 1, 1, new SeededChanceResultSource(1), { choiceCount: 6, kind: "choice" });
  const resized = initialChestExtra(sixSession, prior);
  assert.deepEqual(resized.chests.slots, defaultChestSlots(6), "a changed count deals a fresh grid");
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
