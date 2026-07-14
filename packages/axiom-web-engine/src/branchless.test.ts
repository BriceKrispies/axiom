/*
 * branchless.test.ts — `node --test` coverage for the shared branchless selection
 * + Option kit. Each primitive is exercised on both arms directly (present/absent,
 * in-range/out-of-range, assert pass/fail), so the toolkit the store and reconciler
 * lean on is proven in isolation rather than only through its callers.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { ABSENT, absentProbe, assert as bAssert, demand, fail, isPresent, orCompute, orElse, pick, presentOf, select } from "./branchless.ts";

test("absentProbe / ABSENT: a 0-arg call is the absent value", () => {
  assert.equal(ABSENT, undefined);
  assert.equal(absentProbe<number>(), undefined);
  assert.equal(absentProbe(5), 5);
});

test("fail throws its message", () => {
  assert.throws(() => fail("boom"), /boom/u);
});

test("assert passes on true and throws on false", () => {
  bAssert(true, "should not throw");
  assert.throws(() => {
    bAssert(false, "nope");
  }, /nope/u);
});

test("pick indexes in range and throws out of range", () => {
  assert.equal(pick(["a", "b", "c"], 1), "b");
  assert.throws(() => pick([], 0), /out of range/u);
});

test("select chooses by condition", () => {
  assert.equal(select(true, "yes", "no"), "yes");
  assert.equal(select(false, "yes", "no"), "no");
});

test("presentOf / isPresent distinguish present from absent", () => {
  assert.deepEqual(presentOf(7), [7]);
  assert.deepEqual(presentOf(absentProbe<number>()), []);
  assert.equal(isPresent(0), true);
  assert.equal(isPresent(absentProbe<number>()), false);
});

test("demand unwraps present and throws on absent", () => {
  assert.equal(demand(9, "missing"), 9);
  assert.throws(() => demand(absentProbe<number>(), "missing"), /missing/u);
});

test("orElse returns the value or the fallback", () => {
  assert.equal(orElse(3, 0), 3);
  assert.equal(orElse(absentProbe<number>(), 0), 0);
});

test("orCompute returns the value or lazily computes the fallback", () => {
  let calls = 0;
  const compute = (): number => {
    calls += 1;
    return 42;
  };
  assert.equal(orCompute(5, compute), 5);
  assert.equal(calls, 0);
  assert.equal(orCompute(absentProbe<number>(), compute), 42);
  assert.equal(calls, 1);
});
