import assert from "node:assert/strict";
import { test } from "node:test";

import { assert as protocolAssert, fail, ProtocolError } from "./protocol-error.ts";

test("ProtocolError carries its message and name and is an Error", () => {
  const error = new ProtocolError("boom");
  assert.ok(error instanceof Error);
  assert.ok(error instanceof ProtocolError);
  assert.equal(error.name, "ProtocolError");
  assert.equal(error.message, "boom");
});

test("fail always throws a ProtocolError with the given message", () => {
  assert.throws(
    () => fail("explode"),
    (error: unknown) => error instanceof ProtocolError && error.message === "explode",
  );
});

test("assert does not throw when the condition holds", () => {
  assert.doesNotThrow(() => {
    protocolAssert(true, "should not throw");
  });
});

test("assert throws a ProtocolError with the message when the condition is false", () => {
  assert.throws(
    () => {
      protocolAssert(false, "condition failed");
    },
    (error: unknown) => error instanceof ProtocolError && error.message === "condition failed",
  );
});
