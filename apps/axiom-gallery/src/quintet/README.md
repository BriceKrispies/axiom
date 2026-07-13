# Quintet

A deterministic block-breaking placement game built as an Axiom app.

Place a generated **quintet** — a 5-cell polyomino — onto a 10×10 board. Fill
any whole row or column to clear it; clearing several lines with one placement
multiplies the points each cleared block is worth. Every piece the generator
offers is guaranteed to fit somewhere; when nothing can fit, the game shows a
stuck state and you press **Reset**.

## Play

* **Press the board** — the waiting quintet is summoned and hovers under your
  cursor/finger. (Dragging it from the *Next Quintet* panel still works too.)
* The snapped preview reads **green** where it will land validly, **red** when it
  is off-board or overlapping a filled block.
* **Release** on a valid spot to place it; release anywhere invalid to return it
  to the panel.
* **Undo** rewinds the last placement in full — any rows/columns it cleared come
  back, the score and move count rewind, and the same piece returns to the
  panel. Press it repeatedly to rewind play by play.
* **Reset board** clears everything and starts a fresh game.

## Scoring

```text
lines_cleared  = (#full rows) + (#full columns)
cleared_blocks = unique cells removed (a row∩column cell counts once)
points         = cleared_blocks * lines_cleared
```

So one full row scores 10; two full rows score 40; one row + one column sharing
a cell removes 19 unique blocks for 38.

## Build (browser)

The app is wired into the demo gallery; build it like the other wasm demos:

```sh
cargo build -p axiom-quintet --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir apps/axiom-quintet/web/pkg \
  target/wasm32-unknown-unknown/release/axiom_quintet.wasm
```

Then serve `apps/axiom-quintet/web/` (or run `make gallery-build && make
gallery`).

## Test

The deterministic game core is plain Rust and fully native-testable:

```sh
cargo test -p axiom-quintet
```

See `ARCHITECTURE.md` for the placement rationale (app, kernel-only).
