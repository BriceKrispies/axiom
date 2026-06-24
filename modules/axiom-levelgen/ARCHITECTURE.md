# `axiom-levelgen` — architecture

The **composition tier** of the procedural-generation pivot (roadmap Phase 9): a
feature module that folds the three domain generators into one world.

## What it is

- **`LevelGenApi::generate(seed, &Address, width, height) -> World`** — composes:
  1. a terrain **elevation** heightfield,
  2. an independent terrain **moisture** heightfield (a salted seed),
  3. a **biome** map classified from the elevation/moisture pair, and
  4. a **placement** of scattered objects,
  all keyed deterministically by `(seed, address)` (the address digest folds into
  a world seed so distinct sites are distinct worlds).
- **`World`** — the composed result: row-major terrain heights + biome codes, plus
  object positions, with canonical bytes + a stable digest. Read through its
  methods (not a second `lib.rs` export — Module Law #8).

## Why a feature module (not an engine module)

An engine module's `allowed_modules` must be empty — it may never depend on another
module. Composing `terrain` + `biome` + `placement` therefore cannot live in an
engine module. A **feature module** (`kind = "feature-module"`) is the sanctioned
exception: it may depend on exactly the modules it lists. This is the same tier as
`axiom-render-pipeline` (which composes scene + resources + render + webgpu). Only
apps — or another feature module — may depend on `levelgen`.

## It translates contracts, it does not leak them

Each domain module exposes one facade and keeps its result type behind it, so
`levelgen` cannot **name** `axiom_terrain::HeightField`, `axiom_biome::BiomeMap`,
or `axiom_placement::Placement`. It calls each facade, reads the values through
their public methods (`heights()`, `classify()`, `positions()`), and stores the
read-outs in its own neutral `World`. That is exactly the "apps and feature modules
translate between module contracts" rule from the vertical-slice design: each
domain module stays a black box with a stable shape.

## Why it depends on kernel + space (+ the three modules)

- **terrain / biome / placement** — the domain generators it composes
  (`allowed_modules`).
- **space** — the content `Address` it keys by (and `SpaceApi::digest` folded into
  the world seed).
- **kernel** — `BinaryWriter` + `StableHash` for the world's canonical bytes/digest.
- It does **not** declare `entropy`/`proc`: those are used transitively by the
  domain modules, never named here.

## The invariants it guarantees

- **Deterministic:** identical `(seed, address, size)` yield byte-identical worlds.
- **Genuinely composed:** the biome map varies with the terrain it is classified
  from (a flat single-biome world would fail the test), and objects stay in bounds.
- **Distinct sites, distinct worlds:** the address digest keys the whole world.
