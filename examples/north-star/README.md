# North-Star Sketches

This directory holds **aspirational API sketches**: what an Axiom example
*should* look like once the engine is finished and has a stable, high-level
API on the tier of Godot / Unity / Unreal.

These files are design targets, not shipping code.

## What this is

A north-star sketch answers one question: *if the engine were done, how short
and how obvious would this be to write?* It is written against an imagined,
finished `axiom::prelude` — `App`, `DefaultPlugins`, `Spin`, `Camera`,
`DirectionalLight`, and friends — none of which exist yet.

Each sketch has a real, shipping counterpart somewhere in the workspace. The
gap between the two **is the work remaining**. When a sketch can be deleted
because it compiles and runs verbatim against the real API, that slice of the
engine is done.

| Sketch | Shipping counterpart today | What the gap represents |
|---|---|---|
| [`rotating_cubes.rs`](rotating_cubes.rs) | `apps/axiom-demo-rotating-cube-browser` (~8 files) | window/canvas binding, GPU backend init, surface lifecycle, render pipeline, fixed-tick driver, scene-as-data — all the engine surface the one-file `App` hides |

## What this is NOT

- **Not compiled.** None of these files are part of the Cargo workspace. The
  repo root is a virtual workspace (no root package), and Cargo only
  auto-discovers `examples/*.rs` *inside* a package, so loose `.rs` files here
  are never built.
- **Not classified.** Because they are not workspace packages, the Module Law
  (`cargo xtask check-architecture`) never sees them. They are neither a layer,
  a module, an app, nor a tool.
- **Not covered.** The Coverage Law applies to the engine spine (layers and
  modules). Sketches are not engine code and are not held to 100% — there is
  nothing to cover, because nothing here runs.
- **Not a spec.** The exact symbol names (`SceneCommands`, `Material::lit`,
  `Spin::around`) are illustrative ergonomics, not a committed API contract.
  The real API may land differently; the sketch only fixes the *shape* and the
  *altitude* we are aiming for.

## How to use a sketch

- **As a design target.** When building or re-cutting a layer/module, compare
  the real call sites against the sketch. If the real code is dramatically
  more verbose for no structural reason, that is a signal — either the
  abstraction is missing or it is in the wrong place.
- **As a deletion test.** The day a sketch compiles unmodified against the
  shipping API, move it into a real example crate and delete it from here. That
  is the definition of "done" for the slice it describes.

## Adding a sketch

1. Drop a single `.rs` file here, written against the *imagined* finished API.
2. Open it with the standard banner: **NOT COMPILED, NOT A WORKSPACE MEMBER**,
   plus a one-line pointer to its shipping counterpart.
3. Add a row to the table above so the sketch-to-reality mapping stays
   discoverable.
4. Do **not** add it to any `Cargo.toml`, `members` list, or
   `module.toml`/`app.toml`. Keeping it out of the graph is the whole point.
