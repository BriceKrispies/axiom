import assert from "node:assert/strict";
import { test } from "node:test";

import { AuthoringError } from "../src/authoring-error.ts";
import { each, orElse, pick, whenPresent } from "../src/control-flow.ts";

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

test("orElse keeps a present value and falls back when absent", () => {
  assert.equal(orElse("here", "fallback"), "here");
  assert.equal(orElse(undefined, "fallback"), "fallback");
  // A falsy-but-present value (0) is kept — presence, not truthiness, decides.
  assert.equal(orElse(0, 7), 0);
});

test("whenPresent runs the effect only for a present value", () => {
  const seen: string[] = [];
  whenPresent("x", (value) => {
    seen.push(value);
  });
  whenPresent(undefined, (value: string) => {
    seen.push(value);
  });
  assert.deepEqual(seen, ["x"]);
});
