/*
 * The wire encoder/decoder: a dependency-free twin of the `axiom-net-protocol`
 * Rust module, byte-for-byte. Each encoder validates its fields and writes a
 * header + body; each decoder validates the version and kind, then reads the
 * body. Dispatch is a total lookup table keyed by the message kind, so there is
 * no `switch`.
 */

import { type ByteReader, byteReader } from "./byte-reader.ts";
import { type ByteWriter, byteWriter } from "./byte-writer.ts";
import {
  type ClientIntentMessage,
  type DecodedKind,
  type DecodedMessage,
  type JoinRoomMessage,
  KIND_CLIENT_INTENT,
  KIND_JOIN_ROOM,
  KIND_LEAVE_ROOM,
  KIND_REJECTED_INTENT,
  KIND_SERVER_EVENT,
  KIND_SERVER_SNAPSHOT,
  KIND_WELCOME,
  type LeaveRoomMessage,
  MAX_PAYLOAD_LEN,
  MAX_ROOM_ID_LEN,
  type RejectedIntentMessage,
  type ServerEventMessage,
  type ServerSnapshotMessage,
  WIRE_MAJOR,
  WIRE_MINOR,
  type WelcomeMessage,
} from "./messages.ts";
import { assert } from "./protocol-error.ts";

const ZERO = 0;

// --- field validation (mirrors the Rust validators) ---

const assertProtocolVersion = (value: number): void => {
  assert(value !== ZERO, "protocol version must be nonzero");
};
const assertClientId = (value: number): void => {
  assert(value !== ZERO, "client id must be nonzero");
};
const assertFixedStep = (value: number): void => {
  assert(value !== ZERO, "fixed step must be nonzero");
};
const assertRoomId = (bytes: Uint8Array): void => {
  assert(bytes.length !== ZERO, "room id must be non-empty");
  assert(bytes.length <= MAX_ROOM_ID_LEN, "room id exceeds the maximum length");
};
const assertPayload = (bytes: Uint8Array): void => {
  assert(bytes.length <= MAX_PAYLOAD_LEN, "opaque payload exceeds the maximum byte length");
};

// --- header read/write ---

const writeHeader = (writer: ByteWriter, kind: number): void => {
  writer.u16(WIRE_MAJOR);
  writer.u16(WIRE_MINOR);
  writer.u8(kind);
};

const readCompatibleVersion = (reader: ByteReader): void => {
  const major = reader.u16();
  // Minor is read to advance the cursor but is not compatibility-checked.
  reader.u16();
  assert(major === WIRE_MAJOR, `incompatible wire version major ${major}`);
};

const readExpectedKind = (reader: ByteReader, expected: number): void => {
  readCompatibleVersion(reader);
  const kind = reader.u8();
  assert(kind === expected, `expected message kind ${expected}, got ${kind}`);
};

// --- field bundles for encoders that carry more than three values ---

/** Fields of a {@link encodeClientIntent} frame. */
export interface ClientIntentFields {
  readonly clientSequence: number;
  readonly predictedClientTick: number;
  readonly lastSeenServerTick: number;
  readonly payload: Uint8Array;
}

/** Fields of a {@link encodeWelcome} frame. */
export interface WelcomeFields {
  readonly protocolVersion: number;
  readonly clientId: number;
  readonly serverTick: number;
  readonly fixedStepNs: number;
}

// --- encoders ---

export const encodeJoinRoom = (
  protocolVersion: number,
  roomId: Uint8Array,
  token: Uint8Array,
): Uint8Array => {
  assertProtocolVersion(protocolVersion);
  assertRoomId(roomId);
  assertPayload(token);
  const writer = byteWriter();
  writeHeader(writer, KIND_JOIN_ROOM);
  writer.u32(protocolVersion);
  writer.byteSlice(roomId);
  writer.byteSlice(token);
  return writer.finish();
};

export const encodeLeaveRoom = (roomId: Uint8Array): Uint8Array => {
  assertRoomId(roomId);
  const writer = byteWriter();
  writeHeader(writer, KIND_LEAVE_ROOM);
  writer.byteSlice(roomId);
  return writer.finish();
};

export const encodeClientIntent = (fields: ClientIntentFields): Uint8Array => {
  assertPayload(fields.payload);
  const writer = byteWriter();
  writeHeader(writer, KIND_CLIENT_INTENT);
  writer.u64(fields.clientSequence);
  writer.u64(fields.predictedClientTick);
  writer.u64(fields.lastSeenServerTick);
  writer.byteSlice(fields.payload);
  return writer.finish();
};

export const encodeWelcome = (fields: WelcomeFields): Uint8Array => {
  assertProtocolVersion(fields.protocolVersion);
  assertClientId(fields.clientId);
  assertFixedStep(fields.fixedStepNs);
  const writer = byteWriter();
  writeHeader(writer, KIND_WELCOME);
  writer.u32(fields.protocolVersion);
  writer.u64(fields.clientId);
  writer.u64(fields.serverTick);
  writer.u64(fields.fixedStepNs);
  return writer.finish();
};

export const encodeServerSnapshot = (
  serverTick: number,
  lastAcceptedClientSequence: number,
  payload: Uint8Array,
): Uint8Array => {
  assertPayload(payload);
  const writer = byteWriter();
  writeHeader(writer, KIND_SERVER_SNAPSHOT);
  writer.u64(serverTick);
  writer.u64(lastAcceptedClientSequence);
  writer.byteSlice(payload);
  return writer.finish();
};

export const encodeServerEvent = (serverTick: number, payload: Uint8Array): Uint8Array => {
  assertPayload(payload);
  const writer = byteWriter();
  writeHeader(writer, KIND_SERVER_EVENT);
  writer.u64(serverTick);
  writer.byteSlice(payload);
  return writer.finish();
};

export const encodeRejectedIntent = (clientSequence: number, reasonCode: number): Uint8Array => {
  const writer = byteWriter();
  writeHeader(writer, KIND_REJECTED_INTENT);
  writer.u64(clientSequence);
  writer.u32(reasonCode);
  return writer.finish();
};

// --- decoders ---

export const decodeJoinRoom = (bytes: Uint8Array): JoinRoomMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_JOIN_ROOM);
  const protocolVersion = reader.u32();
  assertProtocolVersion(protocolVersion);
  const roomId = reader.byteSlice();
  assertRoomId(roomId);
  const token = reader.byteSlice();
  assertPayload(token);
  return { kind: KIND_JOIN_ROOM, protocolVersion, roomId, token };
};

export const decodeLeaveRoom = (bytes: Uint8Array): LeaveRoomMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_LEAVE_ROOM);
  const roomId = reader.byteSlice();
  assertRoomId(roomId);
  return { kind: KIND_LEAVE_ROOM, roomId };
};

export const decodeClientIntent = (bytes: Uint8Array): ClientIntentMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_CLIENT_INTENT);
  const clientSequence = reader.u64();
  const predictedClientTick = reader.u64();
  const lastSeenServerTick = reader.u64();
  const payload = reader.byteSlice();
  assertPayload(payload);
  return { clientSequence, kind: KIND_CLIENT_INTENT, lastSeenServerTick, payload, predictedClientTick };
};

export const decodeWelcome = (bytes: Uint8Array): WelcomeMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_WELCOME);
  const protocolVersion = reader.u32();
  assertProtocolVersion(protocolVersion);
  const clientId = reader.u64();
  assertClientId(clientId);
  const serverTick = reader.u64();
  const fixedStepNs = reader.u64();
  assertFixedStep(fixedStepNs);
  return { clientId, fixedStepNs, kind: KIND_WELCOME, protocolVersion, serverTick };
};

export const decodeServerSnapshot = (bytes: Uint8Array): ServerSnapshotMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_SERVER_SNAPSHOT);
  const serverTick = reader.u64();
  const lastAcceptedClientSequence = reader.u64();
  const payload = reader.byteSlice();
  assertPayload(payload);
  return { kind: KIND_SERVER_SNAPSHOT, lastAcceptedClientSequence, payload, serverTick };
};

export const decodeServerEvent = (bytes: Uint8Array): ServerEventMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_SERVER_EVENT);
  const serverTick = reader.u64();
  const payload = reader.byteSlice();
  assertPayload(payload);
  return { kind: KIND_SERVER_EVENT, payload, serverTick };
};

export const decodeRejectedIntent = (bytes: Uint8Array): RejectedIntentMessage => {
  const reader = byteReader(bytes);
  readExpectedKind(reader, KIND_REJECTED_INTENT);
  const clientSequence = reader.u64();
  const reasonCode = reader.u32();
  return { clientSequence, kind: KIND_REJECTED_INTENT, reasonCode };
};

// --- kind peeking + total-table dispatch ---

const KIND_SET: ReadonlySet<number> = new Set<number>([
  KIND_JOIN_ROOM,
  KIND_LEAVE_ROOM,
  KIND_CLIENT_INTENT,
  KIND_WELCOME,
  KIND_SERVER_SNAPSHOT,
  KIND_SERVER_EVENT,
  KIND_REJECTED_INTENT,
]);

// Narrow a raw byte to a DecodedKind, throwing when it is not a known kind.
const assertKind: (raw: number) => asserts raw is DecodedKind = (raw): void => {
  assert(KIND_SET.has(raw), `unknown message kind ${raw}`);
};

/** Peek the (validated) message kind of an encoded frame. */
export const peekKind = (bytes: Uint8Array): DecodedKind => {
  const reader = byteReader(bytes);
  readCompatibleVersion(reader);
  const raw = reader.u8();
  assertKind(raw);
  return raw;
};

const DECODERS: Readonly<Record<DecodedKind, (bytes: Uint8Array) => DecodedMessage>> = {
  [KIND_JOIN_ROOM]: decodeJoinRoom,
  [KIND_LEAVE_ROOM]: decodeLeaveRoom,
  [KIND_CLIENT_INTENT]: decodeClientIntent,
  [KIND_WELCOME]: decodeWelcome,
  [KIND_SERVER_SNAPSHOT]: decodeServerSnapshot,
  [KIND_SERVER_EVENT]: decodeServerEvent,
  [KIND_REJECTED_INTENT]: decodeRejectedIntent,
};

/** Decode any frame, dispatching on its validated kind. */
export const decodeFrame = (bytes: Uint8Array): DecodedMessage => DECODERS[peekKind(bytes)](bytes);
