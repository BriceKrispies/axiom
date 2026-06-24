# `axiom-debug-overlay` — architecture

The developer debug overlay + command console for the browser engine surface,
extracted into a real engine module so any browser app can mount it.

## Why it's a module (and a platform-facing one)

A debug overlay binds the DOM, and Module Law #9 forbids `web_sys` in a module —
*except* the platform-facing allowlist (`PLATFORM_FACING_MODULES` in
`crates/xtask/src/hygiene.rs`). This is the fourth sanctioned entry, alongside
`windowing`, `gpu-backend`, and `canvas2d-backend`. Like those, it splits into a
native-clean core and a `wasm32`-only arm:

```
DebugOverlayApi            (lib.rs / overlay_api.rs)  — the one public facade
  ├─ overlay_state         the pure model (visibility/pin/density/console/diag)
  ├─ overlay_density       density ring (const-table indexed by discriminant)
  ├─ command(+_registry)   parse + static dispatch table (no eval)
  ├─ console               history navigation + result log
  ├─ keyboard              Backquote/console-key classification + shortcut table
  ├─ diagnostics           the read-out model + integer→text formatters
  └─ dom_binding           #[cfg(wasm32)] — the real web_sys DOM + listeners
```

## The laws it satisfies

- **One facade** (#8): `lib.rs` exports only `DebugOverlayApi`. Diagnostics cross
  it as primitives/tuples; **no naked float** (timing is `fps_milli` /
  `frame_time_micros` integers), so `engine_no_unitless_float_public_api` passes.
- **Branchless** (`engine_no_branching`, baseline 0): the whole core is branch-
  free. `toggle` is `self.visible = !self.visible | self.pinned`; density cycling,
  shortcut dispatch, and labels are `const` tables indexed by a fieldless-enum
  discriminant; conditionals are `then`/`map`/`unwrap_or` chains.
- **100% coverage**: every region/line/function of the native core is tested.
- **Platform arm is gate-exempt**: `dom_binding` compiles only for `wasm32`, so
  it never enters the native build, the coverage gate, or the branchless lint —
  it is verified in a real browser.

## Diagnostics contract

The overlay never reads engine state directly. The host (an app) calls the
`set_frame` / `set_backends` / `set_counters` / `set_fallback` / `set_visibility`
seam with real, measured engine facts; the overlay formats and displays them.
This keeps every two-tier pairing re-composable: the dev harness feeds
browser-measured values, a future engine app feeds its own — with no overlay
change.
