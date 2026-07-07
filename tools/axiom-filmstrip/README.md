# axiom-filmstrip

An agent-facing, deterministic **filmstrip capture tool**. It runs a real Axiom
app from a deterministic scenario, captures the rendered frame at a list of ticks
(or named animation markers), and composes them into **one labeled contact-sheet
PNG** (plus a sidecar metadata TOML) for human review.

It is a thin orchestrator over [`axiom-shot`](../axiom-shot): the actual per-frame
rendering reuses `axiom_shot::registry` (build a slice's `RunningApp`) and
`axiom_shot::capture` (render it through the GPU/Canvas2D backend, write PNG). No
renderer is duplicated and no scene is mocked — the pixels are the real app's.

## Quick start

```sh
# GPU capture (real off-screen wgpu) — REQUIRES the offscreen feature:
cargo run --manifest-path tools/axiom-filmstrip/Cargo.toml --release --features offscreen -- \
  --app soccer_penalty --scenario default_penalty_kick --backend gpu \
  --ticks "0,10,20,30,40,50,60,70,80,90,100,110,120" \
  --out target/filmstrips/soccer_penalty_kick.png

# Software Canvas 2D capture — works in the default build:
cargo run --manifest-path tools/axiom-filmstrip/Cargo.toml --release -- \
  --app soccer_penalty --scenario default_penalty_kick --backend canvas2d \
  --ticks "0,10,20,30,40,50,60,70,80,90,100,110,120" \
  --out target/filmstrips/soccer_penalty_kick_canvas.png
```

Each run writes two files:

- `--out` PNG — the contact sheet.
- the same path with a `.toml` extension — the metadata (see below).

Output directories are created automatically.

## Flags

| Flag | Default | Meaning |
|------|---------|---------|
| `--app` | *(required)* | Registered app to capture (see `app_registry.rs`). |
| `--scenario` | app's first scenario | Deterministic scenario to replay. |
| `--backend` | `gpu` | `gpu` (needs `--features offscreen`) or `canvas2d`. |
| `--ticks` | — | Comma-separated tick list, e.g. `"0,10,20"` (tick mode). |
| `--markers` | — | Comma-separated marker names (marker mode). |
| `--out` | `target/filmstrips/<app>_<scenario>.png` | Output PNG path. |
| `--viewport` | `1280x720` | Capture bounding box `WIDTHxHEIGHT`. |
| `--columns` | `4` | Contact-sheet grid columns. |
| `--camera` | app's default | Named camera preset. |
| `--debug-overlays` | off | Overlay debug markers (per-app; soccer = goalie save volumes). |
| `--plan` | — | Load a plan TOML; explicit CLI flags override its fields. |

Provide **exactly one** of `--ticks` or `--markers`.

### Capture modes

- **Tick mode** (`--ticks "0,10,20"`): the app is deterministically advanced to
  each tick and captured.
- **Marker mode** (`--markers "kicker.foot.ball_contact,result.freeze"`): each
  marker name is resolved to a tick through the app's static marker trace, then
  captured. The label band shows the marker name.

### Viewport & aspect

`--viewport` is a bounding box. Frames are rendered at the app's **authored
aspect** fitted inside it, so nothing is ever stretched (soccer's 8:5 scene in a
`1280x720` box renders at `1152x720`). Contact-sheet tiles scale-to-fit with
letterboxing.

## Plan files

A `--plan` TOML mirrors the CLI (`app`, `scenario`, `backend`,
`viewport_width`, `viewport_height`, `columns`, `ticks`, `markers`, `camera`,
`debug_overlays`, `out`). Explicit CLI flags override plan values. See
`examples/soccer_penalty_ticks.toml` and `examples/soccer_penalty_markers.toml`.

```sh
cargo run --manifest-path tools/axiom-filmstrip/Cargo.toml --release --features offscreen -- \
  --plan tools/axiom-filmstrip/examples/soccer_penalty_ticks.toml
```

## Metadata sidecar

For `out.png` the tool writes `out.toml`: the app, scenario, backend, viewport,
columns, camera, `debug_overlays`, `cinematic`, the captured `ticks` and
`markers`, the output path, the exact `command` args, and a `[[frames]]` array
with each frame's tick, marker, size, and an FNV-1a `hash` of its RGBA (so a
capture is reproducible and comparable).

## Backends & the GPU (`offscreen`) requirement

- **`canvas2d`** — the software rasterizer; always available in the default build.
- **`gpu`** — the native off-screen wgpu path, byte-faithful to the browser's
  WebGPU/WebGL2 arm (and, for soccer, the retro-32-bit + cinematic grade). It is
  gated behind the **non-default `offscreen` feature**, mirroring `axiom-shot`:
  enabling it turns on `axiom-gpu-backend/offscreen`, which must not unify into a
  module during the workspace coverage/dylint gates. So `--backend gpu` needs
  `--features offscreen`.

If `--backend gpu` is passed **without** `--features offscreen`, the tool prints a
clear notice and renders `canvas2d` instead (the metadata records the backend that
actually rendered — GPU is never faked).

**Environment note:** this repo's dev sandbox has a working native GPU, so
`--features offscreen --backend gpu` produces real GPU captures here. In an
environment with no GPU adapter, use `--backend canvas2d`.

## Debug overlays

`--debug-overlays` is wired through per app. For `soccer_penalty` it enables the
existing goalie save-volume debug markers (rendered as real in-scene geometry, via
the `axiom-shot` `BuildParams::soccer_debug` seam). Apps with no in-pixel overlay
path return a clear "unsupported" error rather than silently ignoring the flag.

## Extending

- **Add an app**: add one `FilmstripApp` row to `registry()` in
  `src/app_registry.rs` (its `shot_name` must be a slice registered in
  `axiom-shot`'s `registry.rs`), plus its `native` size, supported `backends`,
  `cameras`, `cinematic` flag, and a `markers` trace fn.
- **Add a scenario**: add its name to the app's `scenarios` and teach the app's
  builder (in `axiom-shot`) how to build it; add a marker trace arm for it.
- **Add marker names**: add `AnimationMarker { name, tick }` rows to the app's
  trace fn (e.g. `soccer_markers`).

## Errors

The tool exits non-zero with an actionable message for: unknown app / scenario /
camera; unsupported backend; both or neither of `--ticks`/`--markers`; invalid
tick list, columns, or viewport; unknown marker name; a capture or output-write
failure; an unreadable/invalid `--plan` file.
