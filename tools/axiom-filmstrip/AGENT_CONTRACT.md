# axiom-filmstrip — agent contract

How `axiom-agent` (or any automated caller) should invoke `axiom-filmstrip`.

## Purpose

Produce **one labeled contact-sheet PNG** of a real Axiom app captured
deterministically at a set of ticks or animation markers, plus a machine-readable
metadata TOML — so an agent can render a play and hand a human (or itself) a single
image to review, and reference each frame by tick/marker and hash.

The output is deterministic: the same command yields the same pixels and the same
per-frame hashes.

## Invocation

```sh
cargo run --manifest-path tools/axiom-filmstrip/Cargo.toml --release [--features offscreen] -- <FLAGS>
```

- Add `--features offscreen` to use `--backend gpu` (real wgpu). Without it,
  `--backend gpu` falls back to `canvas2d` (a notice is printed; the metadata
  records the backend that actually rendered).
- Exit code `0` on success; non-zero on any error (message on stderr).

## Tick capture mode

Capture explicit ticks. Provide `--ticks` (and **not** `--markers`).

```sh
cargo run --manifest-path tools/axiom-filmstrip/Cargo.toml --release --features offscreen -- \
  --app soccer_penalty --scenario default_penalty_kick --backend gpu \
  --ticks "0,10,20,30,40,50,60,70,80,90,100,110,120" \
  --out target/filmstrips/soccer_penalty_kick.png
```

## Marker capture mode

Capture named animation markers (resolved to ticks by the app's static trace).
Provide `--markers` (and **not** `--ticks`). Each tile is labeled with its marker.

```sh
cargo run --manifest-path tools/axiom-filmstrip/Cargo.toml --release --features offscreen -- \
  --app soccer_penalty --scenario default_penalty_kick --backend gpu \
  --markers "kicker.runup.start,kicker.left_foot.plant,kicker.hip.twist.peak,kicker.foot.ball_contact,kicker.followthrough.peak,goalie.dive.commit,ball.goal_line.cross,result.freeze" \
  --out target/filmstrips/soccer_penalty_markers.png
```

`soccer_penalty` / `default_penalty_kick` marker names:
`kicker.runup.start`, `kicker.stride.1`, `kicker.stride.2`,
`kicker.left_foot.plant`, `kicker.hip.twist.peak`, `kicker.right_leg.swing.apex`,
`kicker.foot.ball_contact`, `kicker.followthrough.peak`, `goalie.dive.commit`,
`ball.goal_line.cross`, `result.freeze`.

## Plan mode

Instead of many flags, pass a plan TOML. Explicit CLI flags override plan fields.

```sh
cargo run --manifest-path tools/axiom-filmstrip/Cargo.toml --release --features offscreen -- \
  --plan tools/axiom-filmstrip/examples/soccer_penalty_ticks.toml
```

Plan fields: `app`, `scenario`, `backend`, `viewport_width`, `viewport_height`,
`columns`, `ticks`, `markers`, `camera`, `debug_overlays`, `out`.

## Outputs

For `--out <dir>/<name>.png` the tool writes:

- **`<dir>/<name>.png`** — the contact sheet. Grid of `--columns` (default 4)
  columns; rows = `ceil(frames / columns)`. Each tile shows the frame plus a label
  band: `APP  SCENARIO` / `BACKEND  TICK <n>  [MARKER]`.
- **`<dir>/<name>.toml`** — metadata: `app`, `scenario`, `backend`, `camera`,
  `debug_overlays`, `cinematic`, `columns`, `out`, `viewport`, `ticks`, `markers`,
  `command` (the exact args), and a `[[frames]]` array (`tick`, `marker`, `width`,
  `height`, `hash`). Parse this to map tiles → ticks/markers and to compare runs by
  `hash`.

Output directories are created automatically.

## Error behavior

Non-zero exit with a stderr message; nothing is written. Cases:

- unknown `--app` (lists valid apps)
- unknown `--scenario` / `--camera` / marker (lists valid values)
- unsupported `--backend`, or a backend the app does not support
- neither or both of `--ticks` / `--markers`
- malformed `--ticks` (non-integer), `--columns`, or `--viewport`
- `--debug-overlays` on an app without a debug path
- unreadable / invalid `--plan` file
- a capture or output-write failure

Callers should check the exit code and read stderr; on success, parse the sidecar
TOML at the `.toml` sibling of `--out`.

## Extending the registry

- **Another app**: add a `FilmstripApp` row to `registry()` in
  `src/app_registry.rs`. Its `shot_name` must be a slice registered in
  `axiom-shot` (`tools/axiom-shot/src/registry.rs`). Set `native` (authored w,h),
  `backends`, `cameras`, `cinematic`, `supports_debug`, and a `markers` trace fn.
- **Another scenario**: add its name to the app's `scenarios`, teach the
  `axiom-shot` builder to build it, and add a marker-trace arm.
- **Another marker name**: add an `AnimationMarker { name, tick }` to the app's
  trace fn (e.g. `soccer_markers`), with the tick grounded in the deterministic
  scenario.
