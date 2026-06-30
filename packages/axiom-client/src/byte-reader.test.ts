import assert from "node:assert/strict";
import { test } from "node:test";

import { byteReader } from "./byte-reader.ts";
import { ProtocolError } from "./protocol-error.ts";

const bytes = (...values: number[]): Uint8Array => Uint8Array.from(values);

test("u8 reads a single byte and advances", () => {
  const reader = byteReader(bytes(42, 7));
  assert.equal(reader.u8(), 42);
  assert.equal(reader.u8(), 7);
});

test("u16 reads a little-endian 16-bit value", () => {
  assert.equal(byteReader(bytes(1, 0)).u16(), 1);
  assert.equal(byteReader(bytes(255, 255)).u16(), 65_535);
  assert.equal(byteReader(bytes(0x34, 0x12)).u16(), 4660);
});

test("u32 reads a little-endian 32-bit value", () => {
  assert.equal(byteReader(bytes(1, 0, 0, 0)).u32(), 1);
  assert.equal(byteReader(bytes(0x78, 0x56, 0x34, 0x12)).u32(), 305_419_896);
});

test("u64 reads a little-endian 64-bit value as a number", () => {
  assert.equal(byteReader(bytes(42, 0, 0, 0, 0, 0, 0, 0)).u64(), 42);
  assert.equal(byteReader(bytes(255, 255, 255, 255, 255, 255, 255, 255)).u64(), 18_446_744_073_709_552_000);
});

test("reads compose in sequence across mixed widths", () => {
  const reader = byteReader(bytes(1, 2, 0, 3, 0, 0, 0));
  assert.equal(reader.u8(), 1);
  assert.equal(reader.u16(), 2);
  assert.equal(reader.u32(), 3);
});

test("byteSlice reads a u32-length-prefixed slice and advances past it", () => {
  const reader = byteReader(bytes(3, 0, 0, 0, 10, 20, 30, 99));
  assert.deepEqual(reader.byteSlice(), bytes(10, 20, 30));
  assert.equal(reader.u8(), 99);
});

test("byteSlice reads an empty slice when the length prefix is zero", () => {
  const reader = byteReader(bytes(0, 0, 0, 0));
  assert.deepEqual(reader.byteSlice(), bytes());
});

test("byteSlice honours the reader's byteOffset into a larger buffer", () => {
  const backing = bytes(0xaa, 1, 0, 0, 0, 55);
  const reader = byteReader(backing.subarray(1));
  assert.deepEqual(reader.byteSlice(), bytes(55));
});

test("a read past the end throws a ProtocolError", () => {
  assert.throws(() => byteReader(bytes()).u8(), ProtocolError);
  assert.throws(() => byteReader(bytes(1)).u16(), ProtocolError);
  assert.throws(() => byteReader(bytes(1, 0, 0)).u32(), ProtocolError);
  assert.throws(() => byteReader(bytes(1, 0, 0, 0, 0, 0, 0)).u64(), ProtocolError);
});

test("byteSlice throws when the declared length exceeds the remaining bytes", () => {
  assert.throws(() => byteReader(bytes(4, 0, 0, 0, 1, 2)).byteSlice(), ProtocolError);
});

test("byteSlice throws when even its length prefix is truncated", () => {
  assert.throws(() => byteReader(bytes(0, 0)).byteSlice(), ProtocolError);
});
