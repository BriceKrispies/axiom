/*
 * rocket-launch.test.ts — the flight contract: the path ends at the committed
 * planet (reached smoothly, bounded per-tick delta, no teleport), the flight
 * duration shrinks under reduced motion, and the sub-phase order
 * countdown → liftoff → orbit → dock → reveal is preserved either way.
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import { commitOutcome, createSession, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import type { FlightPhase, RocketSpec } from "./game.ts";
import {
  committedPlanetIndex,
  destinationSlotsOf,
  dockPoint,
  flightPhaseAt,
  flightTimeline,
  ignitedLights,
  planetPosition,
  rocketPosition,
} from "./game.ts";

const spec: RocketSpec = {
  planets: [
    { label: "Gray Moon", mass: 3, tierId: null },
    { label: "Star Token", mass: 3, tierId: "common" },
    { label: "Ticket Bundle", mass: 2, tierId: "uncommon" },
    { label: "Gem Trophy", mass: 1, tierId: "rare" },
    { label: "Golden Capsule", mass: 0.4, tierId: "jackpot" },
  ],
};

const config = (): CasinoGameConfig<RocketSpec> => baseConfig("rocket-launch", "Rocket Launch", "showcase", spec, { targetWinRate: 0.4 });

const committedSession = (seed: number, round: number): SessionState => {
  const source = new SeededChanceResultSource(seed);
  let s = createSession(config(), seed, round, source, { kind: "destination", slots: destinationSlotsOf(spec) });
  s = transition(s, "ready");
  s = transition(s, "committing");
  s = commitOutcome(s, source, { launchStrength: 1 });
  return s;
};

const dist = (a: { x: number; y: number; z: number }, b: { x: number; y: number; z: number }): number =>
  Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);

test("the flight ends at the committed planet, reached smoothly with a bounded per-tick delta", () => {
  const count = spec.planets.length;
  for (let round = 0; round < 12; round += 1) {
    const session = committedSession(2718, round);
    const index = committedPlanetIndex(session);
    const seed = session.committed?.presentationSeed ?? session.seed;
    const timeline = flightTimeline(session.config.presentationSpeed, false);

    let prev = rocketPosition(0, index, count, seed, session.round, timeline);
    let maxStep = 0;
    for (let age = 1; age <= timeline.total; age += 1) {
      const p = rocketPosition(age, index, count, seed, session.round, timeline);
      maxStep = Math.max(maxStep, dist(p, prev));
      prev = p;
    }
    assert.ok(maxStep < 0.5, `round ${round}: max per-tick step ${maxStep} exceeds the continuity bound`);

    const final = rocketPosition(timeline.total, index, count, seed, session.round, timeline);
    assert.deepEqual(final, dockPoint(index, count));
    assert.ok(dist(final, planetPosition(index, count)) <= 0.6, `round ${round}: docked ${dist(final, planetPosition(index, count))} from the planet`);
  }
});

test("the flight duration shrinks under reduced motion, phase order preserved", () => {
  const full = flightTimeline(1, false);
  const reduced = flightTimeline(1, true);
  assert.ok(reduced.total < full.total, `reduced ${reduced.total} not shorter than full ${full.total}`);

  const orderOf = (timeline: ReturnType<typeof flightTimeline>): readonly FlightPhase[] => {
    const seq: FlightPhase[] = [];
    for (let age = 0; age <= timeline.total; age += 1) {
      const phase = flightPhaseAt(age, timeline);
      if (seq[seq.length - 1] !== phase) {
        seq.push(phase);
      }
    }
    return seq;
  };
  const expected: readonly FlightPhase[] = ["liftoff", "orbit", "dock", "reveal"];
  assert.deepEqual(orderOf(full), expected);
  assert.deepEqual(orderOf(reduced), expected);
});

test("the countdown builds before flight — three pad lights ignite in order", () => {
  assert.equal(ignitedLights(0), 0);
  assert.equal(ignitedLights(24), 1);
  assert.equal(ignitedLights(48), 2);
  assert.equal(ignitedLights(72), 3);
  assert.equal(ignitedLights(200), 3);
});
