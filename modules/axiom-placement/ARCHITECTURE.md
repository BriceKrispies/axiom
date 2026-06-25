# `axiom-placement` — architecture

The first **domain generator** on the procedural-generation substrate (roadmap
Phase 9): deterministic object placement.

## What it is

- **`PlacementApi::scatter(seed, &Address, count, width, height)`** — evaluates a
  draw-only `proc` recipe of `count` nodes at the address and reduces each artifact
  word into a `(x, y)` grid cell. The same inputs always yield the same scatter.
- **`Placement`** — the result: a list of integer positions, with canonical bytes
  + a stable `StableHash` digest. Returned by the facade and read through its
  methods (it is not a second `lib.rs` export — Module Law #8).

## Why an engine module (not a layer, not app-local)

Placement is a *domain capability*, not shared spine: it builds **on** the
`space`/`proc` layers rather than under them, and many apps/feature modules will
want it. So it is an **engine module** (`allowed_modules = []`) — isolated, never
importing another module. A second domain generator that needs to share a
primitive with this one would push that primitive *down* into a layer, never a
module→module edge.

## Why it depends on kernel + space + proc

- **space** — the content `Address` it scatters at.
- **proc** — the draw recipe it evaluates (which internally keys `entropy`).
- **kernel** — `BinaryWriter` + `StableHash` for a placement's canonical bytes and
  digest.

## What does **not** belong here

- **No naked floats / no geometry.** Positions are integer grid cells; world
  transforms are `math`'s, and a unit-bearing scalar would be the kernel's.
- **No "what is placed".** A placement names *cells*, not trees or rocks — the
  payload is a richer module's or an app's concern. This keeps placement a small,
  reusable primitive.
- **No noise yet.** Scatter is uniform over the grid; weighted/noise-driven
  placement (and terrain's heightfield noise) graduate from `axiom-growth` into
  their own modules — placement stays the clean, integer baseline.
- Browser/platform APIs, randomness (it routes `proc` → `entropy` → the kernel
  RNG), wall-clock time.

## The invariants it guarantees

- **Deterministic + reproducible:** identical `(seed, address, count, bounds)`
  yield byte-identical placements.
- **In-bounds + panic-free:** every cell is within `width × height`; degenerate
  `0` bounds collapse to the origin rather than panic.
- **Versioned:** the scatter recipe carries a version; bumping it re-keys
  generation (and regoldens) deliberately.
