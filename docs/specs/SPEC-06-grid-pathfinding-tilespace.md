# SPEC-06 — Grid, pathfinding, tile space

> Status: Landed
> Landed (2026-06-28): new module `axiom-grid` (`GridApi` + `Grid`/`Cell`/`TileSpace`/`Dist`); `@axiom/game` projects `createGrid`/`tileSpace` and the native `gridPath`/`gridReachable`/`gridDistanceField`/`stepToward` (the BFS/wavefront runs native-side; the projection forwards a passability mask). The §2 greenfield is now built.
> Contract: §6–§7   Vocabulary: Grid/tilemap, Tile↔pixel + center-snap, BFS pathfinding, Agent steering / best-first step, Circle overlap (have)   Determinism: sim

## 1. Summary

A first-class integer **grid** plus the **pathfinding** and **steering** that read
it. This is the substrate every tile, board, and procedural-level game stands on:
`Grid<T>` (the board), `TileSpace` (tile↔world mapping with snap-to-cell), and the
authoritative path queries `gridPath` / `gridReachable` / `gridDistanceField` /
`stepToward`. It is **`sim`-class**: gameplay reads paths and distances, so they
must be byte-reproducible across runs and machines (contract §17.4, §17.6).

Of the 11 mapped games, the grid/board/tile titles (n=7 for the grid vocabulary,
n=3 for BFS) demand it directly; nothing in the tree provides any of it today.

## 2. Current state (verified)

Greenfield. Nothing in this subsystem exists.

- **No `Grid<T>` anywhere.** No integer 2D container, no `inBounds`/`idx`/`fill`,
  no tile↔world mapping. The contract's §6 surface is absent.
- **No pathfinding of any kind.** No BFS, A*, Dijkstra, flood-fill, or distance
  field in any crate or module. `modules/axiom-agent` **bans** pathfinding and
  navmesh by design, mechanically (`modules/axiom-agent/tests/architecture.rs`
  rejects the symbols `navmesh`/`pathfind`); it is decision-only and must not
  absorb this.
- **`crates/axiom-space` is content *addressing*, not grid geometry.** An
  `Address` is a hierarchical `u64` key-path digested through the kernel's
  `StableHash` — domain-free, "owns no geometry (that is `math`)". It names *what
  site*, it does not lay out a 2D board. Not reusable here.
- **`modules/axiom-placement` is integer scatter, not a grid.** It emits a list of
  integer positions an app places content at; it has no cell container, no
  bounds query, no traversal.
- **TS surface absent.** `packages/axiom-client` is a netcode client; no
  `createGrid`, `tileSpace`, or `gridPath` projection exists.

## 3. Architectural placement

**New engine module `modules/axiom-grid` (`GridApi` facade).**

```toml
[module]
name = "grid"
crate_name = "axiom-grid"
kind = "engine-module"
allowed_layers = ["kernel", "math"]
allowed_modules = []          # isolated — composes no module
introduced_capabilities = ["integer-grid", "tile-space", "grid-pathfinding", "grid-steering"]
```

**Why a module, not a layer or kernel addition.** A grid is a *gameplay
substrate*, not an always-true engine primitive — it is exciting, domain-shaped,
and only some games need it, so it fails the kernel bar (CLAUDE.md: "if something
is exciting, it probably does not belong in the kernel"). It is not a layer
either: no lower layer genuinely *uses* a grid (unlike `space`, which sibling
generators all key by), so wedging it into the spine would manufacture a
ceremonial edge. It is an **isolated capability with one facade** — the exact
shape of an engine module (`allowed_modules = []`). Pathfinding belongs here, not
in `axiom-agent`: this module owns the *map query* (given a grid, find a path);
`agent` owns *decisions* and is forbidden the navmesh/pathfinder noun.

**Why these two deps, both genuine.**

- **`math`** — `TileSpace::tile_to_world` returns a `Vec2` (cell center) and
  `world_to_tile` consumes one; the tile↔world mapping is math geometry.
- **`kernel`** — `cell_size` is a dimensioned `Meters` (public engine APIs forbid
  naked `f32`), and a `Grid`'s deterministic state hash is the kernel's
  `StableHash` over its canonical cell bytes (the §17.4 obligation).

**Facade shape (Module Law #8).** The behavioral facade is `GridApi`. `Grid<T>`,
`Cell`, and `TileSpace` are the pure value-type vocabulary it hands back and
takes in — the nouns the facade traffics, the same carve-out that lets `ecs`
export `EntityHandle` and a module export its `ids`. They carry data, not engine
state. (If the checker treats a *generic* `Grid<T>` as more than ids vocabulary,
that is the §9 facade-surface question, not a reason to leak a second facade.)

## 4. API surface

### 4.1 Native (`axiom-grid`, sim-class)

```rust
// One facade. All construction and all queries go through it; Grid/Cell/TileSpace
// are the value types it returns.
impl GridApi {
    pub fn create<T: Copy>(cols: u32, rows: u32, fill: T) -> Grid<T>;
    pub fn tile_space(origin: Vec2, cell_size: Meters) -> TileSpace;

    // §7 — passable is a pure predicate over the cell value.
    pub fn distance_field<T: Copy>(&self, g: &Grid<T>, start: Cell,
                                   passable: impl Fn(T) -> bool) -> Grid<Dist>;
    pub fn path<T: Copy>(&self, g: &Grid<T>, start: Cell, goal: Cell,
                         passable: impl Fn(T) -> bool) -> Option<Vec<Cell>>;
    pub fn reachable<T: Copy>(&self, g: &Grid<T>, start: Cell, goal: Cell,
                              passable: impl Fn(T) -> bool) -> bool;
    pub fn step_toward<T: Copy>(&self, g: &Grid<T>, from: Cell, target: Cell,
                                passable: impl Fn(T) -> bool) -> Cell;
}

impl<T: Copy> Grid<T> {
    pub fn cols(&self) -> u32; pub fn rows(&self) -> u32;
    pub fn get(&self, x: i32, y: i32) -> T;          // OOB -> the grid's default cell
    pub fn set(&mut self, x: i32, y: i32, value: T);
    pub fn in_bounds(&self, x: i32, y: i32) -> bool;
    pub fn idx(&self, x: u32, y: u32) -> usize;      // row-major flat index
    pub fn fill(&mut self, value: T);
    pub fn for_each(&self, f: impl FnMut(T, u32, u32));
}
```

`Dist` is a distance newtype with an explicit `unreachable` sentinel (projected as
`Infinity`); never a raw float. The out-of-bounds `get` returning the **default
cell** is a contract guarantee, not an error path — it keeps neighbor reads
branchless (no bounds `if`).

### 4.2 TS authoring projection (contract §6–§7, verbatim shapes)

```ts
interface Grid<T = number> {
  readonly cols: number; readonly rows: number;
  get(x: number, y: number): T;            // out-of-bounds returns the grid's default cell
  set(x: number, y: number, value: T): void;
  inBounds(x: number, y: number): boolean;
  idx(x: number, y: number): number;       // row-major flat index
  fill(value: T): void;
  clone(): Grid<T>;
  forEach(cb: (value: T, x: number, y: number) => void): void;
}
function createGrid<T>(cols: number, rows: number, fill: T): Grid<T>;

interface TileSpace {
  tileToWorld(x: number, y: number): Vec2;     // center of the cell
  worldToTile(p: Vec2): { x: number; y: number };
  snapToCell(p: Vec2): Vec2;                    // nearest cell center
}
function tileSpace(origin: Vec2, cellSize: number): TileSpace;

type Cell = { x: number; y: number };
// start/goal bundled into one `CellPair` so the queries stay within the SDK's 3-param lint cap.
type CellPair = { start: Cell; goal: Cell };
function gridPath(grid: Grid, ends: CellPair, passable: (v: number) => boolean): Cell[] | null;
function gridReachable(grid: Grid, ends: CellPair, passable: (v: number) => boolean): boolean;
function gridDistanceField(grid: Grid, start: Cell, passable: (v: number) => boolean): Grid<number>;
function stepToward(grid: Grid, ends: CellPair, passable: (v: number) => boolean): Cell;
```

`gridPath` returns `null` (not throw) when unreachable; the distance field reads
`Infinity` at unreachable cells.

## 5. Data contracts

Neutral value types crossing the boundary:

- **`Grid<T>`** — `{ cols, rows, default, cells: row-major Vec<T> }`. Pure data;
  serializes to canonical bytes for its state hash.
- **`Cell`** — `{ x, y }` integer coordinate newtype.
- **`TileSpace`** — `{ origin: Vec2, cell_size: Meters }`; methods are pure
  functions of those two fields (no stored grid, no engine state).
- **`Dist`** — finite step count or `unreachable` sentinel; the distance-field
  cell type, projected as `number | Infinity`.

## 6. Determinism

`sim`-class; binds the §17 cross-cutting law.

- **Single clock / no randomness.** Path queries are pure functions of `(grid,
  start, goal, passable)` — no tick, no RNG, no wall-clock reaches them.
- **Stable neighbor order.** 4-connectivity is visited in a **fixed canonical
  order** (e.g. N, E, S, W) at every cell, so the wavefront expands identically
  every run.
- **Stable tie-breaking.** When two cells share a distance (path reconstruction)
  or two neighbors share a `stepToward` cost, the tie breaks on a **total order**
  of `Cell` (lexicographic `(y, x)`), never on iteration-order accident. This is
  what makes a path *byte*-identical, not merely *a* shortest path (§17.4).
- **Integer board, deterministic cost.** Coordinates and distances are integers;
  `stepToward`'s Euclidean cost is compared via a deterministic squared-distance
  integer form, so no float rounding selects a different step across machines
  (§17.6).
- **State hash.** `GridApi` exposes the grid's `StableHash` so a replay can pin
  the per-tick board state in the §17.4 hash sequence.

## 7. Acceptance / proof

- **Branchless.** The whole module is non-test spine code: zero `if`/`match`/
  `for`/`while`/`?`. The hard part (§9) is BFS without a `while` queue and
  without `if` neighbor guards. Expressed as **bounded wavefront relaxation**:
  the distance field is computed by a fixed number of relaxation passes over the
  flat cell array, each pass an iterator `fold`/`map` that sets every cell to
  `min(self, 1 + min(distance of its passable, in-bounds neighbors))`. Bounds and
  passability collapse the neighbor `if`s into `Grid::get`'s default-cell return
  plus `passable(..).then_some(..)` option combinators and saturating arithmetic;
  the queue's `while` becomes a bounded pass count (≤ reachable diameter, capped
  at `cols*rows`). `path` is gradient descent down the converged field via a
  bounded `stepToward` fold; `reachable` is "goal cell is finite"; `step_toward`
  is an iterator `min_by` over the four neighbors with the lexicographic tie-break.
- **100% coverage**, including every edge the shape forces:
  out-of-bounds `get` → default cell; `start == goal` (empty/one-cell path);
  fully-blocked grid (`null` / all-`Infinity`); `start`/`goal` on an impassable
  cell; a `stepToward` with no passable neighbor (stays put). No dead arm added
  to hit a number — each is a real contract outcome.
- **Replay / golden.** A fixed grid + obstacle layout yields a **golden path
  byte-sequence** and a **golden distance-field hash**; the test asserts they
  reproduce exactly on a second run (and that an alternate equal-length route is
  *not* chosen, proving the tie-break). `tileToWorld(worldToTile(p))` round-trips
  to the cell center; `snapToCell` is idempotent.
- **TS projection** held to the SDK laws (tsgo, Oxlint branch ban, 100%
  coverage); a TS path on a known maze matches the native golden.

## 8. Dependencies & order

- **Depends on:** kernel + math only (both already landed). No dependency on
  SPEC-01..05.
- **Lands after** SPEC-00 (the boundary it projects through) and the math/scalar
  helpers of SPEC-03 (`Vec2`, clamp); otherwise independent — buildable in
  parallel with input/timers.
- **Depended on by:** tile/board/procedural-level apps, and any agent that needs
  to *follow* a path (the agent decides; this module computes the route it
  follows). Contract build-order slot is §18.6.

## 9. Open questions

- **Branchless BFS is the central risk.** A queue-based BFS is `O(cells)`; the
  branchless **fixed-iteration relaxation** above is up to `O(cells × diameter)`
  worst case. Open: (a) is the pass-count bound `cols*rows`, or can a tighter
  *early-converged* bound be expressed without a data-dependent `while`? (b) does
  a single fold that relaxes in scan order (carrying partial results within a
  pass) cut the pass count enough for large boards, or is a true wavefront
  required? (c) at what grid size does the constant factor force a different
  branchless structure (e.g. a priority bucket array)? Resolve before
  implementation — this decides the data shape, per No-Shortcuts.
- **`Grid<T>` genericity vs Module Law #8.** Authors want `T` arbitrary (the
  contract default is `number`). Confirm the checker accepts a *generic* value
  type as facade vocabulary alongside `GridApi`, and decide how a generic `T`
  marshals across the wasm boundary (likely: native `Grid<u32>` cells + an app-
  side `T`↔`u32` table, the same handle-table pattern SPEC-00 uses).
- **`Infinity` representation across the boundary.** `Dist`'s sentinel must
  project to JS `Infinity` without a branch on the TS side and without a magic
  number leaking into sim comparisons. Fix the sentinel encoding once, here.
- **Diagonal / weighted movement.** The contract fixes 4-connectivity and unit
  cost. 8-connectivity and weighted terrain are out of scope for this spec; note
  them so a later amendment extends `passable` to a cost function deliberately,
  not by accident.
