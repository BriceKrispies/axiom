import assert from "node:assert/strict";
import { test } from "node:test";

import { AuthoringError, assert as branchlessAssert, fail } from "../src/authoring-error.ts";

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
