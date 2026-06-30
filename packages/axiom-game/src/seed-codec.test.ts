import assert from "node:assert/strict";
import { test } from "node:test";

import { seedFromHalves } from "./seed-codec.ts";

test("seedFromHalves round-trips a full 64-bit seed with high bits set", () => {
  // 0x12345678_9abcdef0 split into its low/high u32 halves (the Rust side's
  // `seed as u32` / `(seed >> 32) as u32`); recombining must reproduce it exactly,
  // proving the lo/hi split preserves the full 2^64 seed space (determinism).
  assert.equal(seedFromHalves(0x9a_bc_de_f0, 0x12_34_56_78), 0x12_34_56_78_9a_bc_de_f0n);
});

test("seedFromHalves covers the seed-space extremes and each half in isolation", () => {
  assert.equal(seedFromHalves(0, 0), 0n);
  // Max u64: both halves all-ones recombine to 2^64 - 1.
  assert.equal(seedFromHalves(0xff_ff_ff_ff, 0xff_ff_ff_ff), 0xff_ff_ff_ff_ff_ff_ff_ffn);
  // Low half only (no high bits) stays the small value.
  assert.equal(seedFromHalves(0x00_00_00_01, 0), 1n);
  // High half only lands exactly at 2^32 (the boundary the old i64 ABI carried).
  assert.equal(seedFromHalves(0, 0x00_00_00_01), 0x01_00_00_00_00n);
});
