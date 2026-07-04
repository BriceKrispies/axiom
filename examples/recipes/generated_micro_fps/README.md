# Generated Micro-FPS — an Axiom recipe project

A complete, small, **playable industrial sci-fi training facility**, generated
entirely from *recipes* by the existing Axiom procedural pipeline
(`axiom-recipe` + `axiom-proc-texture` + `axiom-proc-mesh`) and expanded into an
ordinary Axiom scene with a gameplay ruleset.

Nothing here is a new engine crate or a new procedural operator. This project is
a **standalone example** (its `Cargo.toml` carries an empty `[workspace]` table,
so it is *not* a workspace member and touches none of the engine's coverage /
branchless / architecture gates). It only *composes* the existing operators into
a hierarchy of reusable recipe macros, then turns their output into runtime
resources through the ordinary hooks `add_texture_data` / `add_mesh_data` /
`add_material`.

> **The load-bearing idea: ship the recipe, not the resources.** The whole game's
> art is **1796 bytes** of packed recipe that expand into ~0.29 MB of textures and
> ~100 scene objects, deterministically, from one seed.

## Run it

All commands run against this crate's manifest (it is not part of the root
workspace):

```sh
# The size / performance report (default command):
cargo run --manifest-path examples/recipes/generated_micro_fps/Cargo.toml -- report

# Run every validation check (exit non-zero on any failure):
cargo run --manifest-path examples/recipes/generated_micro_fps/Cargo.toml -- validate

# Expand the menu tableau + the three-area level and print scene counts:
cargo run --manifest-path examples/recipes/generated_micro_fps/Cargo.toml -- expand

# Export the packed recipe blob (the shippable artifact):
cargo run --manifest-path examples/recipes/generated_micro_fps/Cargo.toml -- pack micro_fps.pack

# The full test + validation suite:
cargo test --manifest-path examples/recipes/generated_micro_fps/Cargo.toml
```

## What it generates

A hierarchical recipe project (each layer in its own file under `src/`):

| File | Role |
|------|------|
| `style.rs` | The one `Style`: level **seed**, shared **palette**, art-direction knobs (grime, contrast, panel size, resolution). |
| `textures.rs` | 12 **texture recipe macros** — walls, floor, doors, gates, crate, pipe, light, both enemy surfaces, weapon, exit — parameterized by the style. |
| `meshes.rs` | 10 **mesh recipe macros** — wall panel, floor tile, door, crate, pipe, light bar, both enemy bodies, weapon, exit pillar. |
| `materials.rs` | 13 **materials** binding a generated texture to a palette base color (+ emissive) — one shared global look. |
| `prefabs.rs` | 12 **prefabs** = mesh + material + gameplay tag. |
| `grammar.rs` | The **seeded scene grammar**: reusable room-shell / corridor / scatter macros compose the level. Nothing is hand-placed per object. |
| `scenes.rs` | The **title/menu tableau** and the **three-area level**, expanded into a live `RunningApp` with a first-person controller + light. |
| `gameplay.rs` | The deterministic **gameplay ruleset**: spawn, enemy health, weapon pickup, hitscan, damage, death, gate unlock, win. |
| `pack.rs` | The **packed-recipe export** + the **size report**. |
| `validation.rs` | The nine **validation checks**. |

The generated game has: a menu tableau; one generated level of three connected
areas — **start room → corridor (normal door) → combat room → locked gate →
final room**; a player spawn + first-person camera; a generated weapon pickup;
two enemy prefab variants ("grunt" box and "sentry" cylinder), four instances
scattered by the seed; a **locked gate** that opens only when the combat room is
cleared; and a **win trigger** at the exit in the final room.

## Gameplay rules

Provable deterministically (see `gameplay.rs` tests): the player starts unarmed
with 100 HP and a locked gate; touching the weapon grants it; hitscan fire kills
the nearest enemy in range (grunt = 2 shots, sentry = 2 shots); enemies in melee
range drain HP; 0 HP = death; clearing all enemies unlocks the gate; reaching the
exit **with the gate open** wins.

## Rendering it

The expanded scene renders through the engine's real renderer. Headless proof
(the same `scene_renderer` the browser's WebGPU/WebGL2 path runs), via the
`axiom-shot` tool:

```sh
cargo run --manifest-path tools/axiom-shot/Cargo.toml -- --app micro-fps      --backend gpu --tick 1 --out screenshots/micro_fps_level.png
cargo run --manifest-path tools/axiom-shot/Cargo.toml -- --app micro-fps-menu --backend gpu --tick 1 --out screenshots/micro_fps_menu.png
```

### In a browser (wasm)

The project ships a live browser build (`src/web.rs`): it expands the level, then
drives the engine's `run_web_multi` present loop with `max_instances` set to the
real generated renderable count. `micro_fps_start` is a **navigable** first-person
camera — WASD to move, **click the canvas to capture the mouse and look around**
(pointer-lock mouse-look, cloned from the gallery's `forest_walk` demo;
arrow-left/right also turn). `micro_fps_overview_start` is a fixed overview.
Build + serve:

```sh
cd examples/recipes/generated_micro_fps
cargo build --target wasm32-unknown-unknown --lib
wasm-bindgen --target web --no-typescript --out-dir web \
  target/wasm32-unknown-unknown/debug/generated_micro_fps.wasm
python -m http.server 8099 --directory web
# then open http://localhost:8099/index.html?backend=canvas2d   (first person)
#           http://localhost:8099/overview.html?backend=canvas2d (overview)
```

`?backend=canvas2d` forces the software renderer; drop it (or use
`?backend=webgpu`) where the browser has working WebGPU. The canvas2d arm renders
darker/stylized than the WebGPU/native GPU arm.

## Known limitations

Honest notes on where the existing operators / engine could not express something
exactly, and how it was approximated:

- **No text/UI operator.** The "menu" is a readable *title tableau* (the weapon
  glowing on a crate pedestal under lights), not a labelled menu with buttons.
- **Gameplay is a deterministic model, not fully live-wired.** The level is
  live-**navigable** (a real `spawn_controller` first-person camera drives the
  scene in a browser build). The *combat / gate / win* rules are proven by the
  deterministic `GameState` simulation over the generated layout, driven by
  high-level intents (the same intents a WASD+fire mapping would emit). Wiring
  those intents to live per-frame input + scene raycast is the remaining step;
  the rules themselves are complete and tested.
- **The gameplay sim is logical, not physical.** `MoveToward` walks straight to a
  point (no collision) — collision/navigation belongs to the live layer, not the
  rule model.
- **Materials, prefabs, scene grammar, and gameplay are composition, not
  operators.** The procedural pipeline generates *textures and meshes* from
  recipes; there is no material/scene/gameplay operator, so those tiers are
  expressed as parameterized Rust composition over the baked recipe outputs. Only
  the textures and meshes are packed as recipes.
- **Rooms are open-topped** (walls + floor + hanging lights, no ceiling tiles) —
  a deliberate readability/entity-count choice for a small arena, not an operator
  limitation.
- **Enemy A uses `Displace`**, which is a v0 noise displacement over the whole
  body (an irregular silhouette), not articulated limbs.
- **Browser rendering uses canvas2d here.** The live browser build works (see
  *In a browser*), but this sandbox has no browser WebGPU, so the screenshots use
  the canvas2d software backend, which renders darker/stylized than the WebGPU or
  native-GPU arm. On a machine with working browser WebGPU, drop `?backend=` for
  the bright GPU look.
- **Combat is not yet wired to the live browser input.** The browser build is
  live-navigable (WASD + pointer-lock mouse-look drive the first-person camera).
  Firing / enemy
  damage / gate / win are proven by the deterministic `GameState` model; mapping
  those to live per-frame input + scene raycast is the remaining step.

## Budgets — all met

| Budget | Limit | This project |
|--------|-------|--------------|
| Packed recipe | < 150 KB | **1796 bytes** (≈ 1.2 %) |
| Generated mesh vertices | < 200,000 | **452** (10 unique meshes; instances reuse them) |
| Generated texture memory | < 96 MB | **0.29 MB** |
| New engine crates | 0 | **0** (standalone example) |
| New procedural operators | 0 | **0** |
| Imported image/mesh/audio assets | 0 | **0** |

## Final size / performance report

```
Generated Micro-FPS — size & performance report
  level seed             : 0x00000fac11175eed
  determinism hash       : 0xf282f82ab0e085fd
  packed recipe (shipped): 3520 bytes
  texture RAM (generated): 344064 bytes (0.34 MB)
  mesh vertices          : 692
  mesh indices           : 2760
  recipes                : 15 textures, 20 meshes, 16 materials
  scene                  : 167 renderables, 4 enemies, 169 entities
  expansion time         : ~50 ms (varies by machine)
```

`validate` output:

```
  [PASS] 1. same seed → same determinism hash
  [PASS] 2. expands from recipe data only (no editor data)
  [PASS] 3. entity count bounded
  [PASS] 4. vertex + index counts bounded
  [PASS] 5. texture memory bounded
  [PASS] 6. recipe graphs are acyclic
  [PASS] 7. all referenced recipes/resources resolve
  [PASS] 8. packed recipe produced (< 150 KB)
  [PASS] 9. size report generated
9/9 checks passed
```

See [`NOTES.md`](NOTES.md) for the design rationale and the recipe-graph shapes.
