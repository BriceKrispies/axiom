# Browser Debug Overlay & Command Console

A developer debug overlay for the live browser/WASM engine surface, plus a tiny
in-overlay command console. It is mounted over a bare canvas by this harness app
(`apps/axiom-browser-dev-harness`).

## Keyboard contract

All four shortcuts use the **physical** key (`KeyboardEvent.code == "Backquote"`),
not `KeyboardEvent.key`, so they work on every layout:

| Shortcut            | Action                                  |
| ------------------- | --------------------------------------- |
| `` ` ``             | Toggle the overlay (show / hide)        |
| `Shift` + `` ` ``   | Cycle density: compact → normal → verbose → compact |
| `Ctrl` + `` ` ``    | Pin / unpin the overlay                 |
| `Alt` + `` ` ``     | Open the overlay and focus the console  |

Rules:

- Backquote is **not** stolen while a normal `input` / `textarea` /
  `contenteditable` element is focused — **except** when the overlay's own
  console input owns focus (then `` ` `` still drives the overlay).
- `preventDefault` is called **only** for a Backquote chord the overlay actually
  handles. A held platform meta key (Cmd/Win) is left to the OS.
- **Pinning protects against an accidental close**: a pinned overlay ignores the
  `` ` `` toggle's hide. Unpin with `Ctrl` + `` ` ``, or run `overlay.hide`.

## Overlay

- Hidden by default; lightweight `position: fixed` panel at the top-left, over
  the canvas — it never resizes or replaces the canvas.
- `pointer-events: none`, so it does **not** block game input; only the console
  input opts back in, and only keystrokes are captured while it is focused.
- Sharp rectangular styling (no rounded corners, no pills), monospace, high
  contrast, compact spacing, ~360px wide (verbose is wider). Readable over both
  light and dark scenes. Styles live in one injected `<style>` block owned by the
  overlay (`OVERLAY_CSS` in `src/debug_overlay.rs`).

### Density modes

- **compact** — title, fps, frame time, renderer backend, fallback count.
- **normal** — the core diagnostics read-out plus the command-history count.
- **verbose** — everything in normal, plus the raw backend selection, the
  overlay's own debug state, and a command-history preview.

## Command console

Lives at the bottom of the overlay with a `>` prompt. Enter submits, Escape blurs
(the overlay stays open), ArrowUp/ArrowDown walk the in-memory history. The last
few results render above the input.

Commands are **stubbed but routed through a real registry/dispatcher** — there is
no `eval`, `Function` constructor, dynamic import, or arbitrary script execution.
Parsing trims whitespace and splits the command name from its arguments; an empty
line does nothing; an unknown command returns a clean error.

| Command                | Effect                                              |
| ---------------------- | --------------------------------------------------- |
| `help`                 | List the available commands                         |
| `clear`                | Clear the command output                            |
| `overlay.compact`      | Set density to compact                              |
| `overlay.normal`       | Set density to normal                               |
| `overlay.verbose`      | Set density to verbose                              |
| `overlay.pin`          | Pin the overlay                                     |
| `overlay.unpin`        | Unpin the overlay                                   |
| `overlay.hide`         | Hide the overlay                                    |
| `diagnostics.snapshot` | Print a text snapshot of the current diagnostics    |
| `backend.report`       | Print renderer/canvas/sim/storage/audio/network     |
| `replay.mark`          | Acknowledge a replay marker (stub)                  |
| `perf.mark`            | Acknowledge a performance marker (stub)             |

## Architecture & boundaries (important)

This is **app-side developer tooling**. The deterministic engine spine — the
kernel, runtime, math, and every layer/module — never learns about the DOM,
keyboard events, CSS, canvas, WebGPU, browser timing, or command text. All of
that lives here, in an app: the only tier permitted to reference `web_sys`
outside the platform-facing `host` layer and `windowing` module (Module Law #9).

- **The overlay consumes host diagnostics; it must never become deterministic
  engine state.** It only ever *reads* a `BrowserDiagnosticsSnapshot`.
- **Browser/DOM APIs stay in the browser host/app surface** (`src/debug_overlay.rs`
  and `src/web.rs`, both `#[cfg(target_arch = "wasm32")]`).
- **Future diagnostics are fed in through `BrowserDiagnosticsSnapshot`**, never
  hardcoded into the overlay. Today the values come from the replaceable
  `StubDiagnosticsProvider`; a real host implements `DiagnosticsProvider` and
  swaps it in, with no overlay changes.

### What's tested where

The pure logic — density cycling, command parsing/registry/dispatch, console
history navigation, keyboard-shortcut classification, the diagnostics snapshot —
is browser-free Rust that compiles on **native** and is fully unit-tested under
`cargo test --workspace` (plus an end-to-end `tests/overlay_pipeline.rs`). The DOM
controller (`DebugOverlayController`) and the `#[wasm_bindgen]` entry are the thin
`wasm32` edge.

### Browser verification

There is no automated browser/e2e test harness wired into the native test suite
yet, so **browser-level verification of the live DOM is pending**. To check it by
hand, build and serve the harness and drive it with the repo's Playwright
controller:

```sh
make harness-build      # cargo build --target wasm32 + wasm-bindgen into web/pkg
make harness            # serve apps/axiom-browser-dev-harness/web at http://localhost:8000

uv run scripts/playwright_controller.py goto http://localhost:8000/
uv run scripts/playwright_controller.py eval "(() => { const e = new KeyboardEvent('keydown', {code:'Backquote', bubbles:true}); window.dispatchEvent(e); return !!document.getElementById('axiom-debug-overlay') && !document.getElementById('axiom-debug-overlay').hasAttribute('hidden'); })()"
uv run scripts/playwright_controller.py screenshot overlay
```
