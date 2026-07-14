//! The one behavioral facade: construction plus the deterministic path queries.

use axiom_kernel::{Meters, StableHash};
use axiom_math::Vec2;

use crate::cell::Cell;
use crate::dist::Dist;
use crate::grid::Grid;
use crate::tile_space::TileSpace;

/// The grid module's single facade. All construction and every query go through
/// it; [`Grid`], [`Cell`], [`TileSpace`], and [`Dist`] are the value types it
/// returns and consumes.
///
/// The path queries are `sim`-class — pure functions of `(grid, cells,
/// passable)` with a fixed N, E, S, W neighbor order and a lexicographic `(y, x)`
/// tie-break — so a path is *byte*-identical run-to-run and machine-to-machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridApi;

impl GridApi {
    /// A fresh facade handle (stateless — every query is pure).
    pub fn new() -> Self {
        Self
    }

    /// Create a `cols × rows` board with every cell set to `fill`.
    pub fn create<T: Copy>(cols: u32, rows: u32, fill: T) -> Grid<T> {
        Grid::new(cols, rows, fill)
    }

    /// A tile↔world mapping with the given world `origin` and dimensioned
    /// `cell_size`.
    pub fn tile_space(origin: Vec2, cell_size: Meters) -> TileSpace {
        TileSpace::new(origin, cell_size)
    }

    /// The BFS distance from `start` to every passable cell, as a `Grid<Dist>`
    /// (unreachable cells read [`Dist::UNREACHABLE`], projected `Infinity`).
    ///
    /// Computed by **bounded wavefront relaxation**: the field starts all
    /// `UNREACHABLE` with `start` seeded to zero (only if `start` is in bounds
    /// and passable), then `cols * rows` relaxation passes — more than the
    /// longest possible shortest path, so the field is fully converged — each a
    /// branchless `for_each` that relaxes every cell to `min(self, 1 + min
    /// passable-in-bounds neighbor)`. No queue, no `while`, no neighbor `if`:
    /// out-of-bounds neighbors read `UNREACHABLE` through [`Grid::get`]'s default
    /// and impassable cells are pinned `UNREACHABLE`.
    pub fn distance_field<T, F>(&self, g: &Grid<T>, start: Cell, passable: F) -> Grid<Dist>
    where
        T: Copy,
        F: Fn(T) -> bool,
    {
        let mut field = Grid::new(g.cols(), g.rows(), Dist::UNREACHABLE);
        let start_ok = g.in_bounds(start.x, start.y) & passable(g.get(start.x, start.y));
        start_ok.then(|| field.set(start.x, start.y, Dist::ZERO));
        let passes = (g.cols() as usize) * (g.rows() as usize);
        (0..passes).for_each(|_| Self::relax_pass(&mut field, g, &passable));
        field
    }

    /// One relaxation pass over every cell, in row-major order. A passable cell
    /// takes `min(current, 1 + min neighbor)`; an impassable cell is pinned
    /// `UNREACHABLE`. In-place, so a cell already relaxed earlier in the same
    /// pass propagates forward immediately (only speeds convergence; the bounded
    /// pass count guarantees the final result regardless of order).
    fn relax_pass<T, F>(field: &mut Grid<Dist>, g: &Grid<T>, passable: &F)
    where
        T: Copy,
        F: Fn(T) -> bool,
    {
        let cols = field.cols();
        let rows = field.rows();
        (0..rows).for_each(|y| {
            (0..cols).for_each(|x| {
                let (xi, yi) = (x as i32, y as i32);
                let here_passable = passable(g.get(xi, yi));
                let min_neighbor = field
                    .get(xi, yi - 1)
                    .min(field.get(xi + 1, yi))
                    .min(field.get(xi, yi + 1))
                    .min(field.get(xi - 1, yi));
                let relaxed = field.get(xi, yi).min(min_neighbor.plus_one());
                let next = here_passable
                    .then_some(relaxed)
                    .unwrap_or(Dist::UNREACHABLE);
                field.set(xi, yi, next);
            });
        });
    }

    /// The canonical shortest path from `start` to `goal` over passable cells, or
    /// `None` if `goal` is unreachable (including when `start` or `goal` sits on
    /// an impassable cell). `start == goal` yields the single-cell path
    /// `[start]`.
    ///
    /// Reconstructed by gradient descent down a distance field computed **from
    /// the goal**: from `start`, step each move to the neighbor closest to the
    /// goal (smallest `Dist`, lexicographic `(y, x)` tie-break). The descent
    /// strictly decreases the distance every step, so it reaches the goal in
    /// finitely many steps over a fully-converged field.
    pub fn path<T, F>(&self, g: &Grid<T>, start: Cell, goal: Cell, passable: F) -> Option<Vec<Cell>>
    where
        T: Copy,
        F: Fn(T) -> bool,
    {
        let to_goal = self.distance_field(g, goal, passable);
        to_goal
            .get(start.x, start.y)
            .is_reachable()
            .then(|| Self::descend(&to_goal, start, goal))
    }

    /// Whether a passable path exists from `start` to `goal`.
    pub fn reachable<T, F>(&self, g: &Grid<T>, start: Cell, goal: Cell, passable: F) -> bool
    where
        T: Copy,
        F: Fn(T) -> bool,
    {
        self.distance_field(g, start, passable)
            .get(goal.x, goal.y)
            .is_reachable()
    }

    /// One greedy best-first step from `from` toward `target`: the in-bounds,
    /// passable neighbor minimizing the integer squared Euclidean distance to
    /// `target` (lexicographic `(y, x)` tie-break). With no passable neighbor,
    /// stays put (returns `from`) — a real contract outcome, not an error.
    pub fn step_toward<T, F>(&self, g: &Grid<T>, from: Cell, target: Cell, passable: F) -> Cell
    where
        T: Copy,
        F: Fn(T) -> bool,
    {
        Self::neighbors(from)
            .into_iter()
            .filter(|c| g.in_bounds(c.x, c.y) & passable(g.get(c.x, c.y)))
            .min_by_key(|c| (Self::sq_distance(*c, target), *c))
            .unwrap_or(from)
    }

    /// The deterministic [`StableHash`] of a `u32` board's canonical bytes — the
    /// per-tick board state for the §17.4 replay-hash sequence.
    pub fn state_hash(&self, g: &Grid<u32>) -> StableHash {
        StableHash::of_bytes(&g.canonical_bytes())
    }

    /// The four neighbors of `c` in the canonical N, E, S, W order.
    fn neighbors(c: Cell) -> [Cell; 4] {
        [
            Cell::new(c.x, c.y - 1),
            Cell::new(c.x + 1, c.y),
            Cell::new(c.x, c.y + 1),
            Cell::new(c.x - 1, c.y),
        ]
    }

    /// Walk `start → goal` down `to_goal` (distance-to-goal), each step to the
    /// closest neighbor; collect the cells passed through, inclusive of both
    /// ends.
    fn descend(to_goal: &Grid<Dist>, start: Cell, goal: Cell) -> Vec<Cell> {
        std::iter::successors(Some(start), |&c| {
            (c != goal).then(|| Self::closest_neighbor(to_goal, c))
        })
        .collect()
    }

    /// The neighbor of `c` closest to the goal under `to_goal`: smallest `Dist`,
    /// then smallest [`Cell`] (lexicographic `(y, x)`). A total `fold` over the
    /// fixed four-neighbor array — no empty-iterator fallback to leave uncovered.
    fn closest_neighbor(to_goal: &Grid<Dist>, c: Cell) -> Cell {
        let [n, e, s, w] = Self::neighbors(c);
        let key = |cell: Cell| (to_goal.get(cell.x, cell.y), cell);
        [e, s, w].into_iter().fold(n, |best, cand| {
            (key(cand) < key(best)).then_some(cand).unwrap_or(best)
        })
    }

    /// Integer squared Euclidean distance between two cells — deterministic, with
    /// no float rounding to disagree across machines.
    fn sq_distance(a: Cell, b: Cell) -> i64 {
        let dx = (a.x - b.x) as i64;
        let dy = (a.y - b.y) as i64;
        dx * dx + dy * dy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Passable iff the cell holds 0; any non-zero value is a wall.
    fn open(v: u32) -> bool {
        v == 0
    }

    /// Build a grid from row-major rows (top row first), `1` = wall, `0` = floor.
    fn grid_from(rows: &[&[u32]]) -> Grid<u32> {
        let h = rows.len() as u32;
        let w = rows.first().map_or(0, |r| r.len()) as u32;
        let mut g = GridApi::create(w, h, 0u32);
        rows.iter().enumerate().for_each(|(y, row)| {
            row.iter()
                .enumerate()
                .for_each(|(x, &v)| g.set(x as i32, y as i32, v));
        });
        g
    }

    #[test]
    fn create_and_new_make_a_board_and_a_handle() {
        let g = GridApi::create(4, 3, 9u32);
        assert_eq!((g.cols(), g.rows()), (4, 3));
        assert_eq!(g.get(0, 0), 9);
        // The facade is a stateless value.
        assert_eq!(GridApi::new(), GridApi);
    }

    #[test]
    fn tile_space_maps_through_the_facade() {
        let ts = GridApi::tile_space(Vec2::ZERO, Meters::new(2.0).expect("positive"));
        assert_eq!(ts.tile_to_world(1, 1), Vec2::new(3.0, 3.0));
    }

    #[test]
    fn distance_field_on_an_open_board_is_manhattan_from_start() {
        let api = GridApi::new();
        let g = grid_from(&[&[0, 0, 0], &[0, 0, 0], &[0, 0, 0]]);
        let field = api.distance_field(&g, Cell::new(0, 0), open);
        assert_eq!(field.get(0, 0).steps(), Some(0));
        assert_eq!(field.get(1, 0).steps(), Some(1));
        assert_eq!(field.get(2, 2).steps(), Some(4));
    }

    #[test]
    fn path_on_an_open_board_is_the_canonical_route_and_reproduces() {
        let api = GridApi::new();
        let g = grid_from(&[&[0, 0, 0], &[0, 0, 0], &[0, 0, 0]]);
        let golden = vec![
            Cell::new(0, 0),
            Cell::new(1, 0),
            Cell::new(2, 0),
            Cell::new(2, 1),
            Cell::new(2, 2),
        ];
        let path = api.path(&g, Cell::new(0, 0), Cell::new(2, 2), open);
        assert_eq!(path.as_deref(), Some(golden.as_slice()));
        // Deterministic: a second run is byte-identical.
        assert_eq!(api.path(&g, Cell::new(0, 0), Cell::new(2, 2), open), path);
        // The lexicographic tie-break picks THIS route, not the mirror-image
        // (down the left edge then across) of equal length.
        let mirror = vec![
            Cell::new(0, 0),
            Cell::new(0, 1),
            Cell::new(0, 2),
            Cell::new(1, 2),
            Cell::new(2, 2),
        ];
        assert_ne!(path, Some(mirror));
    }

    #[test]
    fn path_detours_around_a_wall_and_reports_reachable() {
        // A wall column splits the board; the only way across is along the
        // bottom row. Exercises the impassable arm of the relaxation.
        let api = GridApi::new();
        let g = grid_from(&[&[0, 1, 0], &[0, 1, 0], &[0, 0, 0]]);
        assert!(api.reachable(&g, Cell::new(0, 0), Cell::new(2, 0), open));
        let path = api
            .path(&g, Cell::new(0, 0), Cell::new(2, 0), open)
            .expect("a detour exists");
        // It starts at the start, ends at the goal, and never steps on a wall.
        assert_eq!(path.first(), Some(&Cell::new(0, 0)));
        assert_eq!(path.last(), Some(&Cell::new(2, 0)));
        assert!(path.iter().all(|c| g.get(c.x, c.y) == 0));
        // The detour is longer than the blocked straight line (2 steps).
        assert!(path.len() > 3);
    }

    #[test]
    fn an_enclosed_goal_is_unreachable() {
        // The goal cell is walled off on every passable side.
        let api = GridApi::new();
        let g = grid_from(&[&[0, 1, 1], &[1, 1, 0], &[1, 0, 0]]);
        let goal = Cell::new(0, 0);
        let start = Cell::new(2, 2);
        assert!(!api.reachable(&g, start, goal, open));
        assert_eq!(api.path(&g, start, goal, open), None);
        // The distance field reads `Infinity` (unreachable) at the enclosed cell.
        let field = api.distance_field(&g, start, open);
        assert!(!field.get(goal.x, goal.y).is_reachable());
    }

    #[test]
    fn start_equals_goal_is_a_single_cell_path() {
        let api = GridApi::new();
        let g = grid_from(&[&[0, 0], &[0, 0]]);
        assert_eq!(
            api.path(&g, Cell::new(1, 1), Cell::new(1, 1), open),
            Some(vec![Cell::new(1, 1)])
        );
    }

    #[test]
    fn a_start_on_an_impassable_cell_has_no_path() {
        let api = GridApi::new();
        let g = grid_from(&[&[1, 0], &[0, 0]]); // (0,0) is a wall
        assert_eq!(api.path(&g, Cell::new(0, 0), Cell::new(1, 1), open), None);
    }

    #[test]
    fn step_toward_takes_the_closest_neighbor_and_stays_put_when_boxed_in() {
        let api = GridApi::new();
        let g = grid_from(&[&[0, 0, 0], &[0, 0, 0], &[0, 0, 0]]);
        // From the corner toward the far corner, East (1,0) and South (0,1) are
        // both squared-distance 5 from the target. The lexicographic `(y, x)`
        // tie-break compares cells (1,0)→(y0,x1) and (0,1)→(y1,x0): row 0 < row
        // 1, so East wins.
        let next = api.step_toward(&g, Cell::new(0, 0), Cell::new(2, 2), open);
        assert_eq!(next, Cell::new(1, 0));
        // A cell whose every neighbor is a wall stays put.
        let boxed = grid_from(&[&[1, 1, 1], &[1, 0, 1], &[1, 1, 1]]);
        let center = Cell::new(1, 1);
        assert_eq!(
            api.step_toward(&boxed, center, Cell::new(2, 2), open),
            center
        );
    }

    #[test]
    fn state_hash_is_deterministic_and_content_sensitive() {
        let api = GridApi::new();
        let a = grid_from(&[&[0, 1], &[1, 0]]);
        let b = grid_from(&[&[0, 1], &[1, 0]]);
        let c = grid_from(&[&[0, 0], &[1, 0]]);
        assert_eq!(api.state_hash(&a), api.state_hash(&b));
        assert_ne!(api.state_hash(&a), api.state_hash(&c));
    }
}
