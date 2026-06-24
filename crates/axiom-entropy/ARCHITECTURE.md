# `axiom-entropy` — architecture

The deterministic **entropy** layer: it expands one seed into an independent,
reproducible stream per generated site.

## What it is

- **`EntropyApi::stream(seed, &Address, version)`** — derives a stream's stable
  key by folding `(seed, SpaceApi::digest(address), version)` with the kernel
  `StableHash`, then seeds a kernel `DeterministicRng` with it.
- **`EntropyStream`** — draws values (`next_u64`/`next_bounded`) and `fork(salt)`s
  isolated sub-streams off its **stable key** (not its draw position), so a fork
  is reproducible regardless of how far the parent has advanced. Generalizes the
  app-local `Rng::fork(salt)` from `apps/axiom-growth`.

## Why it depends on `kernel` + `space`

- **kernel** — it *routes* the existing `DeterministicRng` (it adds no new RNG)
  and *keys* with `StableHash`.
- **space** — the `Address` is what names the site whose stream this is; the
  digest of that address is the entropy key's spatial component.

Both are genuinely used (the `engine_genuine_dependency` dylint confirms it).

## What does **not** belong here

- **No new randomness.** No second RNG algorithm, no OS entropy, no wall clock.
  The kernel owns the deterministic source; this layer only keys and routes it.
- **No noise functions.** Value/Perlin/simplex noise is *domain* generation; it
  graduates from `axiom-growth` into a Phase 9 domain module, not here.
- **No generation, no geometry, no browser/platform APIs.**

## The invariants it guarantees

- **Reproducible:** the same `(seed, address, version)` always yields the same
  sequence, on every run and platform (it inherits the kernel RNG's cross-platform
  stability and the digest's canonical-byte stability).
- **Independent:** distinct sites or versions yield non-overlapping streams
  (collision-free keying over a swept domain is a test invariant).
- **Versioned:** bumping `version` re-keys the stream; restoring it restores the
  stream — versioning is a first-class input, never silent.
