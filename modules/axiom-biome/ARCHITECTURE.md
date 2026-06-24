# `axiom-biome` — architecture

A **domain generator** on the procedural-generation substrate (roadmap Phase 9):
biome classification.

## What it is

- **`BiomeApi::classify(elevation, moisture) -> u8`** — a branchless,
  Whittaker-style band lookup: elevation buckets into low/mid/high, moisture into
  dry/wet, and their product indexes a 6-entry biome table.
- **`BiomeApi::map(seed, &Address, count) -> BiomeMap`** — generates a field by
  drawing per-cell elevation then moisture from an entropy stream keyed by the
  address, classifying each.
- **Biome codes** — `OCEAN`/`BEACH`/`DESERT`/`FOREST`/`MOUNTAIN`/`PEAK`, small
  `u8` constants on the facade (so there is no second `lib.rs` export — Module
  Law #8 stays satisfied without a category type).
- **`BiomeMap`** — the result: a `Vec<u8>` of codes with canonical bytes + a
  stable digest, read through its methods.

## Where the domain rules live

The thresholds that decide *what makes a biome a biome* live **here**, not in the
generic `proc-validate` layer (which only knows neutral words). That is the Phase 9
split: generic validation/scoring/constraints are a **layer**; domain semantics
("forest is mid-elevation and wet") are a **domain module**.

## Why it depends on kernel + space + entropy — and NOT terrain

- **space** — the `Address` a biome map is keyed at.
- **entropy** — the keyed stream per-cell elevation/moisture is drawn from.
- **kernel** — `BinaryWriter` + `StableHash` for the map's canonical bytes/digest.
- **not `terrain`** — modules may not depend on modules. A real world that pairs a
  terrain heightfield with a biome map is composed by an **app or feature module**,
  which reads terrain's heights and feeds them to `BiomeApi::classify`. Biome never
  imports terrain.

## What does **not** belong here

- **No naked floats / no geometry.** Elevation, moisture, and codes are integers.
- **No generic validation.** Scoring/constraints/repair are `proc-validate`'s; this
  module owns only the *domain* thresholds.
- Browser/platform APIs, randomness (it routes `entropy`), wall-clock time.

## The invariants it guarantees

- **Deterministic:** identical `(seed, address, count)` yield byte-identical maps;
  identical `(elevation, moisture)` always classify the same.
- **ε-stable:** a small change to elevation/moisture keeps the biome *except* near
  a band threshold, where it flips — a metamorphic test pins this.
- **Bounded vocabulary:** every code is one of the six named biomes.
- **Versioned:** the generator carries a version; bumping it re-keys (+ regoldens).
