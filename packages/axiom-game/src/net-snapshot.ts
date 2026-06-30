/*
 * The delta-transparent inbound-snapshot path (SPEC-13 §16.5). The server-
 * authoritative authority may broadcast a per-player snapshot either as a FULL
 * participant block (the keyframe `makeNetParticipants` decodes) or as a DELTA
 * frame — a sparse byte-patch against the client's last keyframe — when that is
 * smaller. This module lets a hosted/joined game accept either transparently: a
 * delta is reconstructed against the last keyframe into the SAME full payload a
 * full frame carries, so the author's `onSnapshot`/`onRestore` always sees full
 * state.
 *
 * `reconstructSnapshot` is the byte-for-byte twin of `@axiom/client`'s
 * `applySnapshot` (the two packages stay decoupled — `axiom-net.ts` re-implements
 * the wire codec rather than importing `@axiom/client`, exactly as
 * `net-participants.ts` mirrors the Rust participant block). The diff blob layout
 * (little-endian, identical to Rust `snapshot_delta`):
 *
 * ```text
 *   u32                new_len
 *   u32                change_count
 *   change_count × (u32 offset, u8 byte)
 *   u32                tail_len
 *   u8 * tail_len      tail
 * ```
 *
 * Reconstruct: take `base[..common]` (common = min(baseLen, new_len)), overwrite
 * each `offset` with `byte`, append `tail`. Branchless throughout: the changes are
 * a bounded `Array.from` map over a mutable cursor (the same shape the byte
 * decoders use), the patch is `each`, and the frame-kind select is a `pick`.
 */

import { type DecodedSnapshot, makeNetParticipants } from "./net-participants.ts";
import { each, pick } from "./control-flow.ts";
import { assert } from "./authoring-error.ts";

/** The byte width of a `u32` count / length prefix or change offset. */
const U32_BYTES = 4;
/** The byte width of one change's literal byte. */
const U8_BYTES = 1;
/** The leading `u32 new_len` + `u32 change_count` header width. */
const HEADER_BYTES = U32_BYTES + U32_BYTES;
/** The offset of the first byte in a buffer. */
const START = 0;

/** One byte changed at `offset` between the base and the reconstructed payload. */
interface Change {
  readonly offset: number;
  readonly byte: number;
}

/** A `DataView` over `bytes`' exact backing region (respecting any sub-array offset). */
const viewOf = (bytes: Uint8Array): DataView => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);

/** Read the `change_count` changes from `cursor.offset`, advancing it past each `(u32, u8)` pair. */
const readChanges = (view: DataView, count: number, cursor: { offset: number }): readonly Change[] =>
  Array.from({ length: count }, (): Change => {
    const offset = view.getUint32(cursor.offset, true);
    const byte = view.getUint8(cursor.offset + U32_BYTES);
    cursor.offset += U32_BYTES + U8_BYTES;
    return { byte, offset };
  });

/** Overwrite the shared prefix with the changes, gating each offset to the prefix length. */
const applyChanges = (prefix: Uint8Array, changes: readonly Change[], common: number): void => {
  each(changes, (change): void => {
    assert(change.offset < common, "delta change offset is out of range");
    prefix[change.offset] = change.byte;
  });
};

/** Concatenate the patched shared prefix and the verbatim tail into the new payload. */
const joinPrefixAndTail = (prefix: Uint8Array, tail: Uint8Array, common: number): Uint8Array => {
  const out = new Uint8Array(common + tail.length);
  out.set(prefix, START);
  out.set(tail, common);
  return out;
};

/** The validated leading fields of a diff blob: the new length, the shared-prefix length, and the change count. */
interface DeltaHeader {
  readonly common: number;
  readonly count: number;
  readonly newLen: number;
}

/** Read the `new_len` / `change_count` header and derive the shared-prefix length against `baseLen`. */
const readDeltaHeader = (view: DataView, baseLen: number): DeltaHeader => {
  const newLen = view.getUint32(START, true);
  const count = view.getUint32(U32_BYTES, true);
  return { common: Math.min(baseLen, newLen), count, newLen };
};

/** Read the length-prefixed verbatim tail from `cursor.offset` (after the changes). */
const readTail = (view: DataView, blob: Uint8Array, cursor: { offset: number }): Uint8Array => {
  const tailLen = view.getUint32(cursor.offset, true);
  const tailStart = cursor.offset + U32_BYTES;
  return blob.subarray(tailStart, tailStart + tailLen);
};

/** Reconstruct the full payload from a keyframe `base` and a delta `blob` (twin of `@axiom/client` `applySnapshot`). */
export const reconstructSnapshot = (base: Uint8Array, blob: Uint8Array): Uint8Array => {
  const view = viewOf(blob);
  const { common, count, newLen } = readDeltaHeader(view, base.length);
  const cursor = { offset: HEADER_BYTES };
  const changes = readChanges(view, count, cursor);
  const prefix = base.slice(START, common);
  applyChanges(prefix, changes, common);
  const tail = readTail(view, blob, cursor);
  assert(common + tail.length === newLen, "delta tail length is inconsistent with the declared length");
  return joinPrefixAndTail(prefix, tail, common);
};

/** Whether an inbound snapshot frame is a full keyframe or a delta patch against the last keyframe. */
export type SnapshotFrameKind = "full" | "delta";

/** The delta-transparent inbound-snapshot path: feed each frame, read full participant state out. */
export interface SnapshotIntake {
  /** Decode a snapshot frame to its full `DecodedSnapshot`, reconstructing a delta against the last keyframe. */
  readonly accept: (kind: SnapshotFrameKind, payload: Uint8Array) => DecodedSnapshot;
}

/**
 * Build a stateful, delta-transparent snapshot intake. A `"full"` frame is the
 * keyframe (stored as the base for the next delta); a `"delta"` frame is
 * reconstructed against the stored keyframe into the full payload (and itself
 * becomes the new keyframe, so chained deltas accumulate). Either way `accept`
 * returns the full `DecodedSnapshot` the author sees — the full frame is the
 * keyframe and the fallback.
 */
export const makeSnapshotIntake = (): SnapshotIntake => {
  const keyframe: { base: Uint8Array } = { base: new Uint8Array() };
  const acceptFull = (payload: Uint8Array): DecodedSnapshot => {
    keyframe.base = payload;
    return makeNetParticipants(payload);
  };
  const acceptDelta = (blob: Uint8Array): DecodedSnapshot => {
    const full = reconstructSnapshot(keyframe.base, blob);
    keyframe.base = full;
    return makeNetParticipants(full);
  };
  return {
    accept: (kind: SnapshotFrameKind, payload: Uint8Array): DecodedSnapshot =>
      pick([acceptFull, acceptDelta], Number(kind === "delta"))(payload),
  };
};
