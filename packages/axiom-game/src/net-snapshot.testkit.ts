/*
 * Test fixtures for the inbound-snapshot path. Two builders the AUTHORITY side
 * owns (and the client never runs), used to drive `net-snapshot.ts`'s reconstruct
 * + intake without importing `@axiom/client`:
 *   - `diffSnapshot` is the authority-side twin of `@axiom/client`'s `diffSnapshot`
 *     (the delta blob a `reconstructSnapshot` round-trip consumes);
 *   - `participantBlock` encodes a participant block in the exact layout
 *     `net-participants.ts` decodes, so a full/delta intake test asserts decoded
 *     seats/state, not just opaque bytes.
 * Test-tier, so it keeps its branches/loops (the Branchless Law exempts tests).
 */

const U32 = 4;
const U8 = 1;
const U64 = 8;
const CHANGE_SIZE = U32 + U8;

/** One seat for {@link participantBlock}: its id, whether it joined this tick, and its encoded held intent. */
export interface BlockSeat {
  readonly player: number;
  readonly joined: boolean;
  readonly intent: Uint8Array;
}

/** Build the delta blob that turns `base` into `next` (the authority's sparse byte patch). */
export const diffSnapshot = (base: Uint8Array, next: Uint8Array): Uint8Array => {
  const common = Math.min(base.length, next.length);
  const changes: { offset: number; byte: number }[] = [];
  for (let index = 0; index < common; index += 1) {
    if (base[index] !== next[index]) {
      changes.push({ byte: next[index]!, offset: index });
    }
  }
  const tail = next.subarray(common);
  const buffer = new Uint8Array(U32 + U32 + changes.length * CHANGE_SIZE + U32 + tail.length);
  const view = new DataView(buffer.buffer);
  view.setUint32(0, next.length, true);
  view.setUint32(U32, changes.length, true);
  let cursor = U32 + U32;
  for (const change of changes) {
    view.setUint32(cursor, change.offset, true);
    view.setUint8(cursor + U32, change.byte);
    cursor += CHANGE_SIZE;
  }
  view.setUint32(cursor, tail.length, true);
  buffer.set(tail, cursor + U32);
  return buffer;
};

/** Encode a participant block (seats + left ids + renderable state) in `net-participants.ts`'s layout. */
export const participantBlock = (seats: readonly BlockSeat[], left: readonly number[], state: Uint8Array): Uint8Array => {
  const seatSize = seats.reduce((sum, seat) => sum + U64 + U8 + U32 + seat.intent.length, 0);
  const size = U32 + seatSize + U32 + left.length * U64 + U32 + state.length;
  const buffer = new Uint8Array(size);
  const view = new DataView(buffer.buffer);
  let cursor = 0;
  view.setUint32(cursor, seats.length, true);
  cursor += U32;
  for (const seat of seats) {
    view.setBigUint64(cursor, BigInt(seat.player), true);
    // flags bit0 = joinedThisTick: `Number(joined)` sets exactly that bit (the layout `net-participants.ts` reads).
    view.setUint8(cursor + U64, Number(seat.joined));
    view.setUint32(cursor + U64 + U8, seat.intent.length, true);
    buffer.set(seat.intent, cursor + U64 + U8 + U32);
    cursor += U64 + U8 + U32 + seat.intent.length;
  }
  view.setUint32(cursor, left.length, true);
  cursor += U32;
  for (const player of left) {
    view.setBigUint64(cursor, BigInt(player), true);
    cursor += U64;
  }
  view.setUint32(cursor, state.length, true);
  buffer.set(state, cursor + U32);
  return buffer;
};
