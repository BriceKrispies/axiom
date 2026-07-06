# Live level hot-reload (retro FPS browser demo)

Edit the level while the demo is running in the browser and watch it update —
no recompile, no page reload. Lengthen a wall, move an enemy spawn, recolour the
rooms, raise the ceiling: save the file and the next frame renders the change.

## What this is

The retro FPS level is no longer compiled-in Rust constants. It is a **document** —
`level.axiom` in this directory — parsed at runtime into a `LevelDoc`
(`src/level.rs`): the wall grid plus every gameplay/visual tunable. The same file
is embedded into the wasm build at compile time (via `include_str!`) as the
built-in default, so the demo runs standalone too.

The pieces:

| Piece | Where | Role |
|-------|-------|------|
| `level.axiom` | this dir | the editable level document (and compile-time default) |
| `LevelDoc` / parser | `src/level.rs` | text → grid + tunables |
| `reload_retro_fps` | `src/lib.rs` | rebuild the game + re-author the engine scene from a doc |
| `RunningApp::reauthor` | `modules/axiom/src/app.rs` | engine capability: rebuild the scene in place while ticking |
| SSE client | `src/web.rs` | subscribes to `/events`, applies a new doc at a tick boundary |
| dev server | `tools/axiom-dev-reload` | serves `web/` + pushes `level.axiom` edits over SSE |

The reload is applied at a frame (tick) boundary; the engine frame tick keeps
counting monotonically across reloads (the host driver requires it), so the
demo stays deterministic — a reload is just an explicit event applied at a tick.

## Running it

```sh
make retro_fps-build          # build the wasm bundle into web/pkg (once, and after Rust changes)
make retro-fps-hot            # serve web/ + watch level.axiom, with SSE, at http://localhost:8080
```

Then open <http://localhost:8080> in a WebGPU browser (recent Chrome/Edge), and
edit `apps/axiom-retro-fps-browser/level.axiom`. On every save the level reloads live.
The browser console logs `level reloaded from edit` each time.

Equivalently, without the Makefile:

```sh
cargo build -p axiom-retro-fps-browser --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir web/pkg \
    target/wasm32-unknown-unknown/release/axiom_retro_fps_browser.wasm
cargo run -p axiom-dev-reload          # defaults: port 8080, serves web/, watches level.axiom
```

If you serve the page with a plain static server instead (`make retro_fps`), there is
no `/events` endpoint; the demo simply runs the built-in level and hot-reload is
silently disabled. Hot-reload is a dev convenience, never required.

## What you can edit live

Everything in `level.axiom`: the `[map]` grid (`#` wall, `.` floor, `S` start,
`E` enemy spawn), `wall_height`, the `color_*` triples, movement/enemy speeds,
and the combat tunables. A saved edit resets the player to the level start.

## Current limits (deliberate)

- **Grid size is bounded by the initial grid.** The live renderer's per-instance
  buffer is sized once at startup to the starting grid's capacity
  (`width*height + 2`). Editing *within* that grid — including filling it with
  walls — is fine; growing the grid *larger* than it started would exceed the
  buffer and clamp. Lifting that needs an instance-buffer reallocation in
  `axiom-windowing` (a tracked follow-up), not part of this feature.
- **The base mesh is not re-uploaded.** Everything is the one cube primitive
  instanced N times, so changing a wall's size/position/colour is per-instance
  data that already flows every frame — no geometry upload needed.
- **Behaviour is still compiled.** This hot-reloads level *data*, not gameplay
  *code* (enemy AI, shooting). Changing Rust still needs `make retro_fps-build`.
