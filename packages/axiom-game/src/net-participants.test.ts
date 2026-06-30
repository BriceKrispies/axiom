import assert from "node:assert/strict";
import { test } from "node:test";

import { makeNetParticipants } from "./net-participants.ts";
import { makeNetSim } from "./net.ts";
import { encodeIntent } from "./axiom-net.ts";
import type { SimContext } from "./sim.ts";
import { TickPump } from "./pump.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";

const fixedHz = 60;

const makeContext = (): SimContext => {
  const bridge = new FakeBridge();
  return { bridge, fixedHz, pump: new TickPump(bridge, fixedHz) };
};

/*
 * THE FIXED GOLDEN — a participant block hand-derived from the authoritative byte
 * layout in tools/axiom-netplay-server/src/authority.rs::encode_participant_block.
 * All integers little-endian; lengths u32; ids u64. The byte walk (offsets):
 *
 *   [0..4)   participant_count = 2                          -> 02 00 00 00
 *   seat 1:
 *   [4..12)  player_id = 1 (u64)                            -> 01 00 00 00 00 00 00 00
 *   [12]     flags = 1 (bit0 joinedThisTick)               -> 01
 *   [13..17) intent_len = 14 (u32)                          -> 0e 00 00 00
 *   [17..31) intent = encodeIntent({ fire: true }) (14 B):
 *              [17..21) field_count = 1                     -> 01 00 00 00
 *              [21..25) key_len = 4                         -> 04 00 00 00
 *              [25..29) "fire"                              -> 66 69 72 65
 *              [29]     type tag = 0 (bool)                 -> 00
 *              [30]     value = 1 (true)                    -> 01
 *   seat 2:
 *   [31..39) player_id = 2 (u64)                            -> 02 00 00 00 00 00 00 00
 *   [39]     flags = 0 (already seated, not joined)        -> 00
 *   [40..44) intent_len = 0 (sent no intent)               -> 00 00 00 00
 *   left:
 *   [44..48) left_count = 1 (u32)                           -> 01 00 00 00
 *   [48..56) left[0] = 3 (u64) — a seat vacated this tick   -> 03 00 00 00 00 00 00 00
 *   state:
 *   [56..60) state_len = 3 (u32)                            -> 03 00 00 00
 *   [60..63) state = [42, 7, 255]                           -> 2a 07 ff
 *
 * Total = 63 bytes.
 */
const GOLDEN = Uint8Array.from([
  2, 0, 0, 0,
  1, 0, 0, 0, 0, 0, 0, 0,
  1,
  14, 0, 0, 0,
  1, 0, 0, 0, 4, 0, 0, 0, 102, 105, 114, 101, 0, 1,
  2, 0, 0, 0, 0, 0, 0, 0,
  0,
  0, 0, 0, 0,
  1, 0, 0, 0,
  3, 0, 0, 0, 0, 0, 0, 0,
  3, 0, 0, 0,
  42, 7, 255,
]);

test("the golden's seat-1 intent segment is exactly encodeIntent({ fire: true })", () => {
  // Proves the block composes the flat-record intent codec (axiom-net.ts) verbatim:
  // the 14 intent bytes at [17..31) are encodeIntent's output, byte-for-byte — so
  // decodeMaybeEmpty's non-empty arm decodes a real codec payload.
  assert.deepEqual(GOLDEN.subarray(17, 31), encodeIntent({ fire: true }));
});

test("makeNetParticipants decodes the authoritative participant block", () => {
  const participants = makeNetParticipants(GOLDEN);

  // players are the participant ids in stable order; the tick argument is ignored
  // (one block describes exactly one authoritative tick).
  assert.deepEqual(participants.players(0), [1, 2]);

  // joinedThisTick is the seats with flags bit0 set (only seat 1 here) — exercises
  // joinedFromFlags' true arm (flags 1) and false arm (flags 0) across the two seats.
  assert.deepEqual(participants.joinedThisTick(0), [1]);

  // leftThisTick is the decoded left-id list (decodeTail's bounded Array.from map).
  assert.deepEqual(participants.leftThisTick(0), [3]);

  // the renderable authoritative sim-state blob rides along untouched.
  assert.deepEqual(participants.state(), Uint8Array.from([42, 7, 255]));

  // inputOf(1) projects the decoded { fire: true } intent onto the Input surface
  // (intentFor's found arm + decodeMaybeEmpty's non-empty arm).
  const seatOne = participants.inputOf(1, 0);
  assert.equal(seatOne.isDown("fire"), true);
  assert.equal(seatOne.isDown("jump"), false);
  // A flat networked intent carries no analog look — the surface reads the neutral delta.
  assert.deepEqual(seatOne.look(), { x: 0, y: 0 });
});

test("inputOf projects axis from the held intent fields across all three steps", () => {
  const seatOne = makeNetParticipants(GOLDEN).inputOf(1, 0);
  // { fire: true }: pos held, neg not -> +1; neg held, pos not -> -1; neither -> 0.
  // This drives heldDifference's three numerators and intentInput's AXIS_STEPS pick.
  assert.equal(seatOne.axis("idle", "fire"), 1);
  assert.equal(seatOne.axis("fire", "idle"), -1);
  assert.equal(seatOne.axis("left", "right"), 0);
});

test("inputOf reports the off-wire channels as absent/false (flat intent carries held state only)", () => {
  const seatOne = makeNetParticipants(GOLDEN).inputOf(1, 0);
  // Edges and the local-only pointer/gesture/press-history channels are not on the
  // flat wire — they read false / the empty value (the `absent` slot), never fabricated.
  assert.equal(seatOne.pressed("fire"), false);
  assert.equal(seatOne.released("fire"), false);
  assert.equal(seatOne.pointer(), undefined);
  assert.equal(seatOne.pointerPressed(), undefined);
  assert.equal(seatOne.swipe(), undefined);
  assert.equal(seatOne.pressedAtTick("fire"), undefined);
});

test("a seat that sent no intent (0-byte payload) decodes to the empty held intent", () => {
  // Seat 2 has intent_len 0; decodeMaybeEmpty's empty arm maps the empty payload to
  // {} (via EMPTY_INTENT_BYTES) rather than throwing, so every read is the default.
  const seatTwo = makeNetParticipants(GOLDEN).inputOf(2, 0);
  assert.equal(seatTwo.isDown("fire"), false);
  assert.equal(seatTwo.axis("a", "b"), 0);
});

test("inputOf for an unseated player is the empty intent", () => {
  // No seat 99 in the block -> the empty intent default (intentFor's absent arm).
  const ghost = makeNetParticipants(GOLDEN).inputOf(99, 0);
  assert.equal(ghost.isDown("fire"), false);
});

test("makeNetParticipants wires straight into makeNetSim each snapshot", () => {
  // The decoded snapshot IS a NetParticipants, so a hosted/joined game builds its
  // per-tick NetSim directly from it — the authored onFixedUpdate then reads the
  // decoded players through the same surface local mode uses.
  const sim = makeNetSim(makeContext(), makeNetParticipants(GOLDEN), 1);
  assert.equal(sim.tick, 1);
  assert.deepEqual(sim.players(), [1, 2]);
  assert.equal(sim.inputOf(1).isDown("fire"), true);
  assert.deepEqual(sim.joinedThisTick(), [1]);
  assert.deepEqual(sim.leftThisTick(), [3]);
});
