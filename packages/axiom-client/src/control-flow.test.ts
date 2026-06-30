import assert from "node:assert/strict";
import { test } from "node:test";

import { each, pick } from "./control-flow.ts";
import { ProtocolError } from "./protocol-error.ts";

test("pick selects the in-range option", () => {
  assert.equal(pick(["a", "b", "c"], 0), "a");
  assert.equal(pick(["a", "b", "c"], 2), "c");
});

test("pick throws ProtocolError when the index is out of range", () => {
  assert.throws(() => pick(["a"], 1), ProtocolError);
  assert.throws(() => pick([], 0), ProtocolError);
});

test("each runs the effect once per value in order", () => {
  const seen: number[] = [];
  each([10, 20, 30], (value): void => void seen.push(value));
  assert.deepEqual(seen, [10, 20, 30]);
});

test("each over an empty array never runs the effect", () => {
  let calls = 0;
  each<number>([], (): void => void (calls += 1));
  assert.equal(calls, 0);
});
