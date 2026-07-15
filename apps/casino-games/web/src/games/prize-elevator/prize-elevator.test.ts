/*
 * prize-elevator.test.ts — the ride contract: the car's height rises monotonically
 * with a bounded per-tick delta (no teleports), stops EXACTLY at the committed
 * floor's height at reveal end, and every floor below the destination lights
 * exactly once, in ascending order.
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import { commitOutcome, createSession, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import type { ElevatorSpec } from "./game.ts";
import {
  carHeight,
  carStopHeight,
  committedFloorIndex,
  destinationSlotsOf,
  floorHeight,
  floorLit,
  rideTimeline,
} from "./game.ts";

const spec: ElevatorSpec = {
  floors: [
    { label: "Lobby", mass: 3, tierId: null },
    { label: "Workshop", mass: 3, tierId: null },
    { label: "Star Token", mass: 3, tierId: "common" },
    { label: "Ticket Bundle", mass: 2, tierId: "uncommon" },
    { label: "Gem Trophy", mass: 1, tierId: "rare" },
    { label: "Golden Capsule", mass: 0.4, tierId: "jackpot" },
  ],
};

const config = (): CasinoGameConfig<ElevatorSpec> => baseConfig("prize-elevator", "Prize Elevator", "showcase", spec, { targetWinRate: 0.4 });

const committedSession = (seed: number, round: number): SessionState => {
  const source = new SeededChanceResultSource(seed);
  let s = createSession(config(), seed, round, source, { kind: "destination", slots: destinationSlotsOf(spec) });
  s = transition(s, "ready");
  s = transition(s, "committing");
  s = commitOutcome(s, source, {});
  return s;
};

test("the car height rises with a bounded per-tick delta and stops exactly at the floor", () => {
  for (let round = 0; round < 12; round += 1) {
    const session = committedSession(31337, round);
    const target = committedFloorIndex(session);
    const timeline = rideTimeline(target, session.config.presentationSpeed, false);
    let prev = carHeight(0, target, timeline);
    let maxStep = 0;
    for (let age = 1; age <= timeline.total; age += 1) {
      const h = carHeight(age, target, timeline);
      assert.ok(h >= prev - 1e-9, `round ${round}: height dropped at age ${age} (${h} < ${prev})`);
      maxStep = Math.max(maxStep, Math.abs(h - prev));
      prev = h;
    }
    assert.ok(maxStep < 0.2, `round ${round}: max per-tick height step ${maxStep} exceeds the continuity bound`);
    const finalH = carHeight(timeline.total, target, timeline);
    assert.equal(finalH, carStopHeight(target));
    assert.equal(finalH, floorHeight(target));
  }
});

test("every floor below the destination lights exactly once, in ascending order", () => {
  for (let round = 0; round < 10; round += 1) {
    const session = committedSession(555, round);
    const target = committedFloorIndex(session);
    const timeline = rideTimeline(target, session.config.presentationSpeed, false);
    // The ground floor is where the car starts, so it is lit from age 0; every
    // floor ABOVE it (up to the destination) lights exactly once as the car passes.
    assert.ok(target === 0 || floorLit(0, 0, target, timeline), `round ${round}: ground floor not lit at the start`);
    const firstLit: number[] = [];
    for (let floor = 1; floor < target; floor += 1) {
      let transitions = 0;
      let litAt = -1;
      let was = floorLit(floor, 0, target, timeline);
      assert.equal(was, false, `round ${round}: floor ${floor} was lit before the ride`);
      for (let age = 1; age <= timeline.total; age += 1) {
        const now = floorLit(floor, age, target, timeline);
        if (now && !was) {
          transitions += 1;
          litAt = age;
        }
        was = now;
      }
      assert.equal(transitions, 1, `round ${round}: floor ${floor} lit ${transitions} times`);
      firstLit.push(litAt);
    }
    for (let i = 1; i < firstLit.length; i += 1) {
      assert.ok((firstLit[i] as number) > (firstLit[i - 1] as number), `round ${round}: floor ${i} lit before floor ${i - 1}`);
    }
    // The destination itself lights by the end.
    assert.ok(floorLit(target, timeline.total, target, timeline));
  }
});
