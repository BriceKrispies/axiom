import assert from "node:assert/strict";
import { test } from "node:test";

import { AuthoringError, assert as branchlessAssert, each, fail, pick } from "../src/branchless.ts";

test("each runs the effect for every value in order", () => {
  const seen: number[] = [];
  each([1, 2, 3], (value) => {
    seen.push(value * 2);
  });
  assert.deepEqual(seen, [2, 4, 6]);
});

test("pick selects the in-range element", () => {
  assert.equal(pick(["a", "b", "c"], 1), "b");
});

test("pick throws an AuthoringError when the index is out of range", () => {
  assert.throws(() => pick(["a"], 3), AuthoringError);
});

test("assert is a no-op when the condition holds and throws when it fails", () => {
  assert.doesNotThrow(() => {
    branchlessAssert(true, "unreachable");
  });
  assert.throws(() => {
    branchlessAssert(false, "boom");
  }, new AuthoringError("boom"));
});

test("fail always throws an AuthoringError carrying the message", () => {
  assert.throws(() => fail("nope"), new AuthoringError("nope"));
});
