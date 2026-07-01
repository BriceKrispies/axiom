# axiom-zanzoban — Architecture

A deterministic top-down 2D grid puzzle with ghost replay, built as an Axiom
**app**.

## Architectural placement

This is an **app** (`apps/axiom-zanzoban/`, classified by `app.toml`) — a
composition leaf. It is **not** a kernel change, **not** a new engine layer, and
**not** a reusable engine module. Per the Axiom Module Law, apps are the only
leaf composition roots; nothing depends on them, and they are exempt from the
branchless and 100%-coverage spine gates. That is exactly why all the gameplay —
rules, level format, editor, ghost replay, orchestration — lives here and is not
pushed down into a layer or module.

```
apps/axiom-zanzoban/
├── app.toml                 # classifies this crate as an app
├── Cargo.toml               # deps: axiom-kernel + toml/serde (+ wasm-only browser crates)
├── levels/001-button-door.toml
├── src/
│   ├── lib.rs               # crate root; embeds Level 001; wires the wasm `web` arm
│   ├── coord.rs             # GridCoord + room dimensions
│   ├── direction.rs         # Direction (Up/Down/Left/Right) + deltas
│   ├── group_id.rs          # GroupId — the button↔door wiring link
│   ├── tile_kind.rs         # TileKind — the editor palette
│   ├── level_definition.rs  # LevelDefinition (the canonical level)
│   ├── level_validation.rs  # validation + LevelValidationReport (+ LevelCensus)
│   ├── level_codec.rs        # TOML (de)serialization
│   ├── actor_state.rs       # ActorId / ActorKind / ActorState
│   ├── current_life_recording.rs
│   ├── ghost_replay.rs      # GhostReplay + the 30-tick cadence
│   ├── game_command.rs      # PuzzleCommand / PuzzleStepResult / StepKind
│   ├── game_state.rs        # PuzzleGameState — the deterministic simulation
│   ├── game_step.rs         # step() — the single transition door
│   ├── render_model.rs      # neutral, depth-cued draw description
│   ├── input_mapping.rs     # key string → PuzzleCommand
│   ├── editor_model.rs      # edit-mode model (paint, validate, TOML I/O)
│   ├── playtest_model.rs    # playtest-mode model
│   ├── app.rs               # ZanzobanApp — edit ⟷ playtest mode machine
│   └── web.rs               # wasm32-only 2D-canvas + DOM shell
└── web/index.html           # the page chrome that drives the wasm
```

## The one engine dependency, and why it is genuine

The app depends on exactly one engine layer, the **kernel**, and uses it for
something real: the kernel's deterministic fixed-step time primitives
(`FixedStep`, `Tick`, `SimulationClock`) are the clock that paces ghost replay.
`PuzzleGameState` owns a `SimulationClock`, advances it one `FixedStep` per
`Tick` command, and never reads a wall clock. The task's requirement —
"deterministic fixed-step time for ghost replay … based on the app/runtime fixed
step" — *is* the kernel's clock. Declaring the kernel without using it would be a
ceremonial dependency (forbidden); here it is load-bearing.

No engine **module** is depended on (`allowed_modules = []`). See the rendering
decision below for why the `axiom`/`windowing` modules are deliberately absent.

## Rendering: a 2D canvas, not the 3D cube pipeline

The game is logically 2D — a top-down grid editor and a top-down board with
translucent ghosts, recessed open doors, and pressed buttons. Axiom's live render
path (`axiom-windowing`'s instanced **cube** drawer, reached through the `axiom`
umbrella) is a 3D scene-graph presenter; it cannot express an interactive
click-to-paint grid editor, alpha-blended ghosts, recessed doorways, or the
edit/playtest mode switch. Forcing the game through it would mean fighting a
mismatched tool *and* declaring umbrella/windowing dependencies the game does not
genuinely use.

The task explicitly sanctions the alternative: *"If the current engine does not
yet have enough rendering surface for this, build … a narrow app-local browser
playtest surface following the existing app boundary rules."* `axiom-growth`
already establishes that pattern — its overworld map is a 2D `<canvas>` drawn from
Rust via `CanvasRenderingContext2d`, with DOM input, all confined to the app's
wasm `web` arm. This app follows it: a 2D canvas with **faux-3D depth cues** (a
beveled raised look for walls/closed doors, a recessed look for open doors,
slightly raised/depressed buttons, an opaque player block, translucent outlined
ghost blocks). Browser/DOM/canvas APIs appear **only** in `web.rs`, never in the
core, and `web.rs` is `#[cfg(target_arch = "wasm32")]`, so native `cargo test`
never compiles it.

The depth-cue *decisions* live in the browser-free `render_model.rs`
(`RenderTile::elevation`, the ghost/player alphas), so the visual rules are
unit-tested on native; `web.rs` only plots them.

## Determinism

`PuzzleGameState` is pure simulation:

- **Time** is the kernel `SimulationClock`, advanced one `FixedStep` per `Tick`.
  The browser shell converts wall-clock `requestAnimationFrame` deltas into a
  whole number of `Tick`s at 60 Hz (the fixed step); the core only ever sees
  `Tick`s, so it never reads a clock.
- **No randomness**, no hidden global state, stable iteration order (ghosts in a
  `Vec` in creation order).
- `PuzzleGameState: PartialEq + Eq`, so "two identical command streams produce
  identical state traces" is asserted on the whole state, not a summary.

### Ghost replay cadence

A ghost takes one recorded move every `GHOST_STEP_TICKS = 30` ticks — 0.5 s at
the app's 60-tick/second fixed step (`TICKS_PER_SECOND / 2`). Each ghost owns its
own countdown, so a ghost created mid-game still moves once per half-second from
its own creation. The cadence is pure tick counting; the relationship to the
fixed step is asserted in tests.

### Occupancy, buttons, doors, and actor order

Both the live player and every ghost are *solid actors* occupying one cell. A
button's group is *pressed* while any actor stands on any button of that group; a
door is *open* exactly while its group is pressed, re-evaluated on demand — so it
closes the instant the last actor leaves the button. A move into a wall, a closed
door, an out-of-grid cell, or a cell another actor occupies fails; a failed
live-player move is **not** recorded.

On a `Tick`, ghosts resolve in creation order, each seeing the updated positions
of earlier ghosts (immediate door re-evaluation included). The live player never
moves on a tick — it moves only on its own `Move` command — so it is naturally
last in the stable order the task specifies. A ghost consumes exactly one
recorded move per step window whether or not it physically moves (the recording
plays forward in real time rather than stalling); once finished it stays in its
final cell until the level is restarted.

## Validation shape

The task lists validity rules that need different representations to be
reachable: "not exactly one entrance/exit" needs a model that can hold zero or
many, while "two objects in one cell" needs independent lists that can collide.
Both are unified through a `LevelCensus` (a multiplicity-capable inventory):

- a `LevelDefinition` produces a census with exactly one entrance/exit (single
  fields), so validating a parsed level exercises the bounds/group/overlap/blocked
  rules;
- the editor produces a census straight from its paint grid, where zero or many
  entrances/exits are reachable.

One `validate_census` function and one `LevelValidationReport` serve both.

## What this app deliberately does **not** do

- No new engine layer or module; no generic "puzzle framework".
- No `utils`/`helpers`/`common`/`misc`.
- No `println!`/`todo!`/`unimplemented!` in non-test code; no wall-clock or
  randomness in the core.
- No browser APIs outside the wasm `web` arm.
