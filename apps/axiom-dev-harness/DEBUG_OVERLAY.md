# Browser Debug Overlay & Command Console

A developer debug overlay for the live browser/WASM engine surface, plus a tiny
in-overlay command console. The overlay itself is the **`axiom-debug-overlay`
engine module**; this harness app (`apps/axiom-dev-harness`) is a thin host
that mounts it over a bare canvas via the module's measured-diagnostics driver.

## Keyboard contract

All four shortcuts use the **physical** key (`KeyboardEvent.code == "Backquote"`):

| Shortcut            | Action                                  |
| ------------------- | --------------------------------------- |
| `` ` ``             | Toggle the overlay (show / hide)        |
| `Shift` + `` ` ``   | Cycle density: compact → normal → verbose → compact |
| `Ctrl` + `` ` ``    | Pin / unpin the overlay                 |
| `Alt` + `` ` ``     | Open the overlay and focus the console  |

Rules: Backquote is **not** stolen while a normal `input`/`textarea`/
`contenteditable` is focused — except the overlay's own console; `preventDefault`
fires only for handled chords; a held meta key (Cmd/Win) is left to the OS; a
pinned overlay ignores the toggle's hide (unpin, or run `overlay.hide`).

## Moving the window

Drag the **title bar** (the green header) to move the overlay anywhere on screen
— it clamps to stay within the viewport. The position + drag math is the module's
pure, branchless `DragState`; the wasm arm wires it to header pointer events
(mouse, touch, and pen, via Pointer Capture).

## Console commands

`>` prompt at the bottom; Enter submits, Escape blurs (overlay stays open),
ArrowUp/ArrowDown walk the in-memory history. Routed through a **real registry**
— no `eval`/`Function`/dynamic import; unknown commands return a clean error;
empty input is ignored.

`help`, `clear`, `overlay.compact`, `overlay.normal`, `overlay.verbose`,
`overlay.pin`, `overlay.unpin`, `overlay.hide`, `diagnostics.snapshot`,
`backend.report`, `replay.mark` (stub), `perf.mark` (stub).

## Density modes

- **compact** — title, fps, frame time, renderer backend, fallback count.
- **normal** — the core diagnostics read-out plus the command-history count.
- **verbose** — everything in normal, plus the raw backend selection, the
  overlay's own debug state, and a command-history preview.

## Architecture & boundaries

The overlay lives in `modules/axiom-debug-overlay`, the **fourth sanctioned
platform-facing module** (Module Law #9, alongside `windowing`/`gpu-backend`/
`canvas2d-backend`). It is held to the full engine spine discipline:

- **One facade** (`DebugOverlayApi`) — Module Law #8. Diagnostics cross it as
  primitives (booleans, integers, `&str`, tuples); **no naked float** crosses
  (timing is integer-encoded `fps_milli` / `frame_time_micros`, so the
  `engine_no_unitless_float_public_api` lint is satisfied).
- **Branchless core** — the whole state machine (density, command
  registry/dispatch, console history, keyboard classification, the diagnostics
  model) is branchless (`toggle` is `!visible | pinned`, density cycling and
  shortcut dispatch are `const` tables indexed by an enum discriminant, …).
- **100% covered** — every region/line/function of the pure core is exercised by
  native tests.
- **DOM in the wasm arm only** — `web_sys` lives in `dom_binding.rs`
  (`#[cfg(target_arch = "wasm32")]`), behind the native-clean facade, exactly
  like windowing's live presentation arm. It never enters the native build, the
  coverage gate, or the branchless lint.

The deterministic engine spine (kernel/runtime/math, the layers) still knows
nothing about the DOM, keyboard, CSS, or command text.

## Diagnostics are real, fed by the driver — never engine state

The overlay only ever *reads* diagnostics pushed in through the facade; it is a
read-out, never a source of engine state. The module's measured-diagnostics
driver (`DebugOverlayApi::mount_with_measured_diagnostics`, which this harness
mounts) feeds **real** values — there is no stub provider:

- **fps / frame time** — measured from `requestAnimationFrame` deltas.
- **frame index** — the RAF frame counter.
- **visibility** — `document`'s hidden flag.
- **renderer backend / fallback** — a real `navigator.gpu` capability probe,
  reporting the engine's actual WebGPU→WebGL2 fallback choice.
- Fields the driver can't observe without running the engine (sim ticks, GPU
  submissions, worker messages) are honest zeroes; absent subsystems
  (storage/audio/network) are honest `none`.

A different host (a real engine app) implements the same `set_frame` /
`set_backends` / `set_counters` / … seam with its own real engine facts — the
overlay code does not change.

## Browser verification

No automated browser/e2e harness is wired into the native suite yet, so
DOM-level verification is by hand. Build the wasm bundle into
`apps/axiom-dev-harness/web/pkg/` (cargo build --target wasm32 + wasm-bindgen),
serve `apps/axiom-dev-harness/web/`, then drive it with the Playwright
controller:

```sh
uv run scripts/playwright_controller.py goto http://localhost:8000/
uv run scripts/playwright_controller.py eval "(() => { window.dispatchEvent(new KeyboardEvent('keydown',{code:'Backquote',bubbles:true})); const e=document.getElementById('axiom-debug-overlay'); return !!e && !e.hasAttribute('hidden'); })()"
uv run scripts/playwright_controller.py screenshot overlay
```
