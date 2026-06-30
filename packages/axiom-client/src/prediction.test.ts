import assert from "node:assert/strict";
import { test } from "node:test";

import { resimulate } from "./prediction.ts";

const u8 = (...bytes: number[]): Uint8Array => Uint8Array.from(bytes);

// A deterministic step that folds each intent's first payload byte into a base-10
// accumulator, so the result encodes BOTH the values and their order (the twin of
// the Rust `fold_step` resimulation test helper).
const foldStep = (state: number, payload: Uint8Array): number => state * 10 + payload[0]!;

// A generic, non-numeric step: appends each intent's first byte to an immutable list.
const append = (state: readonly number[], payload: Uint8Array): readonly number[] => [...state, payload[0]!];

test("resimulate is the identity over an empty unacked list", () => {
  // The disabled-prediction path: the client passes no unacked intents, so the
  // just-snapped baseline is returned verbatim (`each` maps zero elements).
  assert.equal(resimulate(100, [], foldStep), 100);
});

test("resimulate folds every unacked intent in send order", () => {
  // 1,2,3 in order -> 0*10+1=1 -> 1*10+2=12 -> 12*10+3=123: order is observable.
  assert.equal(resimulate(0, [u8(1), u8(2), u8(3)], foldStep), 123);
});

test("resimulate threads a generic, non-numeric state through the step", () => {
  // State is game-generic: here an immutable list, proving resimulate never assumes
  // a numeric or mutable state shape.
  assert.deepEqual(resimulate<readonly number[]>([], [u8(7), u8(9)], append), [7, 9]);
});
