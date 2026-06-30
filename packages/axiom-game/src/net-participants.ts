/*
 * The authority-snapshot participant decoder (SPEC-13 §16.1 / §16.5). The
 * server-authoritative authority (`tools/axiom-netplay-server`'s `Authority`)
 * broadcasts a `ServerSnapshotFor` whose opaque payload IS the *participant
 * block*: the per-tick seats, each seat's last-applied intent, the seats that
 * joined/left this tick, and the authoritative renderable sim-state blob. This
 * module decodes that block into the `NetParticipants` seam `makeNetSim` already
 * consumes — so a hosted/joined game's `onFixedUpdate` reads decoded players each
 * snapshot through the exact `inputOf`/`players`/`joinedThisTick`/`leftThisTick`
 * surface `local` mode uses.
 *
 * ## The authoritative byte layout (mirror of `authority.rs::encode_participant_block`)
 * All integers little-endian; lengths are `u32`; ids are `u64`. The acks
 * `(player, sequence)` ride in the OUTER `ServerSnapshotFor` envelope, NOT this
 * block, so the runtime decodes the envelope (over `@axiom/client`) and hands the
 * inner `.payload` here.
 *
 * ```text
 *   u32                participant_count
 *   participant_count repetitions of:
 *     u64              player_id          (the stable 1-based seat id)
 *     u8               flags              (bit0 = joinedThisTick; others reserved 0)
 *     u32              intent_len
 *     u8 * intent_len  intent             (the flat-record `encodeIntent` payload
 *                                          last applied this tick; 0 bytes = none)
 *   u32                left_count
 *   left_count × u64   player_id          (leftThisTick: seats vacated this tick)
 *   u32                state_len
 *   u8 * state_len     state              (RunningApp::snapshot_sim — the
 *                                          deterministic renderable world)
 * ```
 *
 * ## inputOf is the flat-intent projection, not the local rich Input
 * A seat's `inputOf(player)` is the decoded `Intent` (via `decodeIntent`) presented
 * through the `Input` surface. The flat per-tick `Intent` (SPEC-13 §16.2) carries
 * named HELD values only — it is a state snapshot, not an event log — so `isDown`/
 * `axis` read those held fields, while edges (`pressed`/`released`) and the local-
 * only channels (`pointer`/`pointerPressed`/`swipe`/`pressedAtTick`) are not on the
 * wire and read as absent/false. A networked game that needs an edge encodes it as
 * an explicit boolean field in its `Intent` and reads it via `isDown` (SPEC-13 §9
 * flags flat as the current intent floor).
 */

import type { Action, Input } from "./input.ts";
import type { Intent, NetParticipants } from "./net.ts";
import type { PlayerId, Result, Ticks, Vec2 } from "./vocabulary.ts";
import type { PointerSample, Swipe } from "./native-bridge.ts";
import { orElse, pick } from "./control-flow.ts";
import { decodeIntent } from "./axiom-net.ts";

/** The byte width of a `u32` count / length prefix. */
const U32_BYTES = 4;
/** The byte width of a `u64` player id. */
const U64_BYTES = 8;
/** The byte width of the per-seat `flags` byte. */
const FLAG_BYTES = 1;
/** `flags` bit0: this seat first appeared on the tick being described. */
const FLAG_JOINED_THIS_TICK = 1;
/** The modulus that extracts `flags` bit0 arithmetically (the spine bans bitwise `&`). */
const FLAGS_BIT0_MODULUS = 2;

/** The empty author intent — the decoded form of a seat that sent no intent this tick. */
const EMPTY_INTENT: Intent = {};
/** `encodeIntent({})` — a zero field-count, the buffer a 0-byte intent stands in for. */
const EMPTY_INTENT_BYTES = Uint8Array.from([0, 0, 0, 0]);

/** The axis steps indexed by `held(pos) - held(neg) + AXIS_BIAS`, the SPEC-05 `[-1,0,1]` table. */
const AXIS_STEPS: readonly [-1, 0, 1] = [-1, 0, 1];
/** The offset mapping the `[-1, 0, 1]` held difference onto the `[0, 1, 2]` index. */
const AXIS_BIAS = 1;

/** The absent `Result<Value>` an off-wire input channel reports (the unset optional slot). */
const absent = <Value>(slot?: Value): Result<Value> => slot;

/** One decoded seat: its id, whether it joined this tick, and its decoded held intent. */
interface Seat {
  readonly player: PlayerId;
  readonly joined: boolean;
  readonly intent: Intent;
}

/** The left list and renderable state blob that follow the seats in the block. */
interface SnapshotTail {
  readonly left: readonly PlayerId[];
  readonly state: Uint8Array;
}

/** A `DataView` over `bytes`' exact backing region (respecting any sub-array offset). */
const viewOf = (bytes: Uint8Array): DataView => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);

/** Read a little-endian `u32` at `offset`. */
const readU32 = (view: DataView, offset: number): number => view.getUint32(offset, true);

/** Read a little-endian `u64` player id at `offset` (seat ids are small, so `Number` is exact). */
const readU64 = (view: DataView, offset: number): number => Number(view.getBigUint64(offset, true));

/** Whether `flags` bit0 (joinedThisTick) is set, extracted arithmetically without a bitwise op. */
const joinedFromFlags = (flags: number): boolean => flags % FLAGS_BIT0_MODULUS === FLAG_JOINED_THIS_TICK;

/** Decode an intent payload, mapping a 0-byte payload (the seat sent none) to the empty record. */
const decodeMaybeEmpty = (bytes: Uint8Array): Intent =>
  decodeIntent(pick([bytes, EMPTY_INTENT_BYTES], Number(bytes.length === 0)));

/** Decode one seat at `cursor.offset`, advancing the cursor past its variable-length intent. */
const decodeSeat = (view: DataView, payload: Uint8Array, cursor: { offset: number }): Seat => {
  const player = readU64(view, cursor.offset);
  const flags = view.getUint8(cursor.offset + U64_BYTES);
  const intentLen = readU32(view, cursor.offset + U64_BYTES + FLAG_BYTES);
  const intentStart = cursor.offset + U64_BYTES + FLAG_BYTES + U32_BYTES;
  const intentEnd = intentStart + intentLen;
  cursor.offset = intentEnd;
  return { intent: decodeMaybeEmpty(payload.subarray(intentStart, intentEnd)), joined: joinedFromFlags(flags), player };
};

/** Decode the left-id list and the trailing renderable state blob from `cursor.offset`. */
const decodeTail = (view: DataView, payload: Uint8Array, cursor: { offset: number }): SnapshotTail => {
  const leftCount = readU32(view, cursor.offset);
  cursor.offset += U32_BYTES;
  const left = Array.from({ length: leftCount }, (): PlayerId => {
    const player = readU64(view, cursor.offset);
    cursor.offset += U64_BYTES;
    return player;
  });
  const stateLen = readU32(view, cursor.offset);
  cursor.offset += U32_BYTES;
  return { left, state: payload.subarray(cursor.offset, cursor.offset + stateLen) };
};

/** The decoded held intent for `player`, or the empty intent when no such seat exists. */
const intentFor = (seats: readonly Seat[], player: PlayerId): Intent =>
  orElse(
    seats
      .filter((seat): boolean => seat.player === player)
      .map((seat): Intent => seat.intent)
      .at(0),
    EMPTY_INTENT,
  );

/** The held difference `held(pos) - held(neg)` over the intent's boolean fields (the axis numerator). */
const heldDifference = (intent: Intent, neg: Action, pos: Action): number =>
  Number(Boolean(intent[pos])) - Number(Boolean(intent[neg]));

/*
 * Project a decoded flat `Intent` onto the `Input` surface (see the module header):
 * `isDown`/`axis` read the held fields; edges and local-only channels are absent.
 */
const intentInput = (intent: Intent): Input => ({
  axis: (neg: Action, pos: Action): -1 | 0 | 1 => pick(AXIS_STEPS, heldDifference(intent, neg, pos) + AXIS_BIAS),
  isDown: (action: Action): boolean => Boolean(intent[action]),
  // A networked flat intent carries held keys, not analog look — the neutral delta.
  look: (): Vec2 => ({ x: 0, y: 0 }),
  pointer: (): Result<PointerSample> => absent<PointerSample>(),
  pointerPressed: (): Result<Vec2> => absent<Vec2>(),
  pressed: (): boolean => false,
  pressedAtTick: (): Result<Ticks> => absent<Ticks>(),
  released: (): boolean => false,
  swipe: (): Result<Swipe> => absent<Swipe>(),
});

/** The decoded authority snapshot: the `NetParticipants` seam plus its renderable sim-state blob. */
export interface DecodedSnapshot extends NetParticipants {
  /** The authoritative renderable sim-state blob (RunningApp::snapshot_sim) this snapshot carries. */
  readonly state: () => Uint8Array;
}

/*
 * Decode an authority participant-block payload into the `NetParticipants` seam
 * `makeNetSim` consumes (plus the renderable `state` blob). A single mutable
 * `cursor` walks the buffer as the bounded `Array.from` maps visit each seat /
 * left-id (the counts were length-prefixed, so iteration needs no `while`), exactly
 * as `decodeIntent` walks the flat-record buffer. The result ignores the per-method
 * `tick` argument: one decoded block describes exactly one authoritative tick, so
 * the runtime builds a fresh `DecodedSnapshot` per snapshot.
 */
export const makeNetParticipants = (payload: Uint8Array): DecodedSnapshot => {
  const view = viewOf(payload);
  const cursor = { offset: U32_BYTES };
  const seats = Array.from({ length: readU32(view, 0) }, (): Seat => decodeSeat(view, payload, cursor));
  const tail = decodeTail(view, payload, cursor);
  return {
    inputOf: (player: PlayerId): Input => intentInput(intentFor(seats, player)),
    joinedThisTick: (): readonly PlayerId[] =>
      seats.filter((seat): boolean => seat.joined).map((seat): PlayerId => seat.player),
    leftThisTick: (): readonly PlayerId[] => tail.left,
    players: (): readonly PlayerId[] => seats.map((seat): PlayerId => seat.player),
    state: (): Uint8Array => tail.state,
  };
};
