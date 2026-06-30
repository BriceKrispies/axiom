import assert from "node:assert/strict";
import { test } from "node:test";

import { type SnapshotFrameKind, makeSnapshotIntake, reconstructSnapshot } from "./net-snapshot.ts";
import { type BlockSeat, diffSnapshot, participantBlock } from "./net-snapshot.testkit.ts";
import { AuthoringError } from "./authoring-error.ts";
import { encodeIntent } from "./axiom-net.ts";

const u8 = (...bytes: number[]): Uint8Array => Uint8Array.from(bytes);
const text = (value: string): Uint8Array => new TextEncoder().encode(value);

// One seat holding `fire` as a flat intent — the fixture the full/delta intake tests share.
const fireSeats = (fire: boolean): BlockSeat[] => [{ intent: encodeIntent({ fire }), joined: true, player: 1 }];
// A single quiet seat (no intent) carrying `state` — the fixture the chained-delta test mutates.
const stateBlock = (state: Uint8Array): Uint8Array =>
  participantBlock([{ intent: encodeIntent({}), joined: false, player: 7 }], [], state);

// reconstruct(base, diff(base, next)) === next for every shape of change.
const roundTrip = (base: string, next: string): void => {
  const blob = diffSnapshot(text(base), text(next));
  assert.deepEqual(reconstructSnapshot(text(base), blob), text(next), `${base} -> ${next}`);
};

test("reconstructSnapshot round-trips a delta for equal/changed/grown/shrunk/empty payloads", () => {
  roundTrip("hello world", "hello world");
  roundTrip("hello world", "hELLo world");
  roundTrip("hello", "hello, longer tail");
  roundTrip("hello, longer tail", "hi");
  roundTrip("", "from empty");
  roundTrip("to empty", "");
  roundTrip("", "");
});

test("reconstructSnapshot rejects an out-of-range change offset", () => {
  // new_len = 2 (common with a 2-byte base = 2), one change at offset 5 (outside [0,2)).
  const view = new DataView(new ArrayBuffer(4 + 4 + 4 + 1 + 4));
  view.setUint32(0, 2, true); // new_len
  view.setUint32(4, 1, true); // one change
  view.setUint32(8, 5, true); // offset 5 — out of range
  view.setUint8(12, 90);
  view.setUint32(13, 0, true); // empty tail
  assert.throws((): Uint8Array => reconstructSnapshot(u8(120, 121), new Uint8Array(view.buffer)), AuthoringError);
});

test("reconstructSnapshot rejects an inconsistent tail length", () => {
  // new_len = 2 (common = 2), no changes, but a 5-byte tail: 2 + 5 != 2.
  const tail = text("extra");
  const view = new DataView(new ArrayBuffer(4 + 4 + 4 + tail.length));
  view.setUint32(0, 2, true); // new_len
  view.setUint32(4, 0, true); // no changes
  view.setUint32(8, tail.length, true);
  const blob = new Uint8Array(view.buffer);
  blob.set(tail, 12);
  assert.throws((): Uint8Array => reconstructSnapshot(u8(120, 121), blob), AuthoringError);
});

test("the intake decodes a full keyframe to full participant state", () => {
  const block = participantBlock(
    [{ intent: encodeIntent({ fire: true }), joined: true, player: 1 }],
    [],
    u8(1, 2, 3),
  );
  // The participant facade is tick-scoped (one block describes one tick); the decoder
  // ignores the tick argument, so any tick reads this block's seats.
  const tick = 0;
  const decoded = makeSnapshotIntake().accept("full", block);
  assert.deepEqual(decoded.players(tick), [1]);
  assert.deepEqual(decoded.joinedThisTick(tick), [1]);
  assert.deepEqual(decoded.state(), u8(1, 2, 3));
  assert.equal(decoded.inputOf(1, tick).isDown("fire"), true);
});

test("a delta frame reconstructs the SAME full state a full frame carries", () => {
  const baseBlock = participantBlock(fireSeats(true), [], u8(1, 2, 3));
  const nextBlock = participantBlock(fireSeats(false), [2], u8(9, 9, 9));

  const tick = 0;
  const intake = makeSnapshotIntake();
  // The keyframe establishes the base; the author sees the held `fire: true`.
  assert.equal(intake.accept("full", baseBlock).inputOf(1, tick).isDown("fire"), true);

  // The delta is a patch against that keyframe — the author sees the SAME full
  // state as if the authority had sent `nextBlock` whole.
  const viaDelta = intake.accept("delta", diffSnapshot(baseBlock, nextBlock));
  assert.deepEqual(viaDelta.players(tick), [1]);
  assert.deepEqual(viaDelta.leftThisTick(tick), [2]);
  assert.deepEqual(viaDelta.state(), u8(9, 9, 9));
  assert.equal(viaDelta.inputOf(1, tick).isDown("fire"), false);
});

test("chained deltas advance the keyframe so each patch applies to the prior result", () => {
  const a = stateBlock(u8(10, 20, 30));
  const b = stateBlock(u8(10, 99, 30));
  const c = stateBlock(u8(10, 99, 77));

  const intake = makeSnapshotIntake();
  intake.accept("full", a);
  intake.accept("delta", diffSnapshot(a, b)); // keyframe is now `b`
  // This delta is diffed against `b`; it only reconstructs correctly if the intake
  // advanced its keyframe from `a` to `b`.
  const third = intake.accept("delta", diffSnapshot(b, c));
  assert.deepEqual(third.state(), u8(10, 99, 77));
  assert.deepEqual(third.players(0), [7]);
});

// A frame kind is one of exactly two carriers (compile-time exhaustiveness witness).
const KINDS: readonly SnapshotFrameKind[] = ["full", "delta"];
test("the frame-kind union is the two-carrier keyframe/delta discriminant", () => {
  assert.deepEqual(KINDS, ["full", "delta"]);
});
