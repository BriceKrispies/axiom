/*
 * The per-player-addressed wire frames — the byte-for-byte twins of the Rust
 * `client_intent_for` / `server_snapshot_for` modules. `ClientIntentFor` prefixes
 * the anonymous intent body with the originating player seat; `ServerSnapshotFor`
 * carries a bounded list of per-player `(player, sequence)` acknowledgements
 * instead of a single anonymous ack. They reuse the shared `frame.ts` envelope
 * and the same DataView writer/reader as `codec.ts`, so their bytes stay
 * identical to the Rust encoders.
 */

import { type ByteReader, byteReader } from "./byte-reader.ts";
import { type ByteWriter, byteWriter } from "./byte-writer.ts";
import {
  type ClientIntentForMessage,
  KIND_CLIENT_INTENT_FOR,
  KIND_SERVER_SNAPSHOT_FOR,
  MAX_ACKS,
  type PlayerAck,
  type ServerSnapshotForMessage,
} from "./messages.ts";
import { assertPayload, readExpectedKind, writeHeader } from "./frame.ts";
import { assert } from "./protocol-error.ts";
import { each } from "./control-flow.ts";

/** Reject an ack list longer than {@link MAX_ACKS} (the shared per-player bound). */
export const assertAckCount = (count: number): void => {
  assert(count <= MAX_ACKS, "server snapshot ack list exceeds the maximum count");
};

// Read one `(player, sequence)` acknowledgement pair in wire order.
const readAck = (reader: ByteReader): PlayerAck => ({ player: reader.u64(), sequence: reader.u64() });

/** Write the count-prefixed `(player, sequence)` ack list (twin of Rust `acks::write_acks`). */
export const writeAcks = (writer: ByteWriter, acks: readonly PlayerAck[]): void => {
  assertAckCount(acks.length);
  writer.u32(acks.length);
  each(acks, (ack): void => {
    writer.u64(ack.player);
    writer.u64(ack.sequence);
  });
};

/** Read the count-prefixed ack list, re-validating the bound (twin of Rust `acks::read_acks`). */
export const readAcks = (reader: ByteReader): readonly PlayerAck[] => {
  const count = reader.u32();
  assertAckCount(count);
  return Array.from({ length: count }, (): PlayerAck => readAck(reader));
};

/** Fields of a {@link encodeClientIntentFor} frame (the per-player intent twin). */
export interface ClientIntentForFields {
  readonly player: number;
  readonly clientSequence: number;
  readonly predictedClientTick: number;
  readonly lastSeenServerTick: number;
  readonly payload: Uint8Array;
}

export const encodeClientIntentFor = (fields: ClientIntentForFields): Uint8Array => {
  assertPayload(fields.payload);
  const writer = byteWriter();
  writeHeader(writer, KIND_CLIENT_INTENT_FOR);
  writer.u64(fields.player);
  writer.u64(fields.clientSequence);
  writer.u64(fields.predictedClientTick);
  writer.u64(fields.lastSeenServerTick);
  writer.byteSlice(fields.payload);
  return writer.finish();
};

export const encodeServerSnapshotFor = (
  serverTick: number,
  acks: readonly PlayerAck[],
  payload: Uint8Array,
): Uint8Array => {
  assertAckCount(acks.length);
  assertPayload(payload);
  const writer = byteWriter();
  writeHeader(writer, KIND_SERVER_SNAPSHOT_FOR);
  writer.u64(serverTick);
  writeAcks(writer, acks);
  writer.byteSlice(payload);
  return writer.finish();
};

export const decodeClientIntentFor = (bytes: Uint8Array): ClientIntentForMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_CLIENT_INTENT_FOR);
  const player = reader.u64();
  const clientSequence = reader.u64();
  const predictedClientTick = reader.u64();
  const lastSeenServerTick = reader.u64();
  const payload = reader.byteSlice();
  assertPayload(payload);
  return { clientSequence, kind: KIND_CLIENT_INTENT_FOR, lastSeenServerTick, payload, player, predictedClientTick };
};

export const decodeServerSnapshotFor = (bytes: Uint8Array): ServerSnapshotForMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_SERVER_SNAPSHOT_FOR);
  const serverTick = reader.u64();
  const acks = readAcks(reader);
  const payload = reader.byteSlice();
  assertPayload(payload);
  return { acks, kind: KIND_SERVER_SNAPSHOT_FOR, payload, serverTick };
};
