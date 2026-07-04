# Project notes — Generated Micro-FPS

Design rationale, the recipe-graph shapes, and the exact boundary between what
the procedural pipeline expresses and what is composition on top of it.

## What the pipeline expresses, and what it does not

The existing Axiom procedural pipeline generates **one neutral buffer per
recipe**: a texture recipe bakes to an RGBA8 `TextureBuffer`, a mesh recipe bakes
to a `MeshBuffer`. It has 8 texture operators (Solid, Gradient, Noise, Bricks,
Blur, Blend, ColorRamp, HeightToNormal) and 10 mesh operators (Cube, Cylinder,
Grid, Transform, Extrude, Bevel, Bend, Displace, UVProject, Triangulate).

So **only the art (textures + meshes) is authored as recipes** — those are what
`pack` serializes and what the determinism hash covers. Everything above the art
is *composition*, because the pipeline has no operator for it:

- **Materials** bind a baked texture to a palette color — an engine `Material`,
  not an operator graph.
- **Prefabs** bundle a mesh + material + gameplay tag.
- **The scene grammar** places prefab instances using the deterministic
  `axiom-entropy` stream — the same determinism substrate the operators use, so
  the layout is as reproducible as the art.
- **Gameplay** is a deterministic state machine over the generated layout.

This split is the honest shape of the current engine, and it is recorded here so
a future agent does not mistake the composition tiers for missing operators.

## Hierarchy — no per-object hand authoring

The project is strictly hierarchical:

```
Style (seed + palette + knobs)
  └─ texture macros ─┐
  └─ mesh macros ────┼─ materials ─ prefabs ─┐
                     │                        ├─ grammar (room shell / corridor /
                     │                        │   scatter, seeded) ─ scenes
                     └────────────────────────┘
                                              └─ gameplay ruleset
```

- A **room shell** is `floor_tiles` (a loop over the footprint) + `perimeter_walls`
  (a loop that leaves the center panel open on a doorway side) + `ceiling_lights`.
- A **corridor** is a loop of floor + side-wall panels, capped by a door or gate.
- **Crates, pipes, and enemies** are *scattered* by `EntropyStream::unit()` draws
  keyed by the level seed at distinct addresses — change the seed and the combat
  room re-arranges, but always the same way for a given seed.

Re-skinning is one edit: change `Style::facility()` and every surface, size, and
placement follows.

## Recipe-graph shapes (representative)

- **Wall texture:** `Bricks(panel grid)` and `Noise(grime)` → `Blend` — a grimy
  bolt-panel. Grime amount is `style.grime`.
- **Floor texture:** `Noise` → `ColorRamp(floor..wear)`.
- **Door / gate texture:** `Bricks(rows=5, cols=1)` — horizontal hazard slats in
  the door's base color (amber door, red locked gate, green open gate).
- **Enemy texture:** `Noise` → `ColorRamp(dark..variant)` — a high-contrast body.
- **Wall / floor / door mesh:** `Cube` → `Transform(non-uniform scale)` →
  `UVProject` — an axis-aligned slab sized in world units.
- **Crate / weapon mesh:** `Cube` → `Bevel` → `Transform` → `UVProject` →
  `Triangulate` — low-poly but not blocky.
- **Enemy A ("grunt"):** `Cube` → `Bevel` → `Displace` → `Transform` →
  `Triangulate` — an irregular boxy silhouette.
- **Enemy B ("sentry"):** `Cylinder` → `Bevel` → `UVProject` — a clean, distinct
  cylindrical read (high-contrast enemy language: two very different silhouettes).
- **Pipe:** `Cylinder` → `UVProject`.

Recipe ids are banded: textures `100..`, meshes `200..`, so materials/prefabs
reference a durable number and validation proves every reference resolves.

## Determinism

`pack` serializes every recipe (each already `SchemaVersion`-stamped) into one
seed-stamped blob and hashes it with the kernel `StableHash`. The same seed always
yields the same bytes and hash (`0x9ea0bee4ad030c82` for the shipped seed). The
scene grammar's placement RNG is the seed-keyed `axiom-entropy` stream, so the
layout is deterministic too. Changing `Style::level_seed` changes both.

## Relationship to the `axiom-proc-player` room demo

This project supersedes the minimal `apps/axiom-proc-player` room demo as the
showcase of the pipeline: the room proved *a texture + a mesh + a material become
runtime resources*; this proves *a hierarchical recipe project becomes a small
playable game*. The room demo remains as the pipeline's smallest integration
smoke test.
