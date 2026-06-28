# SPEC-01 — Deterministic randomness

> Status: Draft
> Contract: §3, §17   Vocabulary: Seeded PRNG (have), Fisher-Yates shuffle (missing), Weighted pick (partial)   Determinism: sim

## 1. Summary

A game's randomness is the sharpest test of determinism: loot, spawns, crits,
shuffles, and procedural layouts must replay byte-identically and reconcile
across machines (§17.2, §17.6). The contract gives the author **one** seeded
`Rng` (§3) with eight verbs and named, independent sub-streams. The native core
already has the hard part — a seeded splitmix64 keyed by site — but exposes none
of the contract shape, and no TS `Rng` exists at all.

All 11 games draw randomness; a card game's `shuffle`, a roguelike's `weighted`
drop table, and a shooter's spread cone are the same primitive seen three ways.
None of them may reach for `Math.random`.

## 2. Current state (verified)

- **Seeded PRNG: have.** `axiom_kernel::DeterministicRng` is splitmix64 — seeded,
  branchless, snapshot/restore via `state()`/`from_state()` and `Reflect`. It
  offers `next_u64`, `next_bounded` (Lemire, no division), `next_bool_in_thousand`.
- **Keyed streams: have.** `axiom-entropy` wraps it: `EntropyApi::stream(seed,
  &Address, version)` folds `(seed, SpaceApi::digest(address), version)` through
  `StableHash` into a derived key and seeds an `EntropyStream`; same tuple ⇒ same
  sequence, distinct sites never share state. `EntropyStream::fork(salt: u64)`
  derives an isolated sub-stream from the *stable key* (not the draw position),
  so a fork is reproducible however far the parent has advanced.
- **Missing as contract shapes.** `EntropyStream` has `next_u64`/`next_bounded`/
  `fork` only. Absent: `next()` unit float, `int(maxExclusive)`, `range(min,max)`,
  `bool(p)`, `pick`, `weighted` (Fisher-Yates `shuffle` and weighted-pick are the
  Vocabulary's "missing/partial" entries), and a **string-named** `stream(name)`
  (only the `u64`-salted `fork` exists).
- **TS `Rng`: missing entirely.** `packages/axiom-client` is a netcode client; no
  `@axiom/*` authoring projection exists (per SPEC-00).

## 3. Architectural placement

**Extend the `axiom-entropy` layer; do not add a layer.** Every new verb is a
function of one already-seeded `EntropyStream` — a unit draw is `next_u64`
narrowed to a `Ratio`, `int` is `next_bounded`, `shuffle`/`weighted`/`pick` are
index arithmetic over a draw, and `stream(name)` is `fork(StableHash::of_bytes(
name))`. This is the **lowest correct layer**: it owns the seeded stream and the
`StableHash` keying these verbs reuse, so adding them here keeps "all randomness
is keyed the same way" true by construction. Inventing a `random` layer above
`entropy` would be a ceremonial layer that only re-wraps the stream it sits on
(Layer Law) — and pushing the verbs up into the runtime app would scatter the
single randomness source the contract forbids splitting (§17.2). No new
`depends_on`: the work uses `kernel` (`Ratio`, `StableHash`) and `space`
(`Address`) already declared. `sim`-class, branchless, 100% covered.

The TS `Rng` is **projected, not re-implemented**: the wasm boundary app
(`apps/axiom-game-runtime`, SPEC-00) marshals an `EntropyStream` handle, and
`@axiom/game` exposes the `Rng` interface over it. No randomness algorithm lives
in TS — JS arithmetic is not bit-reproducible and would break §17.6.

## 4. API surface

### 4.1 Native (`axiom-entropy`, extends `EntropyStream`)

Naked floats are banned from public engine APIs, so the unit draw is a kernel
`Ratio` in `[0,1)`; `range`/`bool` compose from it and `int` from `next_bounded`.

```rust
impl EntropyStream {
    pub fn unit(&mut self) -> Ratio;               // uniform [0,1): next_u64 >> 11 over 2^53
    pub fn int(&mut self, max_exclusive: u64) -> u64;        // = next_bounded
    pub fn ratio_bool(&mut self, p: Ratio) -> bool;          // true with probability p
    pub fn pick_index(&mut self, len: usize) -> usize;       // uniform [0,len); precondition len>0
    pub fn weighted_index(&mut self, weights: &[u64]) -> usize;  // cumulative-weight selection
    pub fn shuffle<T>(&mut self, items: &mut [T]);  // in-place Fisher-Yates, deterministic
    pub fn named(&self, name: &str) -> EntropyStream;        // fork(StableHash::of_bytes(name))
}
```

`range`/`pick`/`weighted` over typed values are thin index→value composition done
at the projection (4.2); the native core hands back **indices and unit ratios**,
keeping the spine free of generic gameplay types. `weighted_index` takes integer
weights (the author's TS `number[]` is quantized to fixed-point at the boundary
once, never re-floated in sim) so selection is exact and cross-machine identical.

### 4.2 TS authoring projection (the contract, §3)

```ts
interface Rng {
  next(): number;                       // uniform float [0,1)
  int(maxExclusive: number): number;    // uniform integer [0, maxExclusive)
  range(min: number, max: number): number;        // = min + next()*(max-min)
  bool(p?: number): boolean;            // true with probability p (default 0.5)
  pick<T>(items: readonly T[]): T;      // items[pick_index(items.length)]
  weighted<T>(items: readonly T[], weights: readonly number[]): T;
  shuffle<T>(array: T[]): void;         // in-place Fisher-Yates, deterministic
  stream(name: string): Rng;            // named, independent, reproducible sub-stream
}
```

`Sim.rng` (SPEC-00 §2) is the game's root stream, minted by the runtime app from
`GameConfig.seed`. `pick`/`weighted`/`shuffle` reorder the author's own array
client-side using indices the native core chose — the draw sequence, not the JS
array op, is what determinism rides on.

## 5. Data contracts

- **`Ratio`** (kernel) — the unit-interval draw; the only floating value crossing
  the boundary, and never re-entered into a draw.
- **`EntropyStream` handle** — opaque; the runtime app holds the table mapping a
  JS `Rng` to its native stream. Never serialized into sim state (a replay
  re-mints from seed; the live `state()` snapshots via `Reflect`, §16.5).
- **Weights** cross as a fixed-point `&[u64]`; the quantization rule (a single
  documented scale) lives at the boundary, not per call site.

## 6. Determinism

- **Single randomness source (§17.2).** This is the *only* sanctioned randomness
  in sim code. `stream(name)` is the contract's escape valve for "I want an
  independent sequence" — it forks the keyed stream rather than seeding a second
  generator, so every sub-stream is still a pure function of the root seed. There
  is no second PRNG, no OS entropy, no `Math.random` reachable from `onFixedUpdate`.
- **Named streams are reproducible and independent.** `named(name)` hashes the
  name into the existing key derivation (`StableHash::of_bytes` → `fork`), exactly
  as `EntropyApi::stream` folds an `Address` and `version`. Same name ⇒ same
  sub-stream on every run and platform; distinct names ⇒ non-overlapping streams;
  and (like `fork`) the sub-stream is stable regardless of how far the parent has
  drawn.
- **Cross-instance (§17.6).** splitmix64, Lemire reduction, and integer cumulative
  weights are all exact integer arithmetic — bit-identical across machines, so
  authority and prediction never diverge on a draw.
- `unit()`'s `Ratio` is presentation-safe but is itself a sim value; no
  presentation-clock value ever feeds a draw (§17.5).

## 7. Acceptance / proof

- **100% covered, branchless** across the new surface — the binding gate for a
  sim-class layer.
- **Shuffle is branchless Fisher-Yates.** Expressed as an index fold, not a loop:
  `(1..len).rev().for_each(|i| items.swap(i, self.int(i as u64 + 1) as usize))`
  — iterator adapter + arithmetic index, zero `if`/`for`/`match` (Branchless Law).
  Property test: a shuffle is a permutation (multiset-preserving), and a fixed
  seed yields a fixed, golden ordering.
- **Weighted-pick is branchless.** Cumulative selection without control flow:
  fold weights to a prefix-sum and take the first index whose cumulative exceeds
  the draw via iterator `position`/`scan` over a boolean comparison — never a
  hand-rolled `for`+`break`. Tests: zero-weight entries are never chosen; the
  empirical histogram over many draws tracks the weights; a fixed seed is golden.
- **Replay/golden (sim).** A pinned first-value golden (mirroring
  `golden_first_value_is_stable`) for `unit`, `int`, `weighted_index`, and a
  shuffled ordering. A named-stream test: `named("loot")` vs `named("spawn")`
  diverge; `named("loot")` reproduces across parent draw positions.
- **Projection.** `@axiom/game` `Rng`: tsgo + Oxlint (branch ban) + 100% TS
  coverage; a headless game draws a sequence and asserts it reproduces on a second
  run with the same seed (the §17.4 per-tick contract, scoped to randomness).

## 8. Dependencies & order

Lands **second**, right after SPEC-00 (the boundary that carries `Sim.rng`). It
needs only the existing `EntropyStream` plus SPEC-00's handle table; nothing in
01 blocks on 02+. Everything sim-class downstream that draws — spawns (SPEC-02),
procedural grids (SPEC-06), timers/jitter (SPEC-07), netcode prediction
(SPEC-13) — consumes this `Rng` and must route through it, never a private
generator.

## 9. Open questions

- **Weight quantization scale.** What fixed-point scale does the boundary use to
  turn TS `number[]` weights into `&[u64]`? A single documented constant (e.g.
  `1e6`) keeps it exact and reproducible; the alternative — passing floats and
  summing them in sim — reintroduces cross-machine float drift and is rejected.
- **`bool(p)` precision.** `p` arrives as a TS `number`; it must be quantized to a
  `Ratio` by the same boundary rule as weights, so `bool(0.3)` is identical
  everywhere. Confirm one quantization policy serves both.
- **Root-stream identity.** Is `Sim.rng` keyed purely by `seed`, or by `(seed,
  Address::root, version=0)` through `EntropyApi::stream` for uniformity with the
  procgen keying? Lean on the latter so there is exactly one keying path.
- **`stream(name)` collision domain.** `StableHash` over UTF-8 names is
  collision-free in practice; if an author ever needs a guaranteed-disjoint
  namespace, that is a boundary-side allocation concern, not a kernel change.
