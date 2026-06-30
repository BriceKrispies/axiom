/*
 * The delta-snapshot codec — the byte-for-byte twin of the Rust `snapshot_delta`
 * (the sparse byte-patch diff) and `server_snapshot_for_delta` (the per-player
 * delta frame) modules. A `ServerSnapshotForDelta` carries a diff against the
 * client's last-acked snapshot (`baseTick`) instead of a full payload; the client
 * reconstructs the full payload from `(its base snapshot, this delta)`. The full
 * `ServerSnapshotFor` stays the keyframe and the fallback.
 *
 * Diff blob layout (little-endian, identical to Rust): `u32 newLen`, `u32
 * changeCount`, `changeCount × (u32 offset, u8 byte)`, then a length-prefixed
 * `tail`. Reconstruct: take `base[..common]` (common = min(baseLen, newLen)),
 * overwrite each `offset` with `byte`, append `tail`.
 */

import { type ByteReader, byteReader } from "./byte-reader.ts";
import {
  KIND_SERVER_SNAPSHOT_FOR_DELTA,
  MAX_PAYLOAD_LEN,
  type PlayerAck,
  type ServerSnapshotForDeltaMessage,
} from "./messages.ts";
import { assertPayload, readExpectedKind, writeHeader } from "./frame.ts";
import { readAcks, writeAcks } from "./per-player-codec.ts";
import { assert } from "./protocol-error.ts";
import { byteWriter } from "./byte-writer.ts";
import { each } from "./control-flow.ts";

const START = 0;

/** One byte changed at `offset` between the base and the new payload. */
interface Change {
  readonly byte: number;
  readonly offset: number;
}

/** Fields of an {@link encodeServerSnapshotForDelta} frame. */
export interface ServerSnapshotForDeltaFields {
  readonly serverTick: number;
  readonly acks: readonly PlayerAck[];
  readonly baseTick: number;
  readonly base: Uint8Array;
  readonly next: Uint8Array;
}

// Narrow an indexed byte to `number`, gated on an in-range check (the Uint8Array analogue of `pick`).
const presentByte: (value: number | undefined, inRange: boolean) => asserts value is number = (
  _value,
  inRange,
): void => {
  assert(inRange, "byte index out of range");
};

const byteAt = (bytes: Uint8Array, index: number): number => {
  const value = bytes[index];
  presentByte(value, index < bytes.length);
  return value;
};

/** Compute the diff blob that turns `base` into `next` (twin of Rust `diff`). */
export const diffSnapshot = (base: Uint8Array, next: Uint8Array): Uint8Array => {
  const common = Math.min(base.length, next.length);
  const changes: readonly Change[] = Array.from({ length: common }, (_unused, index): number => index)
    .filter((index): boolean => byteAt(base, index) !== byteAt(next, index))
    .map((index): Change => ({ byte: byteAt(next, index), offset: index }));
  const writer = byteWriter();
  writer.u32(next.length);
  writer.u32(changes.length);
  each(changes, (change): void => {
    writer.u32(change.offset);
    writer.u8(change.byte);
  });
  writer.byteSlice(next.slice(common));
  return writer.finish();
};

// Read the change list (offset then byte, in wire order) into a sorted-key record.
const readChanges = (reader: ByteReader, count: number): readonly Change[] =>
  Array.from({ length: count }, (): Change => {
    const offset = reader.u32();
    const byte = reader.u8();
    return { byte, offset };
  });

// Overwrite the shared prefix with the changes, gating each offset to the prefix.
const applyChanges = (result: Uint8Array, changes: readonly Change[], common: number): void => {
  each(changes, (change): void => {
    assert(change.offset < common, "delta change offset is out of range");
    result[change.offset] = change.byte;
  });
};

/** The validated leading fields of a diff blob: the new length, the shared prefix
 * length, and the change count. */
interface DeltaHeader {
  readonly common: number;
  readonly count: number;
  readonly newLen: number;
}

// Read + validate the `newLen` / `changeCount` header against the base length.
const readDeltaHeader = (reader: ByteReader, baseLen: number): DeltaHeader => {
  const newLen = reader.u32();
  assert(newLen <= MAX_PAYLOAD_LEN, "delta declares a payload over the maximum length");
  const common = Math.min(baseLen, newLen);
  const count = reader.u32();
  assert(count <= common, "delta declares more changes than the shared prefix holds");
  return { common, count, newLen };
};

// Concatenate the patched shared prefix and the verbatim tail into the new payload.
const joinPrefixAndTail = (prefix: Uint8Array, tail: Uint8Array, common: number): Uint8Array => {
  const out = new Uint8Array(common + tail.length);
  out.set(prefix, START);
  out.set(tail, common);
  return out;
};

/** Reconstruct the new payload from `base` and a diff `blob` (twin of Rust `apply`). */
export const applySnapshot = (base: Uint8Array, blob: Uint8Array): Uint8Array => {
  const reader = byteReader(blob);
  const { common, count, newLen } = readDeltaHeader(reader, base.length);
  const result = base.slice(START, common);
  applyChanges(result, readChanges(reader, count), common);
  const tail = reader.byteSlice();
  assert(common + tail.length === newLen, "delta tail length is inconsistent with the declared length");
  return joinPrefixAndTail(result, tail, common);
};

/** Encode a per-player delta snapshot frame (twin of Rust `ServerSnapshotForDelta::encode`). */
export const encodeServerSnapshotForDelta = (fields: ServerSnapshotForDeltaFields): Uint8Array => {
  const delta = diffSnapshot(fields.base, fields.next);
  assertPayload(delta);
  const writer = byteWriter();
  writeHeader(writer, KIND_SERVER_SNAPSHOT_FOR_DELTA);
  writer.u64(fields.serverTick);
  writer.u64(fields.baseTick);
  writeAcks(writer, fields.acks);
  writer.byteSlice(delta);
  return writer.finish();
};

/** Decode a per-player delta snapshot frame (twin of Rust `ServerSnapshotForDelta::decode`). */
export const decodeServerSnapshotForDelta = (bytes: Uint8Array): ServerSnapshotForDeltaMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_SERVER_SNAPSHOT_FOR_DELTA);
  const serverTick = reader.u64();
  const baseTick = reader.u64();
  const acks = readAcks(reader);
  const delta = reader.byteSlice();
  assertPayload(delta);
  return { acks, baseTick, delta, kind: KIND_SERVER_SNAPSHOT_FOR_DELTA, serverTick };
};
