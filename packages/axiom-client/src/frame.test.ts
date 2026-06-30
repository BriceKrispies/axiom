import assert from "node:assert/strict";
import { test } from "node:test";

import {
  assertClientId,
  assertFixedStep,
  assertPayload,
  assertProtocolVersion,
  assertRoomId,
  readCompatibleVersion,
  readExpectedKind,
  writeHeader,
} from "./frame.ts";
import { KIND_JOIN_ROOM, KIND_WELCOME, MAX_PAYLOAD_LEN, MAX_ROOM_ID_LEN } from "./messages.ts";
import { byteReader } from "./byte-reader.ts";
import { byteWriter } from "./byte-writer.ts";
import { ProtocolError } from "./protocol-error.ts";

const u8 = (...bytes: number[]): Uint8Array => Uint8Array.from(bytes);

test("assertProtocolVersion accepts a nonzero version and rejects zero", () => {
  assert.doesNotThrow(() => {
    assertProtocolVersion(1);
  });
  assert.throws(() => {
    assertProtocolVersion(0);
  }, ProtocolError);
});

test("assertClientId accepts a nonzero id and rejects zero", () => {
  assert.doesNotThrow(() => {
    assertClientId(7);
  });
  assert.throws(() => {
    assertClientId(0);
  }, ProtocolError);
});

test("assertFixedStep accepts a nonzero step and rejects zero", () => {
  assert.doesNotThrow(() => {
    assertFixedStep(16_666_667);
  });
  assert.throws(() => {
    assertFixedStep(0);
  }, ProtocolError);
});

test("assertRoomId accepts a bounded non-empty id and rejects empty and over-size ids", () => {
  assert.doesNotThrow(() => {
    assertRoomId(u8(108, 111, 98));
  });
  assert.doesNotThrow(() => {
    assertRoomId(new Uint8Array(MAX_ROOM_ID_LEN));
  });
  assert.throws(() => {
    assertRoomId(u8());
  }, ProtocolError);
  assert.throws(() => {
    assertRoomId(new Uint8Array(MAX_ROOM_ID_LEN + 1));
  }, ProtocolError);
});

test("assertPayload accepts a bounded payload and rejects an over-size payload", () => {
  assert.doesNotThrow(() => {
    assertPayload(new Uint8Array(MAX_PAYLOAD_LEN));
  });
  assert.throws(() => {
    assertPayload(new Uint8Array(MAX_PAYLOAD_LEN + 1));
  }, ProtocolError);
});

test("writeHeader writes the major u16, minor u16, then the one-byte kind", () => {
  const writer = byteWriter();
  writeHeader(writer, KIND_WELCOME);
  assert.deepEqual(Array.from(writer.finish()), [1, 0, 0, 0, KIND_WELCOME]);
});

test("readCompatibleVersion accepts a matching major and rejects an incompatible one", () => {
  assert.doesNotThrow(() => {
    readCompatibleVersion(byteReader(u8(1, 0, 0, 0)));
  });
  assert.throws(() => {
    readCompatibleVersion(byteReader(u8(2, 0, 0, 0)));
  }, ProtocolError);
});

test("readExpectedKind accepts the expected kind and rejects a mismatched one", () => {
  const writer = byteWriter();
  writeHeader(writer, KIND_WELCOME);
  const frame = writer.finish();
  assert.doesNotThrow(() => {
    readExpectedKind(byteReader(frame), KIND_WELCOME);
  });
  assert.throws(() => {
    readExpectedKind(byteReader(frame), KIND_JOIN_ROOM);
  }, ProtocolError);
});
