import assert from "node:assert/strict";
import { test } from "node:test";

import {
  applySnapshot,
  decodeServerSnapshotForDelta,
  diffSnapshot,
  encodeServerSnapshotForDelta,
} from "./snapshot-delta.ts";
import { KIND_SERVER_SNAPSHOT_FOR_DELTA, MAX_PAYLOAD_LEN, type PlayerAck } from "./messages.ts";
import { ProtocolError } from "./protocol-error.ts";
import { byteWriter } from "./byte-writer.ts";

const u8 = (...bytes: number[]): Uint8Array => Uint8Array.from(bytes);
const bytes = (text: string): Uint8Array => new TextEncoder().encode(text);

const roundTrip = (base: string, next: string): void => {
  const blob = diffSnapshot(bytes(base), bytes(next));
  assert.deepEqual(applySnapshot(bytes(base), blob), bytes(next), `${base} -> ${next}`);
};

test("the diff reconstructs the new payload for equal/changed/grown/shrunk/empty", () => {
  roundTrip("hello world", "hello world");
  roundTrip("hello world", "hELLo world");
  roundTrip("hello", "hello, longer tail");
  roundTrip("hello, longer tail", "hi");
  roundTrip("", "from empty");
  roundTrip("to empty", "");
  roundTrip("", "");
});

test("an unchanged payload diffs to a blob smaller than the payload", () => {
  const payload = new Uint8Array(4096).fill(7);
  const blob = diffSnapshot(payload, payload);
  assert.ok(blob.length < payload.length, "delta must beat the full payload");
  assert.deepEqual(applySnapshot(payload, blob), payload);
});

test("the delta snapshot frame round-trips and reconstructs the full payload", () => {
  const base = bytes("authoritative state at tick 42");
  const next = bytes("authoritative STATE at tick 43!");
  const acks: PlayerAck[] = [{ player: 1, sequence: 9 }];
  const frame = encodeServerSnapshotForDelta({ acks, base, baseTick: 42, next, serverTick: 43 });
  const m = decodeServerSnapshotForDelta(frame);
  assert.equal(m.kind, KIND_SERVER_SNAPSHOT_FOR_DELTA);
  assert.equal(m.serverTick, 43);
  assert.equal(m.baseTick, 42);
  assert.deepEqual(m.acks, acks);
  assert.deepEqual(applySnapshot(base, m.delta), next);
});

test("apply rejects an over-max declared length", () => {
  const writer = byteWriter();
  writer.u32(MAX_PAYLOAD_LEN + 1);
  assert.throws((): Uint8Array => applySnapshot(u8(), writer.finish()), ProtocolError);
});

test("apply rejects a change count beyond the shared prefix", () => {
  const writer = byteWriter();
  writer.u32(2); // new_len = 2
  writer.u32(3); // 3 changes claimed (> common of 2)
  assert.throws((): Uint8Array => applySnapshot(u8(120, 121), writer.finish()), ProtocolError);
});

test("apply rejects an out-of-range change offset", () => {
  const writer = byteWriter();
  writer.u32(2); // new_len = 2 -> common with a 2-byte base = 2
  writer.u32(1); // one change
  writer.u32(5); // offset 5 outside [0, 2)
  writer.u8(90);
  writer.byteSlice(u8()); // empty tail
  assert.throws((): Uint8Array => applySnapshot(u8(120, 121), writer.finish()), ProtocolError);
});

test("apply rejects an inconsistent tail length", () => {
  const writer = byteWriter();
  writer.u32(2); // new_len = 2, common with a 2-byte base = 2
  writer.u32(0); // no changes
  writer.byteSlice(bytes("extra")); // tail of 5 != new_len - common (0)
  assert.throws((): Uint8Array => applySnapshot(u8(120, 121), writer.finish()), ProtocolError);
});

test("cross-language byte parity with the Rust net-protocol fixture", () => {
  // The exact bytes the Rust `server_snapshot_for_delta` encoder produces for the
  // same input (`cross_language_byte_parity_fixture`). Identical literal both sides.
  const frame = encodeServerSnapshotForDelta({
    acks: [{ player: 1, sequence: 9 }],
    base: bytes("abc"),
    baseTick: 42,
    next: bytes("abd"),
    serverTick: 43,
  });
  const expected = u8(
    1, 0, 0, 0, 9, 43, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0,
    0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 17, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 100, 0, 0, 0, 0,
  );
  assert.deepEqual(frame, expected);
});
