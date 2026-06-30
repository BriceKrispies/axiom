import assert from "node:assert/strict";
import { test } from "node:test";

import {
  KIND_CLIENT_INTENT,
  KIND_CLIENT_INTENT_FOR,
  KIND_JOIN_ROOM,
  KIND_LEAVE_ROOM,
  KIND_REJECTED_INTENT,
  KIND_SERVER_EVENT,
  KIND_SERVER_SNAPSHOT,
  KIND_SERVER_SNAPSHOT_FOR,
  KIND_WELCOME,
  MAX_ACKS,
  MAX_PAYLOAD_LEN,
  MAX_ROOM_ID_LEN,
  REASON_MALFORMED,
  REASON_NOT_IN_ROOM,
  REASON_OUT_OF_ORDER,
  REASON_UNSPECIFIED,
  WIRE_MAJOR,
  WIRE_MINOR,
} from "./messages.ts";

test("the wire version constants match the Rust module", () => {
  assert.equal(WIRE_MAJOR, 1);
  assert.equal(WIRE_MINOR, 0);
});

test("the message kind discriminants are the stable 0..=8 sequence", () => {
  assert.deepEqual(
    [
      KIND_JOIN_ROOM,
      KIND_LEAVE_ROOM,
      KIND_CLIENT_INTENT,
      KIND_WELCOME,
      KIND_SERVER_SNAPSHOT,
      KIND_SERVER_EVENT,
      KIND_REJECTED_INTENT,
      KIND_CLIENT_INTENT_FOR,
      KIND_SERVER_SNAPSHOT_FOR,
    ],
    [0, 1, 2, 3, 4, 5, 6, 7, 8],
  );
});

test("the documented size bounds match the Rust module", () => {
  assert.equal(MAX_ROOM_ID_LEN, 64);
  assert.equal(MAX_PAYLOAD_LEN, 65_536);
  assert.equal(MAX_ACKS, 4096);
});

test("the reject reason codes are the stable 0..=3 sequence", () => {
  assert.deepEqual(
    [REASON_UNSPECIFIED, REASON_MALFORMED, REASON_OUT_OF_ORDER, REASON_NOT_IN_ROOM],
    [0, 1, 2, 3],
  );
});
