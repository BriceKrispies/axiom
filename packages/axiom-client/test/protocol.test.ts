import { test } from "node:test";
import assert from "node:assert/strict";

import {
  decodeClientIntent,
  decodeFrame,
  decodeJoinRoom,
  decodeLeaveRoom,
  decodeRejectedIntent,
  decodeServerEvent,
  decodeServerSnapshot,
  decodeWelcome,
  encodeClientIntent,
  encodeJoinRoom,
  encodeLeaveRoom,
  encodeRejectedIntent,
  encodeServerEvent,
  encodeServerSnapshot,
  encodeWelcome,
  KIND_CLIENT_INTENT,
  KIND_JOIN_ROOM,
  KIND_LEAVE_ROOM,
  KIND_REJECTED_INTENT,
  KIND_SERVER_EVENT,
  KIND_SERVER_SNAPSHOT,
  KIND_WELCOME,
  MAX_PAYLOAD_LEN,
  peekKind,
  ProtocolError,
  REASON_OUT_OF_ORDER,
} from "../src/index.ts";

const u8 = (...bytes: number[]) => Uint8Array.from(bytes);

test("the protocol encoder/decoder round-trips every message type", () => {
  const joinRoom = encodeJoinRoom(1, u8(108, 111, 98), u8(9, 9));
  assert.equal(peekKind(joinRoom), KIND_JOIN_ROOM);
  const jr = decodeJoinRoom(joinRoom);
  assert.equal(jr.protocolVersion, 1);
  assert.deepEqual(jr.roomId, u8(108, 111, 98));
  assert.deepEqual(jr.token, u8(9, 9));

  const leaveRoom = encodeLeaveRoom(u8(108, 111, 98));
  assert.equal(peekKind(leaveRoom), KIND_LEAVE_ROOM);
  assert.deepEqual(decodeLeaveRoom(leaveRoom).roomId, u8(108, 111, 98));

  const clientIntent = encodeClientIntent(5, 100, 98, u8(1, 2, 3));
  assert.equal(peekKind(clientIntent), KIND_CLIENT_INTENT);
  const ci = decodeClientIntent(clientIntent);
  assert.deepEqual(
    [ci.clientSequence, ci.predictedClientTick, ci.lastSeenServerTick],
    [5, 100, 98],
  );
  assert.deepEqual(ci.payload, u8(1, 2, 3));

  const welcome = encodeWelcome(1, 77, 42, 16_666_667);
  assert.equal(peekKind(welcome), KIND_WELCOME);
  const w = decodeWelcome(welcome);
  assert.deepEqual([w.protocolVersion, w.clientId, w.serverTick, w.fixedStepNs], [1, 77, 42, 16_666_667]);

  const snapshot = encodeServerSnapshot(42, 5, u8(7, 7));
  assert.equal(peekKind(snapshot), KIND_SERVER_SNAPSHOT);
  const s = decodeServerSnapshot(snapshot);
  assert.deepEqual([s.serverTick, s.lastAcceptedClientSequence], [42, 5]);
  assert.deepEqual(s.payload, u8(7, 7));

  const event = encodeServerEvent(9, u8(3));
  assert.equal(peekKind(event), KIND_SERVER_EVENT);
  const e = decodeServerEvent(event);
  assert.equal(e.serverTick, 9);
  assert.deepEqual(e.payload, u8(3));

  const rejected = encodeRejectedIntent(5, REASON_OUT_OF_ORDER);
  assert.equal(peekKind(rejected), KIND_REJECTED_INTENT);
  const rj = decodeRejectedIntent(rejected);
  assert.deepEqual([rj.clientSequence, rj.reasonCode], [5, REASON_OUT_OF_ORDER]);
});

test("decodeFrame dispatches to the right message", () => {
  const welcome = encodeWelcome(1, 1, 0, 1);
  const decoded = decodeFrame(welcome);
  assert.equal(decoded.kind, KIND_WELCOME);
});

test("a truncated inbound payload is rejected", () => {
  const full = encodeWelcome(1, 77, 42, 16_666_667);
  for (let k = 0; k < full.length; k++) {
    assert.throws(() => decodeWelcome(full.slice(0, k)), ProtocolError, `prefix ${k} must throw`);
  }
  // The complete frame still decodes.
  assert.equal(decodeWelcome(full).clientId, 77);
});

test("an unknown inbound message kind is rejected", () => {
  // A valid version header but a kind byte past the known range.
  const bytes = u8(1, 0, 0, 0, 99);
  assert.throws(() => peekKind(bytes), ProtocolError);
  assert.throws(() => decodeFrame(bytes), ProtocolError);
});

test("decoding the wrong kind is rejected", () => {
  const welcome = encodeWelcome(1, 1, 0, 1);
  assert.throws(() => decodeClientIntent(welcome), ProtocolError);
});

test("encoders reject invalid fields", () => {
  assert.throws(() => encodeJoinRoom(0, u8(1), u8()), ProtocolError);
  assert.throws(() => encodeLeaveRoom(u8()), ProtocolError);
  assert.throws(() => encodeWelcome(1, 0, 0, 1), ProtocolError);
  assert.throws(() => encodeWelcome(1, 1, 0, 0), ProtocolError);
  const tooBig = new Uint8Array(MAX_PAYLOAD_LEN + 1);
  assert.throws(() => encodeServerEvent(0, tooBig), ProtocolError);
});

test("encodes to the cross-language golden bytes (matches the Rust module)", () => {
  // The same vector is asserted by axiom-net-protocol's Rust test
  // `encodes_to_the_cross_language_golden_bytes`, proving the two codecs agree:
  // version major=1, minor=0, kind=6, client_sequence=5 (u64 LE),
  // reason_code=2 (u32 LE).
  const bytes = encodeRejectedIntent(5, REASON_OUT_OF_ORDER);
  assert.deepEqual(
    Array.from(bytes),
    [1, 0, 0, 0, 6, 5, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0],
  );
});

test("an incompatible wire major is rejected", () => {
  const welcome = encodeWelcome(1, 1, 0, 1);
  const tampered = welcome.slice();
  tampered[0] = 2; // bump the major
  assert.throws(() => peekKind(tampered), ProtocolError);
});
