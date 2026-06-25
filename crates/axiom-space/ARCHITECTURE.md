# `axiom-space` — architecture

The deterministic **content-addressing** layer: the stable name for *what/where*
content is generated.

## What it is

- **`Address`** — a hierarchical `u64` key-path (root = empty path; `child`
  appends a segment). The stable identity of a chunk / region / content node.
- **`SpaceApi`** — the facade: `root`/`child`/`parent` navigation, a
  length-prefixed canonical byte form (`to_bytes`/`from_bytes`), and a stable
  `digest` (the kernel `StableHash` over those bytes).

It generalizes the app-local `ChunkCoord`/`RegionId`/`PlateId` that
`apps/axiom-growth` built ad hoc into one engine primitive every generator shares.

## Why a layer (not a module)

The moment two domain generators (terrain, biome, structures) need to name the
same site, an engine **module cannot supply** the address — modules may not
depend on one another. The shared addressing primitive must therefore be a layer.

## Why it depends only on `kernel`

An address genuinely needs exactly two kernel capabilities: a **stable digest**
over canonical bytes (`StableHash`) and **canonical serialization**
(`BinaryWriter`/`BinaryReader`, plus `KernelResult` for a clean read error). It
needs nothing from `runtime`, `math`, `host`, … — declaring any of them would be
a ceremonial dependency the `engine_genuine_dependency` dylint bans. So
`depends_on = ["kernel"]`, making it root-adjacent (a peer of `crypto`/`runtime`).

## What does **not** belong here

- **Geometry / coordinates with meaning.** A segment is an opaque key, not a
  world position; world transforms are `math`'s. Signed or multi-axis coordinate
  spaces are encoded *into* segments by the caller.
- **Generation.** An address names a site; it never generates one. Entropy
  streams (`entropy`) and recipes (`proc`) are higher layers that key *by* an
  address.
- **Domain meaning.** "Chunk", "biome", "river" are domain-module concepts; here
  they are just paths.
- Browser/platform APIs, randomness, wall-clock time.

## How later layers use it

`entropy` (Phase 4) keys a deterministic stream by `(seed, Address, version)`;
`proc` (Phase 5) evaluates a recipe *at* an address and stamps the artifact's
provenance with `SpaceApi::digest`. Because the digest is stable across runs and
platforms, the same address always selects the same entropy and labels the same
artifact.
