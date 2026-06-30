# Quintet — architecture

Quintet is an Axiom **app**: a composition leaf. Apps are exempt from the
branchless and 100%-coverage spine gates, so all of Quintet's gameplay lives
here and is never pushed down into a layer or module.

## Placement classification

| Code | Placement |
|------|-----------|
| board / piece / placement / clearing / scoring / generation | **App** (pure game core, `src/*.rs`) |
| canvas drawing + pointer drag-and-drop | **App** (`src/web.rs`, wasm32-only) |
| deterministic random source | **Layer: kernel** (`axiom_kernel::DeterministicRng`) |

Nothing in Quintet belongs in a layer or module: a 10×10 block-placement game is
gameplay, and gameplay never leaks inward (CLAUDE.md, "Feature Module Rules" and
"Determinism Rules"). The only engine dependency is the kernel.

## Why the kernel, and only the kernel

Generation must be **deterministic and replayable**: the next quintet is a pure
function of `(board, score, move-count)`. We seed the kernel's
`DeterministicRng` (splitmix64) from those three inputs — no wall clock, no
unseeded entropy, no hidden global state. That is the one genuine engine
dependency, declared in `app.toml`'s `allowed_layers = ["kernel"]`. The game
renders itself on a 2D `<canvas>`, so it needs none of the 3D render path;
declaring the `engine`/`windowing` modules would be a ceremonial dependency, so
they are omitted.

## Module layout (pure core → thin shell)

```text
board.rs       10×10 grid of filled/empty cells
quintet.rs     the 5-cell piece: a normalized 5×5 mask + shape validation
placement.rs   can a mask be placed at a board anchor? enumerate + commit
clearing.rs    detect full rows/cols, clear simultaneously, score
generation.rs  catalog of fixed pentominoes → deterministic, always-placeable pick
game.rs        QuintetGame: board + score + current piece; try_place / reset
web.rs         wasm32-only 2D-canvas adapter with pointer drag-and-drop
```

Every rule is testable on native (`tests/required_behaviors.rs` plus per-module
unit tests). `web.rs` makes no gameplay decisions — it reads the core and paints
it.

## Guarantees the design enforces

* **Exactly 5 connected cells.** The generator's shape pool is built by growing
  connected cell sets one orthogonal step at a time, so every offered quintet is
  a real 5-cell orthogonally-connected pentomino. Diagonal lines and
  disconnected / corner-only shapes can never be produced, and are independently
  rejected by `QuintetMask::is_valid`.
* **Always placeable, or honestly stuck.** Generation filters the pool to shapes
  that have at least one valid placement on the *current* board. If none fit, it
  returns `None`; the game reports a stuck state and keeps Reset available — it
  never offers an impossible piece.
* **Simultaneous clears, intersection counted once.** After a placement, all
  full rows and columns clear at once; a shared cell is removed and scored
  exactly once. Score = `unique_cleared_blocks * lines_cleared`.
