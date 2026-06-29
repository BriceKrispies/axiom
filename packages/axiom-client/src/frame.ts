/*
 * The versioned frame envelope and field validators shared by every wire
 * message — the byte-for-byte twin of the Rust `axiom-net-protocol` `frame`
 * module. Every frame is a header (`WIRE_MAJOR` u16, `WIRE_MINOR` u16, then a
 * one-byte kind) followed by its body; the validators mirror the Rust message
 * constructors. Both `codec.ts` (the single-seat frames) and `per-player-codec.ts`
 * (the addressed twins) build on these, so the envelope lives in one place and
 * the on-wire bytes can never drift across the message codecs.
 */

import { MAX_PAYLOAD_LEN, MAX_ROOM_ID_LEN, WIRE_MAJOR, WIRE_MINOR } from "./messages.ts";
import type { ByteReader } from "./byte-reader.ts";
import type { ByteWriter } from "./byte-writer.ts";
import { assert } from "./protocol-error.ts";

const ZERO = 0;

// --- field validation (mirrors the Rust validators) ---

export const assertProtocolVersion = (value: number): void => {
  assert(value !== ZERO, "protocol version must be nonzero");
};
export const assertClientId = (value: number): void => {
  assert(value !== ZERO, "client id must be nonzero");
};
export const assertFixedStep = (value: number): void => {
  assert(value !== ZERO, "fixed step must be nonzero");
};
export const assertRoomId = (bytes: Uint8Array): void => {
  assert(bytes.length !== ZERO, "room id must be non-empty");
  assert(bytes.length <= MAX_ROOM_ID_LEN, "room id exceeds the maximum length");
};
export const assertPayload = (bytes: Uint8Array): void => {
  assert(bytes.length <= MAX_PAYLOAD_LEN, "opaque payload exceeds the maximum byte length");
};

// --- header read/write ---

export const writeHeader = (writer: ByteWriter, kind: number): void => {
  writer.u16(WIRE_MAJOR);
  writer.u16(WIRE_MINOR);
  writer.u8(kind);
};

export const readCompatibleVersion = (reader: ByteReader): void => {
  const major = reader.u16();
  // Minor is read to advance the cursor but is not compatibility-checked.
  reader.u16();
  assert(major === WIRE_MAJOR, `incompatible wire version major ${major}`);
};

export const readExpectedKind = (reader: ByteReader, expected: number): void => {
  readCompatibleVersion(reader);
  const kind = reader.u8();
  assert(kind === expected, `expected message kind ${expected}, got ${kind}`);
};
