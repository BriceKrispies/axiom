/*
 * tier.test.ts — `node --test` coverage for the capability ladder and its pure
 * decision. Every rule that decides what a user sees is exercised here on
 * synthetic probe records: the fall-through order, the crash-guard ceiling, the
 * degraded-but-usable outcome, the WebGPU budget skip, and the `?render=`
 * parse. No DOM, no probing, no timers.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { absentProbe } from "./branchless.ts";
import {
  FALLBACK_TIER,
  TIER_ORDER,
  TOP_TIER,
  type Tier,
  type TierOutcome,
  type TierOutcomes,
  type TierProbe,
  type TierProbes,
  ceilingAfterCrash,
  chooseTier,
  demote,
  hardwareAccelerated,
  isSelectable,
  isTier,
  ladderFrom,
  outcomesOf,
  parseTierChoice,
  rank,
  shouldProbeWebgpu,
} from "./tier.ts";

const outcomes = (patch: Partial<Record<Tier, TierOutcome>>): TierOutcomes => ({
  canvas2d: "fail",
  css3d: "fail",
  webgl1: "fail",
  webgl2: "fail",
  webgpu: "fail",
  ...patch,
});

const probe = (outcome: TierOutcome, accelerated = false): TierProbe => ({ accelerated, detail: "synthetic", outcome });

const probes = (patch: Partial<Record<Tier, TierProbe>>): TierProbes => ({
  canvas2d: probe("skipped"),
  css3d: probe("skipped"),
  webgl1: probe("skipped"),
  webgl2: probe("skipped"),
  webgpu: probe("skipped"),
  ...patch,
});

test("the ladder is webgpu -> webgl2 -> webgl1 -> canvas2d -> css3d", () => {
  assert.deepEqual(TIER_ORDER, ["webgpu", "webgl2", "webgl1", "canvas2d", "css3d"]);
  assert.equal(TOP_TIER, "webgpu");
  assert.equal(FALLBACK_TIER, "css3d");
  assert.equal(rank("webgpu"), 0);
  assert.ok(rank("webgl2") < rank("webgl1"));
  assert.ok(rank("canvas2d") < rank("css3d"));
});

test("chooseTier takes the best passing rung", () => {
  assert.equal(chooseTier(outcomes({ canvas2d: "pass", webgl2: "pass", webgpu: "pass" }), TOP_TIER), "webgpu");
  assert.equal(chooseTier(outcomes({ canvas2d: "pass", webgl2: "pass" }), TOP_TIER), "webgl2");
  assert.equal(chooseTier(outcomes({ canvas2d: "pass", webgl1: "pass" }), TOP_TIER), "webgl1");
  assert.equal(chooseTier(outcomes({ canvas2d: "pass" }), TOP_TIER), "canvas2d");
});

test("chooseTier falls all the way through to css3d when nothing passes", () => {
  assert.equal(chooseTier(outcomes({}), TOP_TIER), "css3d", "the fail-safe: css3d needs no drawing context");
  assert.equal(chooseTier(outcomes({ css3d: "pass" }), TOP_TIER), "css3d");
  assert.equal(
    chooseTier(outcomes({ canvas2d: "skipped", webgl1: "skipped", webgl2: "fail", webgpu: "skipped" }), TOP_TIER),
    "css3d",
    "a skipped rung is not a selectable rung",
  );
});

test("a degraded rung is still selectable, and outranks a lower passing one", () => {
  assert.equal(isSelectable("pass"), true);
  assert.equal(isSelectable("degraded"), true);
  assert.equal(isSelectable("fail"), false);
  assert.equal(isSelectable("skipped"), false);
  assert.equal(chooseTier(outcomes({ canvas2d: "pass", webgl2: "degraded" }), TOP_TIER), "webgl2");
});

test("the ceiling caps the ladder, no matter what passed above it", () => {
  const everything = outcomes({ canvas2d: "pass", css3d: "pass", webgl1: "pass", webgl2: "pass", webgpu: "pass" });
  assert.equal(chooseTier(everything, "webgpu"), "webgpu");
  assert.equal(chooseTier(everything, "webgl2"), "webgl2");
  assert.equal(chooseTier(everything, "canvas2d"), "canvas2d", "the crash guard wins over a passing WebGL2 probe");
  assert.deepEqual(ladderFrom("webgl1"), ["webgl1", "canvas2d", "css3d"]);
});

test("demote steps one rung down and bottoms out at the fallback", () => {
  assert.equal(demote("webgpu"), "webgl2");
  assert.equal(demote("webgl2"), "webgl1");
  assert.equal(demote("webgl1"), "canvas2d");
  assert.equal(demote("canvas2d"), "css3d");
  assert.equal(demote("css3d"), "css3d", "the terminal rung demotes to itself");
});

test("a crash sentinel caps the next boot one rung BELOW the tier that died", () => {
  assert.equal(ceilingAfterCrash(absentProbe<Tier>()), TOP_TIER, "no sentinel: the ladder is unconstrained");
  assert.equal(ceilingAfterCrash("webgpu"), "webgl2");
  assert.equal(ceilingAfterCrash("webgl2"), "webgl1");
  assert.equal(ceilingAfterCrash("css3d"), "css3d", "even a crash in the fallback cannot demote below it");
});

test("outcomesOf projects the probe record down to bare outcomes", () => {
  const record = probes({ canvas2d: probe("pass"), webgl2: probe("fail") });
  assert.deepEqual(outcomesOf(record), {
    canvas2d: "pass",
    css3d: "skipped",
    webgl1: "skipped",
    webgl2: "fail",
    webgpu: "skipped",
  });
});

test("hardwareAccelerated reports whether any probe proved a real GPU", () => {
  assert.equal(hardwareAccelerated(probes({ webgl2: probe("pass", true) })), true);
  assert.equal(hardwareAccelerated(probes({ webgl2: probe("degraded", false) })), false, "a software GL context is not acceleration");
  assert.equal(hardwareAccelerated(probes({})), false);
});

test("the WebGPU budget is spent only when acceleration was proven and the ceiling allows it", () => {
  const accelerated = probes({ webgl2: probe("pass", true) });
  assert.equal(shouldProbeWebgpu(accelerated, "webgpu"), true);
  assert.equal(shouldProbeWebgpu(accelerated, "webgl2"), false, "the crash guard already ruled webgpu out");
  assert.equal(
    shouldProbeWebgpu(probes({ webgl2: probe("degraded", false) }), "webgpu"),
    false,
    "no hardware acceleration: Chrome disables WebGPU too, so probing can only burn the timeout",
  );
});

test("isTier recognises exactly the ladder's rungs", () => {
  assert.equal(isTier("webgl2"), true);
  assert.equal(isTier("css3d"), true);
  assert.equal(isTier("css"), false, "an alias is not a tier name");
  assert.equal(isTier("auto"), false);
  assert.equal(isTier("constructor"), false);
});

test("parseTierChoice accepts the tier names, the friendly aliases, and auto", () => {
  assert.equal(parseTierChoice("auto"), "auto");
  assert.equal(parseTierChoice("webgpu"), "webgpu");
  assert.equal(parseTierChoice("WebGL2"), "webgl2", "case-insensitive");
  assert.equal(parseTierChoice("  css  "), "css3d", "trimmed, and the legacy ?backend=css spelling still works");
  assert.equal(parseTierChoice("webgl"), "webgl1");
  assert.equal(parseTierChoice("canvas"), "canvas2d");
  assert.equal(parseTierChoice("dom"), "css3d");
  assert.equal(parseTierChoice("gpu"), "webgpu");
});

test("parseTierChoice rejects nonsense rather than inventing an answer", () => {
  assert.equal(parseTierChoice("vulkan"), undefined);
  assert.equal(parseTierChoice(""), undefined);
  assert.equal(parseTierChoice(absentProbe<string>()), undefined);
  assert.equal(parseTierChoice("constructor"), undefined, "no prototype reachable through the alias table");
  assert.equal(parseTierChoice("toString"), undefined);
});
