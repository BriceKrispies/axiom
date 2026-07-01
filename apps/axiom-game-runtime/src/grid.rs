//! Grid / pathfinding (SPEC-06 §4.2) composed into the bridge: the
//! `gridPath` / `gridReachable` / `gridDistanceField` / `gridStepToward` queries
//! the TS `HostBridge` grid surface projects, every one forwarding to the
//! deterministic [`axiom_grid::GridApi`] BFS / wavefront core. The board's
//! passability is the only thing that crosses; the canonical neighbour order and
//! `(y, x)` tie-break that make a path *byte*-identical run-to-run live in the
//! `axiom-grid` module, never re-derived here.
//! ## Boundary convention (the established `scalar / byte / slice` rule)
//! A query crosses as the board dimensions (`cols`, `rows`), the **row-major
//! passability mask** as bytes (`&[u8]`, `0` = blocked, non-zero = passable), and
//! the endpoint cells as scalar `(x, y)` `i32`s — exactly as physics vectors
//! cross as scalar `(x, y, z)`. The TS host edge (`wasm-host.ts`) packs the
//! contract's `GridField { cols, rows, passable: boolean[] }` + `Cell`s into these
//! and reshapes the results:
//! - `gridPath` returns a flat `Vec<f64>` `[x0, y0, x1, y1, …]` (**empty** = the
//!   goal is unreachable, which the edge maps to the empty `Result`);
//! - `gridStepToward` returns the single best next cell as `[x, y]`;
//! - `gridDistanceField` returns the row-major distances as `Vec<f64>`, with
//!   `f64::INFINITY` at every unreachable cell (the `Dist::UNREACHABLE` sentinel);
//! - `gridReachable` returns a plain `bool`.
//! The board cell type is `u8` (the mask byte itself); the passability predicate
//! the queries take is simply "the cell is non-zero", so an author's arbitrary
//! `passable` predicate is evaluated TS-side into the mask and the native core
//! only sees the resolved board.

use axiom_grid::{Cell, Grid, GridApi};

use crate::GameBridge;

/// Build a `cols × rows` `Grid<u8>` from a row-major passability mask. A short
/// mask leaves the tail cells `0` (blocked); `get`-with-default keeps the read
/// off the panic path, so a malformed length is a clean, deterministic board.
fn build_grid(cols: u32, rows: u32, mask: &[u8]) -> Grid<u8> {
    let mut grid = GridApi::create(cols, rows, 0u8);
    (0..rows).for_each(|y| {
        (0..cols).for_each(|x| {
            let flat = (y as usize) * (cols as usize) + (x as usize);
            grid.set(x as i32, y as i32, *mask.get(flat).unwrap_or(&0));
        });
    });
    grid
}

/// The passability predicate the queries run over the byte board: a cell is
/// passable iff its mask byte is non-zero.
fn passable(value: u8) -> bool {
    value != 0
}

/// A [`Cell`] from a 2-element boundary slice `[x, y]` (missing entries read `0`).
fn cell_in(c: &[i32]) -> Cell {
    Cell::new(*c.first().unwrap_or(&0), *c.get(1).unwrap_or(&0))
}

impl GameBridge {
    /// The canonical shortest path `start → goal` over passable cells
    /// (`gridPath`), flattened to `[x0, y0, x1, y1, …]`; an unreachable goal is the
    /// empty list (the edge's empty `Result`). Endpoints cross as 2-element
    /// `[x, y]` slices.
    pub fn grid_path(&self, cols: u32, rows: u32, mask: &[u8], start: &[i32], goal: &[i32]) -> Vec<f64> {
        let grid = build_grid(cols, rows, mask);
        self.grid
            .path(&grid, cell_in(start), cell_in(goal), passable)
            .map(|cells| {
                cells
                    .into_iter()
                    .flat_map(|c| [f64::from(c.x), f64::from(c.y)])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether `goal` is reachable from `start` over passable cells
    /// (`gridReachable`).
    pub fn grid_reachable(&self, cols: u32, rows: u32, mask: &[u8], start: &[i32], goal: &[i32]) -> bool {
        let grid = build_grid(cols, rows, mask);
        self.grid.reachable(&grid, cell_in(start), cell_in(goal), passable)
    }

    /// The row-major BFS distance field from `start` (`gridDistanceField`), with
    /// `f64::INFINITY` at every unreachable cell.
    pub fn grid_distance_field(&self, cols: u32, rows: u32, mask: &[u8], start: &[i32]) -> Vec<f64> {
        let grid = build_grid(cols, rows, mask);
        let field = self.grid.distance_field(&grid, cell_in(start), passable);
        let cols = cols as usize;
        (0..cols * rows as usize)
            .map(|flat| {
                let x = (flat % cols) as i32;
                let y = (flat / cols) as i32;
                field.get(x, y).steps().map_or(f64::INFINITY, f64::from)
            })
            .collect()
    }

    /// The single best next cell stepping `from` toward `target`
    /// (`gridStepToward`), as `[x, y]` (stays put — `from` — with no passable
    /// neighbour).
    pub fn grid_step_toward(&self, cols: u32, rows: u32, mask: &[u8], from: &[i32], target: &[i32]) -> Vec<f64> {
        let grid = build_grid(cols, rows, mask);
        let step = self.grid.step_toward(&grid, cell_in(from), cell_in(target), passable);
        vec![f64::from(step.x), f64::from(step.y)]
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// The canonical shortest path, flat `[x0, y0, …]` (`gridPath`); empty =
        /// unreachable. Endpoints cross as 2-element `[x, y]` slices.
        #[wasm_bindgen(js_name = gridPath)]
        pub fn grid_path(&self, cols: u32, rows: u32, mask: &[u8], start: &[i32], goal: &[i32]) -> Vec<f64> {
            self.bridge.grid_path(cols, rows, mask, start, goal)
        }

        /// Whether the goal is reachable from the start (`gridReachable`).
        #[wasm_bindgen(js_name = gridReachable)]
        pub fn grid_reachable(&self, cols: u32, rows: u32, mask: &[u8], start: &[i32], goal: &[i32]) -> bool {
            self.bridge.grid_reachable(cols, rows, mask, start, goal)
        }

        /// The row-major BFS distance field (`gridDistanceField`).
        #[wasm_bindgen(js_name = gridDistanceField)]
        pub fn grid_distance_field(&self, cols: u32, rows: u32, mask: &[u8], start: &[i32]) -> Vec<f64> {
            self.bridge.grid_distance_field(cols, rows, mask, start)
        }

        /// The best next cell `[x, y]` stepping toward a target (`gridStepToward`).
        #[wasm_bindgen(js_name = gridStepToward)]
        pub fn grid_step_toward(&self, cols: u32, rows: u32, mask: &[u8], from: &[i32], target: &[i32]) -> Vec<f64> {
            self.bridge.grid_step_toward(cols, rows, mask, from, target)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{demo_app, GameBridge};

    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    // A 3×3 board with a vertical wall in the middle column (x == 1), rows 0..2,
    // leaving the bottom row (y == 2) open as the only way around:
    //   col:  0 1 2
    //   y=0:  . # .
    //   y=1:  . # .
    //   y=2:  . . .
    // Row-major mask (1 = passable):
    const COLS: u32 = 3;
    const ROWS: u32 = 3;
    fn walled_mask() -> Vec<u8> {
        vec![
            1, 0, 1, // y=0
            1, 0, 1, // y=1
            1, 1, 1, // y=2
        ]
    }

    #[test]
    fn path_routes_around_the_wall_and_is_deterministic() {
        let b = bridge();
        // (0,0) -> (2,0): the wall forces the canonical detour down col 0, across
        // the open bottom row, and back up col 2.
        let path = b.grid_path(COLS, ROWS, &walled_mask(), &[0, 0], &[2, 0]);
        assert_eq!(
            path,
            vec![0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 1.0, 2.0, 2.0, 2.0, 2.0, 1.0, 2.0, 0.0]
        );
        // Same board + endpoints reproduce byte-identically (the BFS is pure).
        assert_eq!(path, bridge().grid_path(COLS, ROWS, &walled_mask(), &[0, 0], &[2, 0]));
    }

    #[test]
    fn distance_field_matches_the_known_wavefront() {
        let b = bridge();
        // BFS distance from (0,0). The wall cells (x==1, y<2) are UNREACHABLE.
        let field = b.grid_distance_field(COLS, ROWS, &walled_mask(), &[0, 0]);
        let inf = f64::INFINITY;
        assert_eq!(
            field,
            vec![
                0.0, inf, 6.0, // y=0
                1.0, inf, 5.0, // y=1
                2.0, 3.0, 4.0, // y=2
            ]
        );
    }

    #[test]
    fn reachable_tracks_the_wall_and_an_isolated_cell() {
        let b = bridge();
        // (0,0) can reach the far corner around the wall...
        assert!(b.grid_reachable(COLS, ROWS, &walled_mask(), &[0, 0], &[2, 0]));
        // ...but a fully-walled mask isolates everything: an interior wall cell is
        // never reachable.
        assert!(!b.grid_reachable(COLS, ROWS, &walled_mask(), &[0, 0], &[1, 0]));
    }

    #[test]
    fn step_toward_takes_the_first_canonical_step_and_stays_put_when_blocked() {
        let b = bridge();
        // From (0,0) toward (0,2): the greedy step is straight down to (0,1).
        assert_eq!(b.grid_step_toward(COLS, ROWS, &walled_mask(), &[0, 0], &[0, 2]), vec![0.0, 1.0]);
        // A cell whose only neighbours are all blocked stays put. A 1×1 all-blocked
        // board: the single cell has no passable neighbour, so it returns itself.
        assert_eq!(b.grid_step_toward(1, 1, &[0], &[0, 0], &[0, 0]), vec![0.0, 0.0]);
    }

    #[test]
    fn an_unreachable_goal_is_the_empty_path() {
        let b = bridge();
        // Goal sits on a wall cell: no path exists, so the flat list is empty.
        assert!(b.grid_path(COLS, ROWS, &walled_mask(), &[0, 0], &[1, 0]).is_empty());
    }
}
