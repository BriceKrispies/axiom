//! The board: a row-major integer container with an out-of-bounds-safe read.

/// A row-major 2D grid of `T`, the board every tile/level query reads.
/// The out-of-bounds `get` returning the grid's **default cell** (the `fill` the
/// grid was created with) is a contract guarantee, not an error path: it keeps
/// neighbor reads branchless (no bounds `if` at every wavefront step). `set` on
/// an out-of-bounds coordinate is a silent no-op, the write-side mirror.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid<T> {
    cols: u32,
    rows: u32,
    default: T,
    cells: Vec<T>,
}

impl<T: Copy> Grid<T> {
    /// Create a `cols × rows` grid with every cell — and the out-of-bounds
    /// default — set to `fill`. Crate-internal; authored through
    /// [`GridApi::create`](crate::GridApi::create).
    pub(crate) fn new(cols: u32, rows: u32, fill: T) -> Self {
        Self {
            cols,
            rows,
            default: fill,
            cells: vec![fill; (cols as usize) * (rows as usize)],
        }
    }

    /// The number of columns.
    pub fn cols(&self) -> u32 {
        self.cols
    }

    /// The number of rows.
    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// Whether `(x, y)` is inside the board. Branchless: two range-contains
    /// combined with a bitwise `&`.
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        (0..self.cols as i32).contains(&x) & (0..self.rows as i32).contains(&y)
    }

    /// The row-major flat index of an in-bounds `(x, y)`.
    pub fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.cols + x) as usize
    }

    /// Read `(x, y)`; **out of bounds returns the grid's default cell** (never
    /// panics). The index is computed only on the in-bounds arm, so an OOB read
    /// never indexes the backing store.
    pub fn get(&self, x: i32, y: i32) -> T {
        self.in_bounds(x, y)
            .then(|| self.cells[self.idx(x as u32, y as u32)])
            .unwrap_or(self.default)
    }

    /// Write `value` at `(x, y)`; out of bounds is a silent no-op. Branchless:
    /// the index is produced only when in bounds, then drained.
    pub fn set(&mut self, x: i32, y: i32, value: T) {
        let target = self.in_bounds(x, y).then(|| self.idx(x as u32, y as u32));
        target.into_iter().for_each(|i| self.cells[i] = value);
    }

    /// Set every cell to `value` (does not change the out-of-bounds default).
    pub fn fill(&mut self, value: T) {
        self.cells.iter_mut().for_each(|c| *c = value);
    }

    /// Visit every cell in row-major order, with its `(x, y)` coordinate.
    pub fn for_each(&self, mut f: impl FnMut(T, u32, u32)) {
        let cols = self.cols;
        self.cells
            .iter()
            .copied()
            .enumerate()
            .for_each(|(i, v)| f(v, (i as u32) % cols, (i as u32) / cols));
    }
}

impl Grid<u32> {
    /// Canonical little-endian byte image of the board — header (`cols`, `rows`,
    /// `default`) then every cell in row-major order. The pre-image of the
    /// deterministic [`state_hash`](crate::GridApi::state_hash); `u32` is the
    /// marshalable cell type across the authoring boundary (SPEC-06 §9).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        [self.cols, self.rows, self.default]
            .into_iter()
            .chain(self.cells.iter().copied())
            .flat_map(u32::to_le_bytes)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_index_and_bounds() {
        let g = Grid::new(3, 2, 0u32);
        assert_eq!(g.cols(), 3);
        assert_eq!(g.rows(), 2);
        assert_eq!(g.idx(2, 1), 5); // row-major: 1*3 + 2
        assert!(g.in_bounds(0, 0));
        assert!(g.in_bounds(2, 1));
        assert!(!g.in_bounds(3, 0)); // x past the right edge
        assert!(!g.in_bounds(0, 2)); // y past the bottom edge
        assert!(!g.in_bounds(-1, 0)); // negative x
    }

    #[test]
    fn get_is_out_of_bounds_safe_returning_the_default() {
        let mut g = Grid::new(2, 2, 7u32);
        g.set(1, 0, 42);
        assert_eq!(g.get(1, 0), 42);
        assert_eq!(g.get(0, 0), 7);
        assert_eq!(g.get(-1, 0), 7);
        assert_eq!(g.get(2, 0), 7);
        assert_eq!(g.get(0, -1), 7);
        assert_eq!(g.get(0, 2), 7);
    }

    #[test]
    fn set_out_of_bounds_is_a_silent_no_op() {
        let mut g = Grid::new(2, 2, 0u32);
        g.set(5, 5, 99);
        let mut total = 0u32;
        g.for_each(|v, _, _| total += v);
        assert_eq!(total, 0);
    }

    #[test]
    fn fill_and_for_each_visit_every_cell_row_major() {
        let mut g = Grid::new(2, 2, 0u32);
        g.fill(3);
        let mut seen = Vec::new();
        g.for_each(|v, x, y| seen.push((x, y, v)));
        assert_eq!(seen, vec![(0, 0, 3), (1, 0, 3), (0, 1, 3), (1, 1, 3)]);
    }

    #[test]
    fn canonical_bytes_are_header_then_row_major_cells() {
        let mut g = Grid::new(2, 1, 0u32);
        g.set(0, 0, 1);
        g.set(1, 0, 2);
        let bytes = g.canonical_bytes();
        // cols=2, rows=1, default=0, then cells 1, 2 — all little-endian u32.
        let expected: Vec<u8> = [2u32, 1, 0, 1, 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(bytes, expected);
    }
}
