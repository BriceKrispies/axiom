# `axiom-worldsave` — architecture

The **save/delta model** that realizes the procedural-generation payoff (roadmap
Phase 12): a world reproduces from a tiny save, not a stored copy.

## What it is

- **`WorldSaveApi::save(seed, &Address, w, h, overrides) -> Save`** — captures only
  the regeneration inputs (seed, generator version, address, dimensions) plus the
  player's **deltas** (`(cell_index, biome_code)` overrides). It never stores the
  generated world.
- **`WorldSaveApi::restore(&Save) -> SavedWorld`** — regenerates the `levelgen`
  world from the seed and replays the deltas on top, byte-for-byte identical to the
  live world the save came from.
- **`Save`** — the compact, serializable save (`to_bytes` + digest); far smaller
  than the world it rebuilds.
- **`SavedWorld`** — the regenerated world (heights + biomes-with-overrides +
  objects), read through its methods.

## Why a feature module

It composes the `levelgen` world recipe — an engine module may never depend on
another module (`allowed_modules = []`), so regenerating a `levelgen` world from a
save can only live in a **feature module** (`kind = "feature-module"`), the same
tier as `levelgen` itself. It depends on `kernel` + `space` and the one module it
lists, `levelgen`; nothing but an app (or another feature module) may depend on it.

## The save/delta payoff (and how multiplayer reuses it)

A naive save stores the whole world. This stores only what cannot be regenerated —
`{seed, version, address, deltas}` — and `Save::to_bytes` is far smaller than the
world `restore` rebuilds (a test pins `save * 4 < world`). Loading replays
generation, then applies the deltas.

The **same shape** is how lockstep multiplayer works: a peer ships
`{seed, versions, command/delta stream}`, never full state. Because the whole
generation stack is **integer-only and deterministic** — proven by the
`axiom-proc-fuzz` gate across 2000 seeds — the regenerated world is byte-identical
on every platform, so a server and a browser agree from the same save. Integrating
the `netcode`/`net-protocol` stacks and a native↔wasm parity harness is the
documented next step; the determinism those rely on is already in place and gated.

## What does **not** belong here

- **No full-world storage.** A save never serializes the generated world — that
  would defeat the entire point.
- **No generation.** Restoring regenerates via `LevelGenApi`; this module only
  bundles inputs and replays deltas. It owns no terrain/biome/placement logic.
- **No domain meaning in the deltas.** A delta overrides a cell to an opaque biome
  `u8` code; what the code *means* is `biome`'s concern (so this module does not
  even depend on `biome`).
- Browser/platform APIs, randomness, wall-clock time.

## The invariants it guarantees

- **Round-trip determinism:** `restore` of a save is byte-identical every time and
  on every platform (integer-only generation).
- **Deltas are surgical:** an override changes only its cell's biome; terrain and
  objects are the regenerated base; an out-of-range override is a safe no-op.
- **Compactness:** a save is far smaller than the world it regenerates.
- **Versioned:** the save records the generator version so a mismatch is detectable.
