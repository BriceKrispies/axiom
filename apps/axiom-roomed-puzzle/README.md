# Roomed Puzzle

A deterministic, top-down **2D grid puzzle** built as an Axiom app. A solid
player block walks a room one cell at a time. Press **q** to freeze the current
life into a **ghost** that replays your path; ghosts are solid and can stand on
buttons that open doors — so the way through a locked door is to leave a ghost
holding the button and walk the live player through.

## The twist

- The engine records every **successful** player move in the current life.
- **q** ends the life: it turns the recording into a ghost (starting at the
  entrance), resets the live player to the entrance, clears the recording, and
  **keeps** every existing ghost.
- A ghost replays its exact recorded path, **one move every 0.5 seconds**
  (deterministic fixed step — 30 ticks at 60 ticks/second). When it runs out of
  moves it stays put forever, until you restart.
- **r** restarts the level fresh: player back to the entrance, all ghosts gone,
  recording cleared, level reset.

Ghosts have the same gameplay properties as the player — they occupy cells, block
movement, stand on buttons, hold doors open, and collide with walls and closed
doors. A door is open whenever **any** solid actor (player or ghost) stands on a
button of the same wiring group.

## Level 001 — "Button Door"

A 10×10 room. A wall partition at column `x=7` seals off the exit corridor; the
door at `(7,5)` is the *only* gap, so the exit genuinely cannot be reached without
opening it. The button at `(4,5)` is too far from the door to hold and cross at
once. The solution:

1. Walk the player from the entrance `(1,5)` to the button `(4,5)`.
2. Press **q** — your path becomes a ghost.
3. The ghost walks back onto the button and holds the door open.
4. Walk the (reset) live player through the open door to the exit `(8,5)`.

## Controls (playtest mode)

| Key | Action |
| --- | --- |
| Arrow keys / WASD | Move one cell |
| **q** | Leave a ghost from this life & reset to the entrance |
| **r** | Restart the level fresh (clear all ghosts) |

## Editor (edit mode)

- Pick a tile from the palette (Floor erases, Wall, Entrance, Exit, Button, Door).
- Click a cell to paint it. Buttons/doors use the **group** field (default
  `main`).
- The validation panel updates live; **▶ Playtest** is enabled only when the
  level validates.
- **Export** writes the current level as TOML into the textarea; **Import** loads
  TOML back into the editor.
- **◀ Back to edit** returns from playtest without losing the edited level.

The TOML schema is documented in [`LEVEL_FORMAT.md`](LEVEL_FORMAT.md); the
architecture and determinism model in [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Build & run

### Native tests (the deterministic core)

The whole game core, validation, codec, editor, and the 17 required behaviours
run on native:

```sh
cargo test -p axiom-roomed-puzzle
```

### In the browser (the editor + playtest surface)

The browser surface is the wasm-only `web` arm. Build it with
[`wasm-pack`](https://rustwasm.github.io/wasm-pack/) into `web/pkg/`, then serve
the `web/` directory and open it:

```sh
# from the repo root
wasm-pack build apps/axiom-roomed-puzzle --target web --out-dir web/pkg

# serve the app directory over http://localhost (any static server works)
python -m http.server 8080 --directory apps/axiom-roomed-puzzle/web
# open http://localhost:8080/
```

The page uses only the 2D canvas (no WebGPU required).

### Browser smoke test (Playwright controller)

With the app served as above, the repo's Playwright controller can drive a real
browser (see the root `CLAUDE.md`):

```sh
uv run scripts/playwright_controller.py goto http://localhost:8080/
uv run scripts/playwright_controller.py wait 800
uv run scripts/playwright_controller.py console        # check for errors
uv run scripts/playwright_controller.py screenshot puzzle
```

## Where things live

- Deterministic game core: `src/game_state.rs`, `src/game_step.rs`,
  `src/ghost_replay.rs`, `src/current_life_recording.rs`, `src/actor_state.rs`.
- Level model / validation / TOML: `src/level_definition.rs`,
  `src/level_validation.rs`, `src/level_codec.rs`, `levels/001-button-door.toml`.
- Authoring & play: `src/editor_model.rs`, `src/playtest_model.rs`,
  `src/render_model.rs`, `src/input_mapping.rs`, `src/app.rs`.
- Browser shell (wasm only): `src/web.rs`, `web/index.html`.

> **Note** `web/pkg/` (the wasm-pack output) is a build artifact and is not
> checked in; run the `wasm-pack build` command above to produce it.
