/*
 * polish.test.ts — deterministic tests for the presentation layer (`polish.ts`
 * + the session's event emission): bounded reaction amplitudes, decays that
 * reach exactly zero, swish-vs-make net selection, streak presentation levels,
 * pooled/bounded trails, impact-audio mapping, collision cooldowns, restart
 * cleanup, station labels, golden-ball gating, focus-loss safety, and the HUD
 * count-up. Runs under `node --test` with no DOM.
 *
 *   node --test apps/axiom-three-point/web/src/polish.test.ts
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";

import { BALLS_PER_RACK, POLISH_TUNING as P, RACK_LABELS } from "./constants.ts";
import { RISE_START_TICKS } from "./gameplay.ts";
import { SHOT_TUNING } from "./constants.ts";
import { Impulse, PolishState, countTowards, glintOn, impactNorm, impactPitch, impactVolume, streakPresentationLevel } from "./polish.ts";
import { InputState } from "./engine/input.ts";
import { type GameEvent, type Intent, IDLE_INTENT } from "./types.ts";
import { ThreePointSession } from "./session.ts";

const intent = (over: Partial<Intent>): Intent => ({ ...IDLE_INTENT, ...over });

const madeEvent = (swish: boolean, streak = 1): GameEvent => ({
  entryX: swish ? 0 : 0.2,
  entryZ: 0,
  golden: false,
  kind: "basketMade",
  points: 3,
  streak,
  swish,
});

// ── 1–2. impact strength maps into bounded amplitude / volume / pitch ─────────

test("impact strength maps into bounded reaction amplitude", () => {
  const polish = new PolishState();
  polish.onEvent({ kind: "rimHit", position: { x: 0, y: 3, z: 0 }, speed: 1000 });
  for (let t = 0; t < P.rimVibrationTicks; t += 1) {
    const o = polish.rimOffset();
    assert.ok(Math.abs(o.x) <= P.rimVibrationStrength + 1e-12, "rim x within tuning strength");
    assert.ok(Math.abs(o.y) <= P.rimVibrationStrength + 1e-12, "rim y within tuning strength");
    polish.advance();
  }
  polish.onEvent({ kind: "backboardHit", position: { x: 0, y: 3.5, z: -0.5 }, speed: 999 });
  const b = polish.boardOffset();
  assert.ok(Math.abs(b.z) <= P.backboardShakeStrength + 1e-12, "board z within tuning strength");
});

test("impact strength maps into bounded audio volume and pitch", () => {
  assert.equal(impactNorm(0), 0);
  assert.equal(impactNorm(P.impactSpeedFull * 5), 1);
  assert.equal(impactVolume(0), P.minImpactVolume);
  assert.equal(impactVolume(1e9), P.maxImpactVolume);
  assert.equal(impactPitch(0), P.minImpactPitch);
  assert.equal(impactPitch(1e9), P.maxImpactPitch);
  // Monotonic in between.
  assert.ok(impactVolume(2) < impactVolume(6));
  assert.ok(impactPitch(2) < impactPitch(6));
});

// ── 3–4. vibrations decay to exactly zero ─────────────────────────────────────

test("rim vibration decays to zero", () => {
  const polish = new PolishState();
  polish.onEvent({ kind: "rimHit", position: { x: 0, y: 3, z: 0 }, speed: 6 });
  let sawMotion = false;
  for (let t = 0; t < P.rimVibrationTicks; t += 1) {
    if (Math.abs(polish.rimOffset().x) > 1e-6) sawMotion = true;
    polish.advance();
  }
  assert.ok(sawMotion, "the rim visibly vibrated");
  assert.deepEqual(polish.rimOffset(), { x: 0, y: 0, z: 0 }, "exactly at rest afterwards");
});

test("backboard shake decays to zero", () => {
  const polish = new PolishState();
  polish.onEvent({ kind: "backboardHit", position: { x: 0, y: 3.5, z: -0.5 }, speed: 6 });
  for (let t = 0; t < P.backboardShakeTicks; t += 1) polish.advance();
  const b = polish.boardOffset();
  assert.equal(b.x, 0);
  assert.equal(b.z, 0);
});

// ── 5–6. net reaction ─────────────────────────────────────────────────────────

test("net reaction returns to rest", () => {
  const polish = new PolishState();
  polish.onEvent(madeEvent(false));
  let peak = 0;
  for (let t = 0; t < P.netSnapTicks + P.netSwayTicks; t += 1) {
    peak = Math.max(peak, polish.net().drop);
    polish.advance();
  }
  assert.ok(peak > 0.3, "the net visibly reacted");
  assert.deepEqual(polish.net(), { drop: 0, flare: 0, lateralX: 0, lateralZ: 0 });
});

test("swish and rimmed makes select different net reactions", () => {
  const swish = new PolishState();
  swish.onEvent(madeEvent(true));
  const make = new PolishState();
  make.onEvent(madeEvent(false));
  for (let t = 0; t < P.netSnapTicks; t += 1) {
    swish.advance();
    make.advance();
  }
  const s = swish.net();
  const m = make.net();
  assert.ok(s.drop > m.drop, "a swish snaps down more sharply");
  assert.equal(s.lateralX, 0, "a swish is clean and vertical");
  assert.ok(Math.abs(m.lateralX) > 0, "a rimmed make displaces sideways");
  assert.ok(m.flare > s.flare, "a rimmed make flares the mouth wider");
});

// ── 7–8. streak presentation ──────────────────────────────────────────────────

test("streak presentation level changes at streaks 2, 3, and 4", () => {
  assert.equal(streakPresentationLevel(0), 0);
  assert.equal(streakPresentationLevel(1), 0);
  assert.equal(streakPresentationLevel(2), 1);
  assert.equal(streakPresentationLevel(3), 2);
  assert.equal(streakPresentationLevel(4), 3);
  assert.equal(streakPresentationLevel(9), 3);
});

test("streak loss clears all persistent streak presentation", () => {
  const polish = new PolishState();
  polish.onEvent({ kind: "streakIncreased", streak: 4 });
  for (let t = 0; t < 120; t += 1) polish.advance();
  assert.ok(polish.glow() > 0.9, "the glow ramped in at streak 4");
  polish.onEvent({ hadStreak: 4, kind: "streakBroken" });
  for (let t = 0; t < 200; t += 1) polish.advance();
  assert.equal(polish.glow(), 0, "the glow fully clears after the break");
});

// ── 9–10. pooled/bounded transients ───────────────────────────────────────────

test("trail history never exceeds its configured cap", () => {
  const s = new ThreePointSession();
  // Reach the golden fifth ball, then launch everything with real physics.
  const holdTicks = RISE_START_TICKS + Math.round(0.64 * SHOT_TUNING.shotRiseTicks);
  for (let shot = 0; shot < BALLS_PER_RACK; shot += 1) {
    for (let i = 0; i < 3000 && !(s.phase === "ready" && s.ballInHand); i += 1) s.advance(IDLE_INTENT);
    s.advance(intent({ shootHeld: true, shootPressed: true }));
    for (let i = 1; i < holdTicks - 1; i += 1) s.advance(intent({ shootHeld: true }));
    s.advance(intent({ shootReleased: true }));
    for (let i = 0; i < 40; i += 1) {
      s.advance(IDLE_INTENT);
      for (const ball of s.view().flying) {
        const cap = ball.golden ? P.goldenTrailSamples : P.ballTrailSamples;
        assert.ok(ball.trail.length <= cap, `trail ${ball.trail.length} within cap ${cap}`);
      }
    }
  }
});

test("particle pools never exceed their configured caps", () => {
  // The scene's two trail pools partition the one particle budget.
  assert.ok(P.goldenTrailSamples + P.ballTrailSamples <= P.maxPooledParticles);
  // Squash bookkeeping is hard-capped no matter how many floor hits arrive.
  const polish = new PolishState();
  for (let seq = 0; seq < 40; seq += 1) {
    polish.onEvent({ kind: "floorHit", seq, speed: 9 });
  }
  assert.ok(polish.activeEffects() <= 8 + 1, "squash entries are bounded");
});

// ── 11. restart clears every transient polish state ───────────────────────────

test("restart clears every transient polish state", () => {
  const polish = new PolishState();
  polish.onEvent({ kind: "rimHit", position: { x: 0, y: 3, z: 0 }, speed: 8 });
  polish.onEvent({ kind: "backboardHit", position: { x: 0, y: 3.5, z: -0.5 }, speed: 8 });
  polish.onEvent(madeEvent(true, 4));
  polish.onEvent({ kind: "streakIncreased", streak: 4 });
  polish.onEvent({ kind: "stationTransitionCompleted", final: false, label: "CENTER RACK", station: 1 });
  polish.onEvent({ kind: "floorHit", seq: 0, speed: 9 });
  polish.onEvent({ kind: "ballReleased", progress: 0.6 });
  polish.reset();
  assert.equal(polish.activeEffects(), 0);
  assert.deepEqual(polish.rimOffset(), { x: 0, y: 0, z: 0 });
  assert.deepEqual(polish.net(), { drop: 0, flare: 0, lateralX: 0, lateralZ: 0 });
  assert.equal(polish.glow(), 0);
  assert.equal(polish.kickRecoil(), 0);
  assert.equal(polish.rackDip(), 0);
  assert.equal(polish.award(), null);
  assert.equal(polish.stationLabel(), null);
  assert.equal(polish.squash(0), 1);
});

// ── 12. station labels only for center and right racks ────────────────────────

test("station labels appear only for center and right racks", () => {
  assert.equal(RACK_LABELS[0], null);
  assert.equal(RACK_LABELS[1], "CENTER RACK");
  assert.equal(RACK_LABELS[2], "RIGHT RACK");
  const s = new ThreePointSession();
  assert.equal(s.hud().stationLabel, null, "no label at the initial left spawn");
  // Play through rack 1 with taps and ride the glide to the center rack.
  for (let shot = 0; shot < BALLS_PER_RACK; shot += 1) {
    for (let i = 0; i < 3000 && !(s.phase === "ready" && s.ballInHand); i += 1) s.advance(IDLE_INTENT);
    s.advance(intent({ shootHeld: true, shootPressed: true }));
    s.advance(intent({ shootReleased: true }));
  }
  let seen: string | null = null;
  for (let i = 0; i < 4000 && seen === null; i += 1) {
    s.advance(IDLE_INTENT);
    seen = s.hud().stationLabel;
  }
  assert.equal(seen, "CENTER RACK");
});

// ── 13. golden-ball effects gate on the fifth ball ────────────────────────────

test("golden-ball effects apply only to each rack's fifth ball", () => {
  const s = new ThreePointSession();
  // First (ordinary) ball: never glints, no matter the glint window.
  for (let i = 0; i < 200; i += 1) {
    s.advance(IDLE_INTENT);
    const held = s.view().heldBall;
    if (held !== null) {
      assert.equal(held.golden, false);
      assert.equal(held.glint, false);
    }
  }
  // The window function itself blinks deterministically.
  assert.equal(glintOn(0), true);
  assert.equal(glintOn(50), false);
  assert.equal(glintOn(90), true);
});

// ── 14. collision cooldown suppresses duplicate contact spam ──────────────────

test("collision sound cooldown suppresses duplicate contact spam", () => {
  const s = new ThreePointSession();
  // A short rim-out (progress below the window) rattles the rim.
  for (let i = 0; i < 3000 && !(s.phase === "ready" && s.ballInHand); i += 1) s.advance(IDLE_INTENT);
  const holdTicks = RISE_START_TICKS + Math.round(0.55 * SHOT_TUNING.shotRiseTicks);
  s.advance(intent({ shootHeld: true, shootPressed: true }));
  for (let i = 1; i < holdTicks - 1; i += 1) s.advance(intent({ shootHeld: true }));
  s.advance(intent({ shootReleased: true }));
  const rimTicks: number[] = [];
  for (let t = 0; t < 400; t += 1) {
    s.advance(IDLE_INTENT);
    for (const event of s.drainGameEvents()) {
      if (event.kind === "rimHit") rimTicks.push(t);
    }
  }
  assert.ok(rimTicks.length >= 1, "the rattle produced rim events");
  for (let i = 1; i < rimTicks.length; i += 1) {
    assert.ok(rimTicks[i]! - rimTicks[i - 1]! >= P.impactCooldownTicks, "rim events respect the cooldown");
  }
});

// ── 15–16. focus loss + held restart safety ───────────────────────────────────

test("losing focus cannot create a delayed accidental shot", () => {
  const input = new InputState();
  input.bindAction("shoot", ["Space"]);
  input.keyEvent("Space", true);
  input.beginTick();
  assert.equal(input.pressed("shoot"), true);
  // Blur mid-charge: everything releases; the charge resolves as a normal
  // release edge on the NEXT tick, not at some later surprise moment.
  input.releaseAllKeys();
  input.beginTick();
  assert.equal(input.released("shoot"), true, "the held key resolves immediately");
  input.beginTick();
  assert.equal(input.pressed("shoot"), false);
  assert.equal(input.released("shoot"), false, "no delayed edges after refocus");
  assert.equal(input.isDown("shoot"), false, "nothing stays logically held");
});

test("held R cannot trigger repeated restarts", () => {
  const input = new InputState();
  input.bindAction("restart", ["KeyR"]);
  input.keyEvent("KeyR", true);
  input.beginTick();
  assert.equal(input.pressed("restart"), true);
  for (let i = 0; i < 100; i += 1) {
    input.keyEvent("KeyR", true); // browser auto-repeat
    input.beginTick();
    assert.equal(input.pressed("restart"), false, "auto-repeat never re-fires the edge");
  }
});

// ── 17–18. count-up exactness + determinism ───────────────────────────────────

test("score animation reaches the authoritative score exactly", () => {
  for (const [from, to] of [
    [0, 3],
    [0, 93],
    [45, 46],
    [10, 0],
    [7, 7],
  ] as const) {
    let value = from;
    for (let i = 0; i < 200 && value !== to; i += 1) value = countTowards(value, to);
    assert.equal(value, to, `count-up lands exactly on ${to}`);
  }
});

test("deterministic animation curves produce identical results for identical inputs", () => {
  const run = (): number[] => {
    const polish = new PolishState();
    polish.onEvent({ kind: "rimHit", position: { x: 0, y: 3, z: 0 }, speed: 5.3 });
    polish.onEvent(madeEvent(false, 2));
    polish.onEvent({ kind: "ballReleased", progress: 0.64 });
    const series: number[] = [];
    for (let t = 0; t < 60; t += 1) {
      series.push(polish.rimOffset().x, polish.net().drop, polish.kickRecoil(), polish.crowdPulse());
      polish.advance();
    }
    return series;
  };
  assert.deepEqual(run(), run());
  const i1 = new Impulse();
  const i2 = new Impulse();
  i1.fire(0.5, 10);
  i2.fire(0.5, 10);
  for (let t = 0; t < 12; t += 1) {
    assert.equal(i1.oscillation(2.5), i2.oscillation(2.5));
    i1.advance();
    i2.advance();
  }
});
