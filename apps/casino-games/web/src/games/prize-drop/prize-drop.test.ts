/*
 * prize-drop.test.ts — the fairness/continuity contract for the pachinko fall:
 * the token descends on a CONTINUOUS path (bounded per-tick displacement, never
 * a final-frame snap), lands EXACTLY inside the committed slot's x-range, and is
 * a pure function of (seed, config, drop) so a replay is byte-identical.
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import { commitOutcome, createSession, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import type { DropSpec } from "./game.ts";
import {
  committedSlotIndex,
  committedSlotRanges,
  destinationSlotsOf,
  dropTimeline,
  dropWorldX,
  fallProgress,
  tokenPathX,
} from "./game.ts";

const spec: DropSpec = {
  slots: [
    { label: "Miss", mass: 3, tierId: null },
    { label: "Star Token", mass: 3, tierId: "common" },
    { label: "Miss", mass: 2, tierId: null },
    { label: "Ticket Bundle", mass: 2, tierId: "uncommon" },
    { label: "Golden Capsule", mass: 0.4, tierId: "jackpot" },
    { label: "Gem Trophy", mass: 1, tierId: "rare" },
    { label: "Miss", mass: 3, tierId: null },
  ],
};

const config = (): CasinoGameConfig<DropSpec> => baseConfig("prize-drop", "Prize Drop", "showcase", spec, { targetWinRate: 0.45 });

/** Commit a round with the given drop position and return the settled session. */
const committedSession = (seed: number, round: number, dropPosition: number): SessionState => {
  const source = new SeededChanceResultSource(seed);
  let s = createSession(config(), seed, round, source, { kind: "destination", slots: destinationSlotsOf(spec) });
  s = transition(s, "ready");
  s = transition(s, "committing");
  s = commitOutcome(s, source, { dropPosition });
  return s;
};

/** The full per-tick world-x fall path for a committed session. */
const fallPath = (session: SessionState, dropPosition: number): readonly number[] => {
  const ranges = committedSlotRanges(spec, session.config.targetWinRate);
  const center = ranges[committedSlotIndex(session)]?.center ?? 0;
  const seed = session.committed?.presentationSeed ?? session.seed;
  const timeline = dropTimeline(session.config.presentationSpeed, false);
  const dropX = dropWorldX(dropPosition);
  return Array.from({ length: timeline.total + 1 }, (_, age) =>
    tokenPathX(dropX, center, fallProgress(age, timeline), seed, session.round),
  );
};

test("the token path is continuous — bounded per-tick displacement, no final-frame snap", () => {
  for (let round = 0; round < 8; round += 1) {
    const session = committedSession(4242, round, 0.3 + round * 0.05);
    const path = fallPath(session, 0.3 + round * 0.05);
    let maxStep = 0;
    for (let i = 1; i < path.length; i += 1) {
      maxStep = Math.max(maxStep, Math.abs((path[i] as number) - (path[i - 1] as number)));
    }
    assert.ok(maxStep < 0.4, `round ${round}: max per-tick x step ${maxStep} exceeds the continuity bound`);
    // The last real move before rest is also small — the true "no snap" guarantee.
    const timeline = dropTimeline(session.config.presentationSpeed, false);
    const nearEnd = Math.abs((path[timeline.fall] as number) - (path[timeline.fall - 1] as number));
    assert.ok(nearEnd < 0.4, `round ${round}: final-approach step ${nearEnd} looks like a snap`);
  }
});

test("the token lands inside the committed slot's x-range", () => {
  for (let round = 0; round < 12; round += 1) {
    const drop = 0.15 + (round % 5) * 0.17;
    const session = committedSession(9001, round, drop);
    const ranges = committedSlotRanges(spec, session.config.targetWinRate);
    const slot = ranges[committedSlotIndex(session)];
    assert.ok(slot !== undefined);
    const path = fallPath(session, drop);
    const finalX = path[path.length - 1] as number;
    assert.ok(finalX >= slot.start - 1e-9 && finalX <= slot.end + 1e-9, `round ${round}: final x ${finalX} outside [${slot.start}, ${slot.end}]`);
  }
});

test("same seed + config + drop replays a byte-identical path", () => {
  const a = fallPath(committedSession(7777, 3, 0.62), 0.62);
  const b = fallPath(committedSession(7777, 3, 0.62), 0.62);
  assert.deepEqual(a, b);
});
