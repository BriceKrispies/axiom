import assert from "node:assert/strict";
import { test } from "node:test";

import {
  decodeClientIntent,
  decodeClientIntentFor,
  decodeFrame,
  decodeJoinRoom,
  decodeLeaveRoom,
  decodeRejectedIntent,
  decodeServerEvent,
  decodeServerSnapshot,
  decodeServerSnapshotFor,
  decodeWelcome,
  encodeClientIntent,
  encodeClientIntentFor,
  encodeJoinRoom,
  encodeLeaveRoom,
  encodeRejectedIntent,
  encodeServerEvent,
  encodeServerSnapshot,
  encodeServerSnapshotFor,
  encodeWelcome,
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
  peekKind,
  type PlayerAck,
  ProtocolError,
  REASON_OUT_OF_ORDER,
} from "../src/index.ts";

const u8 = (...bytes: number[]): Uint8Array => Uint8Array.from(bytes);

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

  const clientIntent = encodeClientIntent({
    clientSequence: 5,
    lastSeenServerTick: 98,
    payload: u8(1, 2, 3),
    predictedClientTick: 100,
  });
  assert.equal(peekKind(clientIntent), KIND_CLIENT_INTENT);
  const ci = decodeClientIntent(clientIntent);
  assert.deepEqual([ci.clientSequence, ci.predictedClientTick, ci.lastSeenServerTick], [5, 100, 98]);
  assert.deepEqual(ci.payload, u8(1, 2, 3));

  const welcome = encodeWelcome({ clientId: 77, fixedStepNs: 16_666_667, protocolVersion: 1, serverTick: 42 });
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

test("decodeFrame dispatches every kind to the right decoder", () => {
  const frames = [
    encodeJoinRoom(1, u8(1), u8()),
    encodeLeaveRoom(u8(1)),
    encodeClientIntent({ clientSequence: 1, lastSeenServerTick: 0, payload: u8(), predictedClientTick: 0 }),
    encodeWelcome({ clientId: 1, fixedStepNs: 1, protocolVersion: 1, serverTick: 0 }),
    encodeServerSnapshot(0, 0, u8()),
    encodeServerEvent(0, u8()),
    encodeRejectedIntent(0, 0),
  ];
  const kinds = frames.map((frame) => decodeFrame(frame).kind);
  assert.deepEqual(kinds, [
    KIND_JOIN_ROOM,
    KIND_LEAVE_ROOM,
    KIND_CLIENT_INTENT,
    KIND_WELCOME,
    KIND_SERVER_SNAPSHOT,
    KIND_SERVER_EVENT,
    KIND_REJECTED_INTENT,
  ]);
});

test("a truncated inbound payload is rejected at every prefix", () => {
  const full = encodeWelcome({ clientId: 77, fixedStepNs: 16_666_667, protocolVersion: 1, serverTick: 42 });
  for (let k = 0; k < full.length; k++) {
    assert.throws(() => decodeWelcome(full.slice(0, k)), ProtocolError, `prefix ${k} must throw`);
  }
  assert.equal(decodeWelcome(full).clientId, 77);
});

test("an unknown inbound message kind is rejected", () => {
  const bytes = u8(1, 0, 0, 0, 99);
  assert.throws(() => peekKind(bytes), ProtocolError);
  assert.throws(() => decodeFrame(bytes), ProtocolError);
});

test("decoding the wrong kind is rejected", () => {
  const welcome = encodeWelcome({ clientId: 1, fixedStepNs: 1, protocolVersion: 1, serverTick: 0 });
  assert.throws(() => decodeClientIntent(welcome), ProtocolError);
});

test("encoders and decoders reject invalid fields", () => {
  assert.throws(() => encodeJoinRoom(0, u8(1), u8()), ProtocolError);
  assert.throws(() => encodeJoinRoom(1, u8(), u8()), ProtocolError);
  assert.throws(() => encodeJoinRoom(1, new Uint8Array(MAX_ROOM_ID_LEN + 1), u8()), ProtocolError);
  assert.throws(() => encodeLeaveRoom(u8()), ProtocolError);
  assert.throws(
    () => encodeWelcome({ clientId: 0, fixedStepNs: 1, protocolVersion: 1, serverTick: 0 }),
    ProtocolError,
  );
  assert.throws(
    () => encodeWelcome({ clientId: 1, fixedStepNs: 0, protocolVersion: 1, serverTick: 0 }),
    ProtocolError,
  );
  const tooBig = new Uint8Array(MAX_PAYLOAD_LEN + 1);
  assert.throws(() => encodeServerEvent(0, tooBig), ProtocolError);
  // A decoder that reads a too-large payload also rejects it.
  const eventWithBigPayload = encodeServerEvent(0, new Uint8Array(MAX_PAYLOAD_LEN));
  assert.equal(decodeServerEvent(eventWithBigPayload).payload.length, MAX_PAYLOAD_LEN);
});

test("encodes to the cross-language golden bytes (matches the Rust module)", () => {
  const bytes = encodeRejectedIntent(5, REASON_OUT_OF_ORDER);
  assert.deepEqual(Array.from(bytes), [1, 0, 0, 0, 6, 5, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0]);
});

test("an incompatible wire major is rejected", () => {
  const welcome = encodeWelcome({ clientId: 1, fixedStepNs: 1, protocolVersion: 1, serverTick: 0 });
  const tampered = welcome.slice();
  tampered[0] = 2;
  assert.throws(() => peekKind(tampered), ProtocolError);
});

// --- per-player frames (the W4 codec twins of the Rust `*_for` messages) ---

test("the per-player client intent round-trips and peeks its kind", () => {
  const frame = encodeClientIntentFor({
    clientSequence: 5,
    lastSeenServerTick: 98,
    payload: u8(1, 2, 3),
    player: 7,
    predictedClientTick: 100,
  });
  assert.equal(peekKind(frame), KIND_CLIENT_INTENT_FOR);
  const m = decodeClientIntentFor(frame);
  assert.deepEqual(
    [m.player, m.clientSequence, m.predictedClientTick, m.lastSeenServerTick],
    [7, 5, 100, 98],
  );
  assert.deepEqual(m.payload, u8(1, 2, 3));
});

test("the per-player server snapshot round-trips with multiple acks", () => {
  const acks: PlayerAck[] = [
    { player: 7, sequence: 5 },
    { player: 9, sequence: 3 },
  ];
  const frame = encodeServerSnapshotFor(42, acks, u8(7, 7));
  assert.equal(peekKind(frame), KIND_SERVER_SNAPSHOT_FOR);
  const m = decodeServerSnapshotFor(frame);
  assert.equal(m.serverTick, 42);
  assert.deepEqual(m.acks, acks);
  assert.deepEqual(m.payload, u8(7, 7));
});

test("the per-player server snapshot round-trips with an empty ack list", () => {
  const frame = encodeServerSnapshotFor(1, [], u8());
  const m = decodeServerSnapshotFor(frame);
  assert.deepEqual(m.acks, []);
  assert.deepEqual(m.payload, u8());
  assert.equal(m.serverTick, 1);
});

test("the per-player server snapshot round-trips at the max ack count", () => {
  const acks: PlayerAck[] = Array.from({ length: MAX_ACKS }, (_v, i) => ({ player: i, sequence: i + 1 }));
  const frame = encodeServerSnapshotFor(3, acks, u8(1));
  assert.deepEqual(decodeServerSnapshotFor(frame).acks, acks);
});

test("decodeFrame dispatches the per-player kinds too", () => {
  const intent = encodeClientIntentFor({
    clientSequence: 1,
    lastSeenServerTick: 0,
    payload: u8(),
    player: 2,
    predictedClientTick: 0,
  });
  const snapshot = encodeServerSnapshotFor(0, [{ player: 1, sequence: 1 }], u8());
  assert.deepEqual(
    [decodeFrame(intent).kind, decodeFrame(snapshot).kind],
    [KIND_CLIENT_INTENT_FOR, KIND_SERVER_SNAPSHOT_FOR],
  );
});

test("the per-player encoders to their cross-language golden bytes (matches the Rust module)", () => {
  // ClientIntentFor { player: 7, client_sequence: 5, predicted: 100, last_seen: 98, payload: [1,2,3] }.
  // Header [major u16=1, minor u16=0, kind u8=7], then four LE u64s, then the
  // u32-length-prefixed payload — byte-identical to client_intent_for.rs::encode.
  const intent = encodeClientIntentFor({
    clientSequence: 5,
    lastSeenServerTick: 98,
    payload: u8(1, 2, 3),
    player: 7,
    predictedClientTick: 100,
  });
  assert.deepEqual(
    Array.from(intent),
    [
      1, 0, 0, 0, 7, 7, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 100, 0, 0, 0, 0, 0, 0, 0, 98, 0, 0,
      0, 0, 0, 0, 0, 3, 0, 0, 0, 1, 2, 3,
    ],
  );
  // ServerSnapshotFor { server_tick: 42, acks: [(7,5),(9,3)], payload: [7,7] }.
  // Header [.., kind u8=8], server_tick u64, ack count u32=2, then each
  // (player u64, sequence u64), then the u32-length-prefixed payload — byte-
  // identical to server_snapshot_for.rs::encode.
  const snapshot = encodeServerSnapshotFor(
    42,
    [
      { player: 7, sequence: 5 },
      { player: 9, sequence: 3 },
    ],
    u8(7, 7),
  );
  assert.deepEqual(
    Array.from(snapshot),
    [
      1, 0, 0, 0, 8, 42, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0,
      9, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 7, 7,
    ],
  );
});

test("the per-player frames are rejected at every truncated prefix", () => {
  const intent = encodeClientIntentFor({
    clientSequence: 5,
    lastSeenServerTick: 98,
    payload: u8(1, 2, 3),
    player: 7,
    predictedClientTick: 100,
  });
  for (let k = 0; k < intent.length; k++) {
    assert.throws(() => decodeClientIntentFor(intent.slice(0, k)), ProtocolError, `intent prefix ${k}`);
  }
  assert.equal(decodeClientIntentFor(intent).player, 7);

  const snapshot = encodeServerSnapshotFor(42, [{ player: 7, sequence: 5 }], u8(7, 7));
  for (let k = 0; k < snapshot.length; k++) {
    assert.throws(() => decodeServerSnapshotFor(snapshot.slice(0, k)), ProtocolError, `snapshot prefix ${k}`);
  }
  assert.equal(decodeServerSnapshotFor(snapshot).serverTick, 42);
});

test("the per-player decoders reject the wrong kind", () => {
  const welcome = encodeWelcome({ clientId: 1, fixedStepNs: 1, protocolVersion: 1, serverTick: 0 });
  assert.throws(() => decodeClientIntentFor(welcome), ProtocolError);
  assert.throws(() => decodeServerSnapshotFor(welcome), ProtocolError);
});

test("the per-player encoders reject an over-size payload", () => {
  const tooBig = new Uint8Array(MAX_PAYLOAD_LEN + 1);
  assert.throws(
    () =>
      encodeClientIntentFor({
        clientSequence: 0,
        lastSeenServerTick: 0,
        payload: tooBig,
        player: 0,
        predictedClientTick: 0,
      }),
    ProtocolError,
  );
  assert.throws(() => encodeServerSnapshotFor(0, [], tooBig), ProtocolError);
});

test("the per-player snapshot rejects too many acks at encode and decode", () => {
  const tooMany: PlayerAck[] = Array.from({ length: MAX_ACKS + 1 }, () => ({ player: 0, sequence: 0 }));
  assert.throws(() => encodeServerSnapshotFor(0, tooMany, u8()), ProtocolError);

  // A hand-built frame whose declared ack count exceeds MAX_ACKS must be
  // rejected before any pair is read (MAX_ACKS + 1 = 4097 = 0x1001, LE u32).
  const overBound = u8(1, 0, 0, 0, KIND_SERVER_SNAPSHOT_FOR, 0, 0, 0, 0, 0, 0, 0, 0, 1, 16, 0, 0);
  assert.throws(() => decodeServerSnapshotFor(overBound), ProtocolError);
});
