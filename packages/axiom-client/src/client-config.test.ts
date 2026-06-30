import assert from "node:assert/strict";
import { test } from "node:test";

import { STATUS_CONNECTED, STATUS_CONNECTING, STATUS_DISCONNECTED } from "./client-config.ts";

test("status constants carry their stable wire-facing string values", () => {
  assert.equal(STATUS_DISCONNECTED, "disconnected");
  assert.equal(STATUS_CONNECTING, "connecting");
  assert.equal(STATUS_CONNECTED, "connected");
});

test("the status constants are distinct", () => {
  const all = new Set([STATUS_DISCONNECTED, STATUS_CONNECTING, STATUS_CONNECTED]);
  assert.equal(all.size, 3);
});
