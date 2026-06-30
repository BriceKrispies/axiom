/*
 * The session-seed boundary codec (SPEC-12). The 64-bit deterministic seed stays
 * a `bigint` in the author-facing API everywhere — `GameConfig.seed`,
 * `SessionConfig.seed`, `RoomConfig.seed` — but it must NOT cross the wasm
 * boundary as a single i64: the Binaryen `wasm2js` fallback legalizes i64 into
 * split i32 pairs and cannot use wasm-bindgen's BigInt i64 ABI ("Cannot mix
 * BigInt and other types"). So the wasm `WasmGame` exposes the seed as two u32
 * `number` halves (`seed_lo` + `seed_hi`); this recombines them into the full
 * `bigint` the host channel hands back.
 *
 * The recombine is exact integer arithmetic over the whole 2^64 seed space, so a
 * round-tripped seed is byte-identical to a native i64 seed — determinism is
 * preserved. Arithmetic (`+`/`*2^32`, not `|`/`<<`) keeps it within the
 * `no-bitwise` restriction, exactly as the rgba codec packs colours by positional
 * scale rather than bit-shifts.
 */

/** 2^32 — the positional scale of the high half in the recombined 64-bit seed. */
const HIGH_HALF_SCALE = 4_294_967_296n;

/**
 * Recombine the low + high u32 halves (the wasm `seed_lo` / `seed_hi` getters)
 * into the full 64-bit session seed `bigint`.
 */
export const seedFromHalves = (low: number, high: number): bigint =>
  BigInt(low) + BigInt(high) * HIGH_HALF_SCALE;
