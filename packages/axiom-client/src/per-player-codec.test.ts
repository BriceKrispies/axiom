import assert from "node:assert/strict";
import { test } from "node:test";

import {
  decodeClientIntentFor,
  decodeServerSnapshotFor,
  encodeClientIntentFor,
  encodeServerSnapshotFor,
} from "./per-player-codec.ts";
import {
  KIND_SERVER_SNAPSHOT_FOR,
  MAX_ACKS,
  MAX_PAYLOAD_LEN,
  type PlayerAck,
} from "./messages.ts";
import { encodeWelcome } from "./codec.ts";
import { ProtocolError } from "./protocol-error.ts";

const u8 = (...bytes: number[]): Uint8Array => Uint8Array.from(bytes);

test("the per-player client intent round-trips its fields", () => {
  const frame = encodeClientIntentFor({
    clientSequence: 5,
    lastSeenServerTick: 98,
    payload: u8(1, 2, 3),
    player: 7,
    predictedClientTick: 100,
  });
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

test("the per-player encoders produce their cross-language golden bytes", () => {
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
