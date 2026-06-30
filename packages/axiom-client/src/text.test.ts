import assert from "node:assert/strict";
import { test } from "node:test";

import { toBytes } from "./text.ts";

const bytes = (...values: number[]): Uint8Array => Uint8Array.from(values);

test("toBytes UTF-8 encodes a string", () => {
  assert.deepEqual(toBytes("abc"), bytes(97, 98, 99));
});

test("toBytes encodes multi-byte UTF-8 characters", () => {
  assert.deepEqual(toBytes("é"), bytes(0xc3, 0xa9));
});

test("toBytes of an empty string is an empty array", () => {
  assert.deepEqual(toBytes(""), bytes());
});

test("toBytes passes a Uint8Array through unchanged", () => {
  assert.deepEqual(toBytes(bytes(1, 2, 3)), bytes(1, 2, 3));
});

test("toBytes of an empty Uint8Array is an empty array", () => {
  assert.deepEqual(toBytes(bytes()), bytes());
});
