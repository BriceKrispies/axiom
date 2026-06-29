/*
 * The grid / tile-space / pathfinding projection (SPEC-06 §4.2). `Grid<Value>` is a
 * pure value container (`{ cols, rows, default, cells }`, SPEC-06 §5) the author
 * reads and writes; `TileSpace` is the tile↔world mapping. The authoritative
 * queries — `gridPath` / `gridReachable` / `gridDistanceField` / `stepToward` —
 * route to the native `axiom-grid` core: the projection evaluates the author's
 * `passable` predicate over the grid's cells client-side, hands the core a
 * `GridField` (dimensions + a row-major boolean passability mask), and the core
 * returns the cell sequence / distance field (the deterministic BFS / wavefront is
 * native, never re-derived in TS — the same "one source of truth" rule the RNG
 * draw sequence follows).
 *
 * The two endpoint cells a query needs are bundled into one `{ start, goal }` /
 * `{ from, target }` record so each query stays within the SDK's ≤3-parameter law
 * (the same record-argument shape `createMaterial` uses); `passable` stays a
 * separate trailing predicate, matching `gridDistanceField`'s shape.
 *
 * `TileSpace`'s tile↔world arithmetic is a bit-trivial affine map of `(origin,
 * cellSize)` with no native state to consult, so — like `lerp` in `math.ts` — it
 * stays local rather than paying a bridge crossing; `Math.floor` makes
 * `worldToTile` deterministic.
 *
 * Branchlessness: `get`'s out-of-bounds → default-cell return is a table select
 * (`pick` of the in-array index vs. a guaranteed-absent index, then `orElse` to the
 * default); `inBounds` AND-combines its four comparisons by multiplication (`&&`
 * and bitwise `&` are both banned); `set` guards the write with a `filter` over the
 * in-bounds singleton — never an `if` bounds guard.
 */

import type { Cell, Result, Vec2 } from "./vocabulary.ts";
import { each, orElse, pick } from "./branchless.ts";
import type { GridField } from "./host-descriptors.ts";
import { boundHost } from "./host-binding.ts";

/** Half a cell — the offset from a cell's corner to its centre (SPEC-06 §4.2). */
const CELL_CENTER_OFFSET = 0.5;

/** The product that means "every comparison held" in the branchless `inBounds` AND. */
const ALL_TRUE = 1;

/** A pair of endpoint cells a path query runs between (SPEC-06 §4.2). */
export interface CellPair {
  /** The first endpoint (path `start` / step `from`). */
  readonly start: Cell;
  /** The second endpoint (path `goal` / step `target`). */
  readonly goal: Cell;
}

/** The integer grid container (SPEC-06 §4.2) — pure data, queried natively. */
export interface Grid<Value = number> {
  /** The column count (width in cells). */
  readonly cols: number;
  /** The row count (height in cells). */
  readonly rows: number;
  /** Read cell `(x, y)`; out-of-bounds returns the grid's default cell. */
  readonly get: (x: number, y: number) => Value;
  /** Write cell `(x, y)` (an out-of-bounds write is a clean no-op). */
  readonly set: (x: number, y: number, value: Value) => void;
  /** Whether `(x, y)` is inside the grid. */
  readonly inBounds: (x: number, y: number) => boolean;
  /** The row-major flat index of `(x, y)`. */
  readonly idx: (x: number, y: number) => number;
  /** Overwrite every cell with `value`. */
  readonly fill: (value: Value) => void;
  /** A deep copy of the grid. */
  readonly clone: () => Grid<Value>;
  /** Visit every cell in row-major order with its value and coordinates. */
  readonly forEach: (callback: (value: Value, x: number, y: number) => void) => void;
}

/** The construction inputs for a {@link BridgeGrid} — bundled so the constructor stays ≤3 params. */
export interface GridInit<Value> {
  /** The column count. */
  readonly cols: number;
  /** The row count. */
  readonly rows: number;
  /** The cell returned out of bounds. */
  readonly defaultCell: Value;
  /** The row-major backing cells. */
  readonly cells: Value[];
}

/** The concrete `Grid<Value>` — a flat row-major cell array plus a default cell. */
export class BridgeGrid<Value> implements Grid<Value> {
  readonly #cols: number;
  readonly #rows: number;
  readonly #default: Value;
  readonly #cells: Value[];

  public constructor(init: GridInit<Value>) {
    this.#cols = init.cols;
    this.#rows = init.rows;
    this.#default = init.defaultCell;
    this.#cells = init.cells;
  }

  public get cols(): number {
    return this.#cols;
  }

  public get rows(): number {
    return this.#rows;
  }

  public inBounds(x: number, y: number): boolean {
    return (
      Number(x >= 0) * Number(x < this.#cols) * Number(y >= 0) * Number(y < this.#rows) === ALL_TRUE
    );
  }

  public idx(x: number, y: number): number {
    return y * this.#cols + x;
  }

  public get(x: number, y: number): Value {
    // In bounds → the real flat index; out of bounds → an index past the array end
    // (`#cells.length`, always absent) so `orElse` falls through to the default.
    const flat = pick([this.#cells.length, this.idx(x, y)], Number(this.inBounds(x, y)));
    return orElse(this.#cells[flat], this.#default);
  }

  public set(x: number, y: number, value: Value): void {
    each([this.idx(x, y)].filter((): boolean => this.inBounds(x, y)), (flat): void => {
      this.#cells[flat] = value;
    });
  }

  public fill(value: Value): void {
    this.#cells.fill(value);
  }

  public clone(): Grid<Value> {
    return new BridgeGrid({
      cells: [...this.#cells],
      cols: this.#cols,
      defaultCell: this.#default,
      rows: this.#rows,
    });
  }

  public forEach(callback: (value: Value, x: number, y: number) => void): void {
    each(
      this.#cells.map((value, flat): readonly [Value, number, number] => [
        value,
        flat % this.#cols,
        Math.floor(flat / this.#cols),
      ]),
      ([value, x, y]): void => {
        callback(value, x, y);
      },
    );
  }
}

/** Create a `cols × rows` grid with every cell set to `fill` (SPEC-06 §4.2). */
export const createGrid = <Value>(cols: number, rows: number, fill: Value): Grid<Value> =>
  new BridgeGrid({
    cells: Array.from({ length: cols * rows }, (): Value => fill),
    cols,
    defaultCell: fill,
    rows,
  });

/** The tile↔world mapping (SPEC-06 §4.2) — pure functions of `(origin, cellSize)`. */
export interface TileSpace {
  /** The world-space centre of cell `(x, y)`. */
  readonly tileToWorld: (x: number, y: number) => Vec2;
  /** The cell containing world point `point`. */
  readonly worldToTile: (point: Vec2) => Cell;
  /** Snap world point `point` to the nearest cell centre. */
  readonly snapToCell: (point: Vec2) => Vec2;
}

/** Build a `TileSpace` mapping with cell origin `origin` and square cells of `cellSize`. */
export const tileSpace = (origin: Vec2, cellSize: number): TileSpace => {
  const tileToWorld = (x: number, y: number): Vec2 => ({
    x: origin.x + (x + CELL_CENTER_OFFSET) * cellSize,
    y: origin.y + (y + CELL_CENTER_OFFSET) * cellSize,
  });
  const worldToTile = (point: Vec2): Cell => ({
    x: Math.floor((point.x - origin.x) / cellSize),
    y: Math.floor((point.y - origin.y) / cellSize),
  });
  return {
    snapToCell: (point: Vec2): Vec2 => {
      const cell = worldToTile(point);
      return tileToWorld(cell.x, cell.y);
    },
    tileToWorld,
    worldToTile,
  };
};

/** The `GridField` (dims + passability mask) the native core queries, for the given `passable`. */
const toField = (grid: Grid, passable: (value: number) => boolean): GridField => ({
  cols: grid.cols,
  passable: Array.from({ length: grid.cols * grid.rows }, (_unused, flat): boolean =>
    passable(grid.get(flat % grid.cols, Math.floor(flat / grid.cols))),
  ),
  rows: grid.rows,
});

/** The shortest path between the endpoints, or the empty value when unreachable (SPEC-06 §4.2). */
export const gridPath = (
  grid: Grid,
  ends: CellPair,
  passable: (value: number) => boolean,
): Result<readonly Cell[]> => boundHost().gridPath(toField(grid, passable), ends.start, ends.goal);

/** Whether the `goal` endpoint is reachable from the `start` endpoint (SPEC-06 §4.2). */
export const gridReachable = (
  grid: Grid,
  ends: CellPair,
  passable: (value: number) => boolean,
): boolean => boundHost().gridReachable(toField(grid, passable), ends.start, ends.goal);

/** The BFS distance field from `start` as a `Grid<number>` (`Infinity` unreachable, SPEC-06 §4.2). */
export const gridDistanceField = (
  grid: Grid,
  start: Cell,
  passable: (value: number) => boolean,
): Grid =>
  new BridgeGrid({
    cells: [...boundHost().gridDistanceField(toField(grid, passable), start)],
    cols: grid.cols,
    defaultCell: Number.POSITIVE_INFINITY,
    rows: grid.rows,
  });

/** The best next cell stepping `from` toward `target` (stays put if blocked, SPEC-06 §4.2). */
export const stepToward = (
  grid: Grid,
  ends: CellPair,
  passable: (value: number) => boolean,
): Cell => boundHost().gridStepToward(toField(grid, passable), ends.start, ends.goal);
