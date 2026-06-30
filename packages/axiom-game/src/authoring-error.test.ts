import assert from "node:assert/strict";
import { test } from "node:test";

import { AuthoringError, assert as branchlessAssert, fail } from "./authoring-error.ts";

test("AuthoringError carries its message and a stable name", () => {
  const error = new AuthoringError("bad index");
  assert.ok(error instanceof Error);
  assert.equal(error.name, "AuthoringError");
  assert.equal(error.message, "bad index");
});

test("fail always throws an AuthoringError carrying the message", () => {
  assert.throws(() => fail("nope"), new AuthoringError("nope"));
});

test("assert is a no-op when the condition holds", () => {
  assert.doesNotThrow(() => {
    branchlessAssert(true, "unreachable");
  });
});

test("assert throws an AuthoringError carrying the message when the condition fails", () => {
  assert.throws(() => {
    branchlessAssert(false, "boom");
  }, new AuthoringError("boom"));
});
