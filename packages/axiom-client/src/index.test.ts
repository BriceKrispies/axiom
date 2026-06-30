import assert from "node:assert/strict";
import { test } from "node:test";

import * as api from "./index.ts";

test("the barrel exports the high-level client and transports as constructors", () => {
  const classes = ["AxiomClient", "WebSocketTransport", "WebTransportTransport", "WebRtcTransport", "ProtocolError"];
  for (const name of classes) {
    assert.equal(typeof (api as Record<string, unknown>)[name], "function", `${name} should be a class`);
  }
});

test("the barrel exports the codec functions", () => {
  const fns = [
    "decodeClientIntent",
    "decodeFrame",
    "decodeJoinRoom",
    "decodeLeaveRoom",
    "decodeRejectedIntent",
    "decodeServerEvent",
    "decodeServerSnapshot",
    "decodeWelcome",
    "encodeClientIntent",
    "encodeJoinRoom",
    "encodeLeaveRoom",
    "encodeRejectedIntent",
    "encodeServerEvent",
    "encodeServerSnapshot",
    "encodeWelcome",
    "peekKind",
  ];
  for (const name of fns) {
    assert.equal(typeof (api as Record<string, unknown>)[name], "function", `${name} should be a function`);
  }
});

test("the barrel exports the per-player codec functions", () => {
  const fns = [
    "decodeClientIntentFor",
    "decodeServerSnapshotFor",
    "encodeClientIntentFor",
    "encodeServerSnapshotFor",
  ];
  for (const name of fns) {
    assert.equal(typeof (api as Record<string, unknown>)[name], "function", `${name} should be a function`);
  }
});

test("the barrel re-exports the wire kind discriminants with their stable values", () => {
  assert.equal(api.KIND_JOIN_ROOM, 0);
  assert.equal(api.KIND_LEAVE_ROOM, 1);
  assert.equal(api.KIND_CLIENT_INTENT, 2);
  assert.equal(api.KIND_WELCOME, 3);
  assert.equal(api.KIND_SERVER_SNAPSHOT, 4);
  assert.equal(api.KIND_SERVER_EVENT, 5);
  assert.equal(api.KIND_REJECTED_INTENT, 6);
  assert.equal(api.KIND_CLIENT_INTENT_FOR, 7);
  assert.equal(api.KIND_SERVER_SNAPSHOT_FOR, 8);
});

test("the barrel re-exports the size bounds and reason codes", () => {
  assert.equal(api.MAX_ROOM_ID_LEN, 64);
  assert.equal(api.MAX_PAYLOAD_LEN, 65_536);
  assert.equal(api.MAX_ACKS, 4096);
  assert.equal(api.REASON_UNSPECIFIED, 0);
  assert.equal(api.REASON_MALFORMED, 1);
  assert.equal(api.REASON_OUT_OF_ORDER, 2);
  assert.equal(api.REASON_NOT_IN_ROOM, 3);
});

test("the barrel re-exports the wire-format version", () => {
  assert.equal(api.WIRE_MAJOR, 1);
  assert.equal(api.WIRE_MINOR, 0);
});

test("the barrel exports the prediction resimulation primitive", () => {
  assert.equal(typeof api.resimulate, "function");
  // Sanity: the exported primitive is the pure fold (identity over no intents).
  assert.equal(api.resimulate(5, [], (state: number): number => state + 1), 5);
});
