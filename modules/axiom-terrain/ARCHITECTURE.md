# `axiom-terrain` — architecture

A **domain generator** on the procedural-generation substrate (roadmap Phase 9):
coherent value-noise heightfields. This is where noise graduates from
`apps/axiom-growth` into a reusable, spine-tested engine module.

## What it is

- **`TerrainApi::heightfield(seed, origin_x, origin_y, width, height)`** — a grid
  of integer heights. Each cell's height is the bilinear interpolation of the four
  surrounding **lattice values**; a lattice value is one draw from an entropy
  stream keyed by `(seed, lattice-Address, version)`.
- **`HeightField`** — the result: `width × height` row-major `i32` heights, with
  `at(cx, cy)`, canonical bytes, and a stable `StableHash` digest. Returned by the
  facade and read through its methods (not a second `lib.rs` export — Module
  Law #8).

## The seam-coherence invariant

Lattice values are a pure function of **world** lattice coordinates, so every
heightfield covering a given world point computes the same value there. Two tiles
that overlap in world space therefore agree on the overlap — adjacent tiles share
a **seamless** edge. `axiom-growth` had to enforce this by hand
(`shared_edge_seam_is_zero`); here it is an engine guarantee, proven by
`adjacent_tiles_share_a_seamless_edge`.

## Why it depends on kernel + space + entropy (and not proc, not math)

- **space** — the lattice `Address` each noise value is keyed by.
- **entropy** — the keyed stream a lattice value is drawn from.
- **kernel** — `BinaryWriter` + `StableHash` for a heightfield's canonical bytes
  and digest.
- **not `proc`** — a heightfield is a *spatial field*, not a sequential recipe;
  declaring `proc` would be a ceremonial dependency the genuine-dependency dylint
  bans.
- **not `math`** — heights are unitless integers; geometry (turning a heightfield
  into a mesh) is a caller's concern, so no `f32`/geometry is referenced.

## What does **not** belong here

- **No naked floats.** The noise is computed in fixed-point integers end to end;
  heights are `i32`.
- **No meshing / no geometry.** A heightfield is neutral data. Building a mesh,
  normals, or LOD from it is a render/meshgen concern.
- **No domain semantics.** "Mountain", "ocean", "biome" are a *biome module*'s job
  consuming this heightfield — terrain only produces relief.
- Browser/platform APIs, randomness (it routes `entropy` → the kernel RNG),
  wall-clock time.

## The invariants it guarantees

- **Deterministic + reproducible:** identical `(seed, origin, size)` yield
  byte-identical heightfields.
- **Seamless:** overlapping tiles agree on shared world cells (zero seam).
- **Coherent:** a horizontal step never jumps the full range — it is interpolated
  noise, not white noise.
- **Versioned:** the noise carries a version; bumping it re-keys (and regoldens)
  deliberately.
