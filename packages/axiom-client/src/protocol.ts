// A small, dependency-free encoder/decoder that mirrors the `axiom-net-protocol`
// Rust module byte-for-byte. The Rust engine module owns the canonical contract;
// this file is the browser-side twin so the TypeScript SDK can speak the same
// wire format without a build step. Keep the two in sync: the framing here
// (little-endian, SchemaVersion header + one-byte kind, fixed field order) is the
// same format documented in `modules/axiom-net-protocol/ARCHITECTURE.md`.

// Wire-codec format version (the *encoding* version; compatibility is by major).
export const WIRE_MAJOR = 1;
export const WIRE_MINOR = 0;

// Stable one-byte message-kind discriminants (must match the Rust `frame` module).
export const KIND_JOIN_ROOM = 0;
export const KIND_LEAVE_ROOM = 1;
export const KIND_CLIENT_INTENT = 2;
export const KIND_WELCOME = 3;
export const KIND_SERVER_SNAPSHOT = 4;
export const KIND_SERVER_EVENT = 5;
export const KIND_REJECTED_INTENT = 6;
const KIND_MAX = KIND_REJECTED_INTENT;

// Documented size bounds (must match the Rust module).
export const MAX_ROOM_ID_LEN = 64;
export const MAX_PAYLOAD_LEN = 64 * 1024;

// Well-known machine-readable reject reasons.
export const REASON_UNSPECIFIED = 0;
export const REASON_MALFORMED = 1;
export const REASON_OUT_OF_ORDER = 2;
export const REASON_NOT_IN_ROOM = 3;

/** Thrown when a frame is malformed, truncated, out of bounds, or invalid. */
export class ProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ProtocolError";
  }
}

// --- decoded message shapes (a discriminated union on `kind`) ---

export interface JoinRoomMessage {
  kind: typeof KIND_JOIN_ROOM;
  protocolVersion: number;
  roomId: Uint8Array;
  token: Uint8Array;
}
export interface LeaveRoomMessage {
  kind: typeof KIND_LEAVE_ROOM;
  roomId: Uint8Array;
}
export interface ClientIntentMessage {
  kind: typeof KIND_CLIENT_INTENT;
  clientSequence: number;
  predictedClientTick: number;
  lastSeenServerTick: number;
  payload: Uint8Array;
}
export interface WelcomeMessage {
  kind: typeof KIND_WELCOME;
  protocolVersion: number;
  clientId: number;
  serverTick: number;
  fixedStepNs: number;
}
export interface ServerSnapshotMessage {
  kind: typeof KIND_SERVER_SNAPSHOT;
  serverTick: number;
  lastAcceptedClientSequence: number;
  payload: Uint8Array;
}
export interface ServerEventMessage {
  kind: typeof KIND_SERVER_EVENT;
  serverTick: number;
  payload: Uint8Array;
}
export interface RejectedIntentMessage {
  kind: typeof KIND_REJECTED_INTENT;
  clientSequence: number;
  reasonCode: number;
}

export type DecodedMessage =
  | JoinRoomMessage
  | LeaveRoomMessage
  | ClientIntentMessage
  | WelcomeMessage
  | ServerSnapshotMessage
  | ServerEventMessage
  | RejectedIntentMessage;

// --- little-endian byte writer / reader ---

class ByteWriter {
  private parts: number[] = [];
  u8(v: number): void {
    this.parts.push(v & 0xff);
  }
  u16(v: number): void {
    this.parts.push(v & 0xff, (v >>> 8) & 0xff);
  }
  u32(v: number): void {
    this.parts.push(v & 0xff, (v >>> 8) & 0xff, (v >>> 16) & 0xff, (v >>> 24) & 0xff);
  }
  u64(v: number): void {
    let b = BigInt(v);
    for (let i = 0; i < 8; i++) {
      this.parts.push(Number(b & 0xffn));
      b >>= 8n;
    }
  }
  byteSlice(data: Uint8Array): void {
    this.u32(data.length);
    for (const byte of data) this.parts.push(byte);
  }
  finish(): Uint8Array {
    return Uint8Array.from(this.parts);
  }
}

class ByteReader {
  private data: Uint8Array;
  private view: DataView;
  private pos = 0;
  constructor(data: Uint8Array) {
    this.data = data;
    this.view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  }
  private need(n: number): void {
    if (this.pos + n > this.data.length) {
      throw new ProtocolError("frame ended before a value could be read");
    }
  }
  u8(): number {
    this.need(1);
    return this.view.getUint8(this.pos++);
  }
  u16(): number {
    this.need(2);
    const v = this.view.getUint16(this.pos, true);
    this.pos += 2;
    return v;
  }
  u32(): number {
    this.need(4);
    const v = this.view.getUint32(this.pos, true);
    this.pos += 4;
    return v;
  }
  u64(): number {
    this.need(8);
    const v = this.view.getBigUint64(this.pos, true);
    this.pos += 8;
    return Number(v);
  }
  byteSlice(): Uint8Array {
    const len = this.u32();
    this.need(len);
    const out = this.data.slice(this.pos, this.pos + len);
    this.pos += len;
    return out;
  }
}

// --- header + field validation (mirrors the Rust validators) ---

function writeHeader(w: ByteWriter, kind: number): void {
  w.u16(WIRE_MAJOR);
  w.u16(WIRE_MINOR);
  w.u8(kind);
}

function readCompatibleVersion(r: ByteReader): void {
  const major = r.u16();
  r.u16(); // minor: read but not compatibility-checked
  if (major !== WIRE_MAJOR) {
    throw new ProtocolError(`incompatible wire version major ${major}`);
  }
}

function readExpectedKind(r: ByteReader, expected: number): void {
  readCompatibleVersion(r);
  const kind = r.u8();
  if (kind !== expected) {
    throw new ProtocolError(`expected message kind ${expected}, got ${kind}`);
  }
}

/** Peek the message kind of an encoded frame, validating version and range. */
export function peekKind(bytes: Uint8Array): number {
  const r = new ByteReader(bytes);
  readCompatibleVersion(r);
  const kind = r.u8();
  if (kind > KIND_MAX) {
    throw new ProtocolError(`unknown message kind ${kind}`);
  }
  return kind;
}

function validateProtocolVersion(v: number): void {
  if (v === 0) throw new ProtocolError("protocol version must be nonzero");
}
function validateClientId(v: number): void {
  if (v === 0) throw new ProtocolError("client id must be nonzero");
}
function validateRoomId(bytes: Uint8Array): void {
  if (bytes.length === 0 || bytes.length > MAX_ROOM_ID_LEN) {
    throw new ProtocolError("room id must be non-empty and within the maximum length");
  }
}
function validatePayload(bytes: Uint8Array): void {
  if (bytes.length > MAX_PAYLOAD_LEN) {
    throw new ProtocolError("opaque payload exceeds the maximum byte length");
  }
}
function validateFixedStep(v: number): void {
  if (v === 0) throw new ProtocolError("fixed step must be nonzero");
}

// --- encoders ---

export function encodeJoinRoom(protocolVersion: number, roomId: Uint8Array, token: Uint8Array): Uint8Array {
  validateProtocolVersion(protocolVersion);
  validateRoomId(roomId);
  validatePayload(token);
  const w = new ByteWriter();
  writeHeader(w, KIND_JOIN_ROOM);
  w.u32(protocolVersion);
  w.byteSlice(roomId);
  w.byteSlice(token);
  return w.finish();
}

export function encodeLeaveRoom(roomId: Uint8Array): Uint8Array {
  validateRoomId(roomId);
  const w = new ByteWriter();
  writeHeader(w, KIND_LEAVE_ROOM);
  w.byteSlice(roomId);
  return w.finish();
}

export function encodeClientIntent(
  clientSequence: number,
  predictedClientTick: number,
  lastSeenServerTick: number,
  payload: Uint8Array,
): Uint8Array {
  validatePayload(payload);
  const w = new ByteWriter();
  writeHeader(w, KIND_CLIENT_INTENT);
  w.u64(clientSequence);
  w.u64(predictedClientTick);
  w.u64(lastSeenServerTick);
  w.byteSlice(payload);
  return w.finish();
}

export function encodeWelcome(
  protocolVersion: number,
  clientId: number,
  serverTick: number,
  fixedStepNs: number,
): Uint8Array {
  validateProtocolVersion(protocolVersion);
  validateClientId(clientId);
  validateFixedStep(fixedStepNs);
  const w = new ByteWriter();
  writeHeader(w, KIND_WELCOME);
  w.u32(protocolVersion);
  w.u64(clientId);
  w.u64(serverTick);
  w.u64(fixedStepNs);
  return w.finish();
}

export function encodeServerSnapshot(
  serverTick: number,
  lastAcceptedClientSequence: number,
  payload: Uint8Array,
): Uint8Array {
  validatePayload(payload);
  const w = new ByteWriter();
  writeHeader(w, KIND_SERVER_SNAPSHOT);
  w.u64(serverTick);
  w.u64(lastAcceptedClientSequence);
  w.byteSlice(payload);
  return w.finish();
}

export function encodeServerEvent(serverTick: number, payload: Uint8Array): Uint8Array {
  validatePayload(payload);
  const w = new ByteWriter();
  writeHeader(w, KIND_SERVER_EVENT);
  w.u64(serverTick);
  w.byteSlice(payload);
  return w.finish();
}

export function encodeRejectedIntent(clientSequence: number, reasonCode: number): Uint8Array {
  const w = new ByteWriter();
  writeHeader(w, KIND_REJECTED_INTENT);
  w.u64(clientSequence);
  w.u32(reasonCode);
  return w.finish();
}

// --- decoders ---

export function decodeJoinRoom(bytes: Uint8Array): JoinRoomMessage {
  const r = new ByteReader(bytes);
  readExpectedKind(r, KIND_JOIN_ROOM);
  const protocolVersion = r.u32();
  validateProtocolVersion(protocolVersion);
  const roomId = r.byteSlice();
  validateRoomId(roomId);
  const token = r.byteSlice();
  validatePayload(token);
  return { kind: KIND_JOIN_ROOM, protocolVersion, roomId, token };
}

export function decodeLeaveRoom(bytes: Uint8Array): LeaveRoomMessage {
  const r = new ByteReader(bytes);
  readExpectedKind(r, KIND_LEAVE_ROOM);
  const roomId = r.byteSlice();
  validateRoomId(roomId);
  return { kind: KIND_LEAVE_ROOM, roomId };
}

export function decodeClientIntent(bytes: Uint8Array): ClientIntentMessage {
  const r = new ByteReader(bytes);
  readExpectedKind(r, KIND_CLIENT_INTENT);
  const clientSequence = r.u64();
  const predictedClientTick = r.u64();
  const lastSeenServerTick = r.u64();
  const payload = r.byteSlice();
  validatePayload(payload);
  return { kind: KIND_CLIENT_INTENT, clientSequence, predictedClientTick, lastSeenServerTick, payload };
}

export function decodeWelcome(bytes: Uint8Array): WelcomeMessage {
  const r = new ByteReader(bytes);
  readExpectedKind(r, KIND_WELCOME);
  const protocolVersion = r.u32();
  validateProtocolVersion(protocolVersion);
  const clientId = r.u64();
  validateClientId(clientId);
  const serverTick = r.u64();
  const fixedStepNs = r.u64();
  validateFixedStep(fixedStepNs);
  return { kind: KIND_WELCOME, protocolVersion, clientId, serverTick, fixedStepNs };
}

export function decodeServerSnapshot(bytes: Uint8Array): ServerSnapshotMessage {
  const r = new ByteReader(bytes);
  readExpectedKind(r, KIND_SERVER_SNAPSHOT);
  const serverTick = r.u64();
  const lastAcceptedClientSequence = r.u64();
  const payload = r.byteSlice();
  validatePayload(payload);
  return { kind: KIND_SERVER_SNAPSHOT, serverTick, lastAcceptedClientSequence, payload };
}

export function decodeServerEvent(bytes: Uint8Array): ServerEventMessage {
  const r = new ByteReader(bytes);
  readExpectedKind(r, KIND_SERVER_EVENT);
  const serverTick = r.u64();
  const payload = r.byteSlice();
  validatePayload(payload);
  return { kind: KIND_SERVER_EVENT, serverTick, payload };
}

export function decodeRejectedIntent(bytes: Uint8Array): RejectedIntentMessage {
  const r = new ByteReader(bytes);
  readExpectedKind(r, KIND_REJECTED_INTENT);
  const clientSequence = r.u64();
  const reasonCode = r.u32();
  return { kind: KIND_REJECTED_INTENT, clientSequence, reasonCode };
}

/** Decode any frame, dispatching on its (validated) kind. */
export function decodeFrame(bytes: Uint8Array): DecodedMessage {
  const kind = peekKind(bytes);
  switch (kind) {
    case KIND_JOIN_ROOM:
      return decodeJoinRoom(bytes);
    case KIND_LEAVE_ROOM:
      return decodeLeaveRoom(bytes);
    case KIND_CLIENT_INTENT:
      return decodeClientIntent(bytes);
    case KIND_WELCOME:
      return decodeWelcome(bytes);
    case KIND_SERVER_SNAPSHOT:
      return decodeServerSnapshot(bytes);
    case KIND_SERVER_EVENT:
      return decodeServerEvent(bytes);
    default:
      return decodeRejectedIntent(bytes);
  }
}
