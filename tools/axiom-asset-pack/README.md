# axiom-asset-pack

Native Rust CLI that packs an authored asset set into the engine's **Axiom-native
binary manifest** (`manifest.bin`) plus served asset blobs.

It is the **producer** side of the runtime asset-streaming pipeline. The
**consumer** — the deterministic, I/O-free streaming brain — already exists as
`modules/axiom-assets` (`axiom_assets::AssetsApi`). The binary manifest format is
**owned by that module**; this tool encodes through
`AssetsApi::encode_manifest` and verifies its own output round-trips through
`AssetsApi::from_manifest_bytes`, so the producer and consumer can never drift.

This is a **Tool**: it lives under `tools/`, sits outside the engine dependency
graph, and is exempt from the coverage/branchless gates. It may depend on engine
crates (here: `axiom-kernel` + the `axiom-assets` module); the architecture rule
bans engine code depending on a *tool*, never the reverse.

## Input format (TOML)

```toml
out_dir  = "dist"      # output root, relative to THIS TOML file's directory
blob_dir = "blobs"     # optional (default "blobs"): blob subdir + locator prefix

[[asset]]
id           = 1       # stable u64 asset id (must be unique and non-zero)
kind         = 1       # app-defined u32 kind tag (e.g. 1=mesh, 2=texture, 3=material)
priority     = 100     # u32 streaming priority (the scheduler dispatches higher first)
source       = "assets/hero.mesh"  # source file, relative to this TOML file
dependencies = []      # optional u64 ids; each must reference an asset in this set
```

- `out_dir` and every `source` path are resolved **relative to the input TOML
  file's directory**, so a pack run is location-independent.
- `kind` is opaque to the engine — it is whatever the consuming app decides
  (mesh / texture / material / …).
- `dependencies` form the streaming DAG: an asset only becomes eligible to load
  once all of its dependencies are `ready`. A dependency on an id not present in
  the set is rejected at pack time (the manifest would fail `axiom-assets`
  validation).

## What it does, per asset

1. Reads the source file; `size_hint` = file length.
2. Computes `content_hash` = `axiom_kernel::StableHash::of_bytes(file)` (FNV-1a
   64-bit, deterministic across runs/platforms).
3. Copies the blob to `out_dir/blob_dir/<id>.<ext>` (id-based name, source
   extension preserved; extensionless sources use the bare id).
4. Sets the manifest `locator` to the **`out_dir`-relative URL** the browser
   fetches: `"<blob_dir>/<id>.<ext>"` (forward-slashed).

Then it encodes all entries into `out_dir/manifest.bin` via
`AssetsApi::encode_manifest`, **verifies the bytes round-trip** through
`AssetsApi::from_manifest_bytes` (catching duplicate/null ids and dangling
dependencies), and prints a summary.

## Output layout

For an input at `<dir>/pack.toml` with `out_dir = "dist"`, `blob_dir = "blobs"`:

```text
<dir>/dist/manifest.bin       # feed to AssetsApi::from_manifest_bytes(&bytes, max_in_flight)
<dir>/dist/blobs/1.mesh       # locator "blobs/1.mesh"
<dir>/dist/blobs/2.tex        # locator "blobs/2.tex"
<dir>/dist/blobs/3.mat        # locator "blobs/3.mat"
```

A browser consumer loads `dist/` as a static root: it reads `manifest.bin`, hands
the bytes to `AssetsApi::from_manifest_bytes`, and fetches each `locator` (e.g.
`dist/blobs/1.mesh`) as the scheduler asks for it.

### AssetId ↔ locator convention

`locator = "<blob_dir>/<id>.<ext>"` — **id-based**, not content-addressed, so a
demo author can predict an asset's URL from its id alone. `<ext>` is the source
file's extension (or omitted if the source has none).

## Usage

```sh
cargo run -p axiom-asset-pack -- <input.toml>
```

Run the bundled sample (from the repo root):

```sh
cargo run -p axiom-asset-pack -- tools/axiom-asset-pack/sample/pack.toml
```

## Tests

```sh
cargo test -p axiom-asset-pack
```

Covers the author → pack → `from_manifest_bytes` round-trip (ids, locators,
deps, kinds, counts), the content-hash equality with `StableHash`, the default
`blob_dir`/extensionless naming, and the error cases (missing input, missing
source, malformed TOML, duplicate id, dangling dependency).
