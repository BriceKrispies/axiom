import assert from "node:assert/strict";
import { test } from "node:test";

import { byteWriter, concatBytes } from "./byte-writer.ts";

const bytes = (...values: number[]): Uint8Array => Uint8Array.from(values);

test("concatBytes flattens chunks in order", () => {
  assert.deepEqual(concatBytes([bytes(1, 2), bytes(3), bytes(4, 5)]), bytes(1, 2, 3, 4, 5));
});

test("concatBytes of no chunks is an empty array", () => {
  assert.deepEqual(concatBytes([]), bytes());
});

test("a fresh writer finishes to an empty array", () => {
  assert.deepEqual(byteWriter().finish(), bytes());
});

test("u8 writes a single byte", () => {
  const writer = byteWriter();
  writer.u8(42);
  assert.deepEqual(writer.finish(), bytes(42));
});

test("u16 writes a little-endian 16-bit value", () => {
  const writer = byteWriter();
  writer.u16(4660);
  assert.deepEqual(writer.finish(), bytes(0x34, 0x12));
});

test("u32 writes a little-endian 32-bit value", () => {
  const writer = byteWriter();
  writer.u32(305_419_896);
  assert.deepEqual(writer.finish(), bytes(0x78, 0x56, 0x34, 0x12));
});

test("u64 writes a little-endian 64-bit value", () => {
  const writer = byteWriter();
  writer.u64(42);
  assert.deepEqual(writer.finish(), bytes(42, 0, 0, 0, 0, 0, 0, 0));
});

test("byteSlice writes a u32 length prefix followed by the data", () => {
  const writer = byteWriter();
  writer.byteSlice(bytes(10, 20, 30));
  assert.deepEqual(writer.finish(), bytes(3, 0, 0, 0, 10, 20, 30));
});

test("byteSlice of empty data writes a zero length prefix", () => {
  const writer = byteWriter();
  writer.byteSlice(bytes());
  assert.deepEqual(writer.finish(), bytes(0, 0, 0, 0));
});

test("mixed writes accumulate in call order", () => {
  const writer = byteWriter();
  writer.u8(1);
  writer.u16(2);
  writer.u32(3);
  writer.byteSlice(bytes(9));
  assert.deepEqual(writer.finish(), bytes(1, 2, 0, 3, 0, 0, 0, 1, 0, 0, 0, 9));
});
