# 09 — Burnt Rubber Texture Preparation

## Mission

Move the three procedural albedo bakes (asphalt, verge, foliage — 96 KB total)
out of the material-registration call sites and into the texture preparation
task. They are argument-free deterministic constants, which makes this the
cleanest of the three domain migrations — and the one that most sharply exposes
the generate/register split, because today the generators are **fused into** the
`add_texture_data` calls.

## Architectural owner

- **Package:** `apps/burnt-rubber`
- **Classification:** App
- **Why here:** these are the game's own art. `axiom-runtime` must never learn
  what an albedo is.


## THE ADDITIVE RULE — read before anything else

**You must not change the signature of any existing function.** Add a *prepared
variant* alongside it and leave the original in place, working.

This is not style. `ScenePalette::install` and `road_materials` have **35 call
sites across 6 files**, 25 of them `#[cfg(test)]` fixtures in
`render/car_model.rs`, `render/pickups.rs` and `render/effects.rs` — three files
that **no manifest in this programme owns**. Changing the arity would break the
crate in files nobody is allowed to fix.

```rust
// keep, unchanged, still generating inline
pub fn road_materials(app: &mut RunningApp) -> RoadMaterials;

// add
pub fn road_materials_prepared(app: &mut RunningApp, prepared: &PreparedTextures) -> RoadMaterials;
```

Consequences, all good: the crate compiles at every commit, you can run
`cargo test -p axiom-burnt-rubber` **and the golden run** yourself, `11` becomes
a pure call-site switch, and `13` deletes the now-dead inline paths as its
documented "remove dead compatibility paths" step.

Note also that `cargo test --lib` **builds the lib target first** — so if the
crate did not compile, *zero* of your tests would run and your own completion
criteria would be unverifiable.

## Depends on

**`07-burnt-rubber-preparation-scaffold.md`**.

## Parallel safety

**Fully concurrent with `08` and `10`.**

## Files owned

| Path | Action |
|---|---|
| `apps/burnt-rubber/src/preparation/textures.rs` | modify (stub → real) |
| `apps/burnt-rubber/src/render/palette.rs` | modify (1611 lines) |
| `apps/burnt-rubber/src/render/asphalt_texture.rs` | **only if needed** (already `pub`) |
| `apps/burnt-rubber/src/render/verge_texture.rs` | **only if needed** (already `pub`) |
| `apps/burnt-rubber/src/render/foliage_texture.rs` | **only if needed** (already `pub`) |

## Files allowed to modify

Only the five above.

## Files forbidden to modify

- **`apps/burnt-rubber/src/render/mod.rs`** — reserved for `11`. It holds the
  `ScenePalette::install(app)` call at `:86`; `10` needs `:375-376`. Both of you
  are locked out because `11` must switch every call site in one coherent pass,
  and because two agents holding a 1872-line file is a merge hazard regardless of
  hunk distance.
- **`apps/burnt-rubber/src/preparation/mod.rs`** — FROZEN by `07`
- `apps/burnt-rubber/src/app.rs` — `11`
- `apps/burnt-rubber/src/preparation/{course,meshes}.rs` — `08`, `10`
- `apps/burnt-rubber/src/render/{chunks,scenery_pool,prop_meshes}.rs` — `10`
- `apps/burnt-rubber/tests/golden/**`, `slice.toml`, `tests/agent_golden.rs` —
  **read-only**

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `apps/burnt-rubber/src/render/palette.rs:534-540` | `road_materials` — `add_texture_data(ASPHALT_RES, ASPHALT_RES, asphalt_albedo())` at `:536`, verge at `:539`. **The generator is fused into the registration call** |
| `apps/burnt-rubber/src/render/palette.rs:726-790` | `ScenePalette::install` — foliage bake at `:779`, material at `:781` |
| `apps/burnt-rubber/src/render/palette.rs:554-608` | The four road materials, registered **after** the textures they cite |
| `modules/axiom/src/app/authoring.rs:132-147` | `add_texture_data` — `let id = self.custom_textures.len() as u64 + 1`. **Registration order IS the id** |
| `apps/burnt-rubber/src/render/asphalt_texture.rs:300` | `asphalt_albedo()` — argument-free, `f()==f()` tested |

## Contract consumed

From `07`, frozen:

```rust
pub struct TextureTask { pub out: Rc<RefCell<Option<PreparedTextures>>> }
```
Push order is fixed by `07`'s `tasks()`; there is no order key and no id.

## Contract produced

```rust
// apps/burnt-rubber/src/preparation/textures.rs
#[derive(Debug, Clone)]
pub struct PreparedTextures {
    asphalt: Vec<u8>,   // ASPHALT_RES x ASPHALT_RES RGBA8 (64 KB)
    verge:   Vec<u8>,   // VERGE_RES   x VERGE_RES   RGBA8 (16 KB)
    foliage: Vec<u8>,   // FOLIAGE_RES x FOLIAGE_RES RGBA8 (16 KB)
}

impl PreparedTextures {
    pub fn asphalt(&self) -> &[u8];
    pub fn verge(&self) -> &[u8];
    pub fn foliage(&self) -> &[u8];
}
```

`road_materials` and `ScenePalette::install` gain a `&PreparedTextures`
parameter. **`11` updates their call sites** in `render/mod.rs`; you change the
signatures and everything else that calls them within your owned files.

## Implementation instructions

1. **`preparation/textures.rs`** — `TextureTask::prepare` calls
   `asphalt_albedo()`, `verge_albedo()` and `foliage_albedo()` and stores the
   three `Vec<u8>`s. All three are argument-free and deterministic; simply move
   the calls.

2. **`palette.rs`** — ADD `road_materials_prepared(app, &PreparedTextures)` and
   `ScenePalette::install_prepared(app, &PreparedTextures)` beside the existing
   functions, passing `prepared.asphalt()` / `.verge()` / `.foliage()` into the
   **same** `add_texture_data` calls, in the **same order**, at the **same
   sites**. Leave `road_materials` and `ScenePalette::install` untouched and
   working.

3. **THE CRITICAL CONSTRAINT — read this twice.**
   `add_texture_data` mints `id = self.custom_textures.len() as u64 + 1`
   (`modules/axiom/src/app/authoring.rs:132`). Ids are **1-based dense
   registration-order indices**, and those ids are baked into material contents
   via `Material::with_custom_texture(t.id())` (`palette.rs:557`, `:783`), which
   is in turn encoded in the committed `_render.bin` and `_resources.bin`
   goldens.

   Therefore: **you may change *where the pixels come from*; you may not change
   *when, in what order, or how many times* `add_texture_data` is called.**
   Asphalt still first, verge second, foliage at its existing point inside
   `ScenePalette::install`.

4. **Leave the texture generator functions themselves alone** except for
   visibility changes if needed. Do not "optimise" them. Their output is pinned.

5. **Do not touch `render/mod.rs`.** Its `ScenePalette::install(app)` call at
   `:86` keeps working against the original; `11` switches it to the prepared
   variant. Nothing breaks in the meantime.

## Required behavior

- `TextureTask::prepare` yields three buffers byte-identical to today's
  generator output.
- Registration order, count and resulting ids are unchanged.
- Material contents (including `with_custom_texture` ids) are unchanged.

## Error behavior

The three generators are infallible and argument-free, so `prepare` returns
`Ok(())` unconditionally. Do **not** invent a failure mode. A consumer that finds
the cell `None` must return `Err(...PreparationFailed...)`, never `.expect`
(README §8).

## Determinism requirements

- The three bakes are already `f()==f()`-tested constants. Preserve that.
- No change to registration order — see the critical constraint above.
- No caching, no lazy statics, no `OnceCell`. Preparation runs once; that is the
  caching.

## Tests

Inline `#[cfg(test)] mod tests` in `preparation/textures.rs`:

- `preparing_produces_the_three_albedos` — expected byte lengths for each
- `two_preparations_produce_identical_pixels`
- `the_prepared_asphalt_matches_the_generator` — assert equality with a direct
  `asphalt_albedo()` call, proving the move changed nothing

## Architecture validation

`apps/` is outside the branchless, coverage and dylint gates. No `app.toml`
change.

## Performance considerations

96 KB of pixel synthesis moves from mid-construction to the preparation phase.
The total work is unchanged; only its phase moves. Note separately (and **out of
scope**) that `RunningApp::material_textures()` re-runs procedural generators on
every call — README §6 records that for a future pass.

## Documentation changes

Module doc on `preparation/textures.rs` stating what is prepared and,
prominently, the registration-order constraint.

## Completion criteria

- [ ] `TextureTask` produces all three albedos
- [ ] `road_materials_prepared` and `ScenePalette::install_prepared` added; the
      originals still present and working
- [ ] `add_texture_data` call order, count and sites unchanged
- [ ] Generator functions themselves unmodified in behaviour
- [ ] `render/mod.rs`, `app.rs` and `preparation/mod.rs` untouched
- [ ] Your own tests pass

## Validation commands

```sh
cargo test -p axiom-burnt-rubber
cargo test -p axiom-burnt-rubber --test agent_golden
git diff --name-only
```

The crate compiles throughout, so you run the **full** suite and the golden run
yourself. Golden bytes must be unchanged — your prepared variants are not called
by anything yet, so a diff means you altered the originals.

Expect **two** changed paths (`preparation/textures.rs`, `render/palette.rs`).
An earlier draft claimed "exactly five"; that was wrong — the three generators
are already `pub` and need no edit.

## Deliverable to orchestrator

Report: commit hash; five paths; the `PreparedTextures` contract as implemented;
**the exact signatures of the two new prepared variants** (so `11` can wire them
without guessing); confirmation the originals still work and all 15 golden files
are byte-unchanged;
confirmation that registration order is unchanged; deviations.
