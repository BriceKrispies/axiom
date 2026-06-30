//! The quintet piece: a 5-occupied-cell polyomino described inside a 5×5 mask.
//!
//! A [`QuintetMask`] is a *shape*, not a board placement — it carries no board
//! position. Its occupied cells are always **normalized**: shifted so the
//! shape's bounding box hugs the top-left `(0, 0)` corner, then stored sorted.
//! That gives every shape one canonical form, so equal shapes compare equal and
//! the generator preview always draws the piece in the same place.
//!
//! "Occupied" is `x`; "empty" is `o`. A *valid* quintet has exactly
//! [`QUINTET_CELLS`] occupied cells that are connected by orthogonal (edge)
//! adjacency — diagonal-only touching does not count, so a single diagonal line
//! and any corner-only or disconnected shape are rejected.

/// A quintet's bounding mask is `MASK_SIZE` × `MASK_SIZE`.
pub const MASK_SIZE: i32 = 5;

/// Every valid quintet occupies exactly this many cells.
pub const QUINTET_CELLS: usize = 5;

/// The four orthogonal steps — the *only* adjacency that counts as "connected".
pub(crate) const ORTHOGONAL: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// A quintet shape: a normalized set of occupied cells inside a 5×5 mask.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QuintetMask {
    /// Occupied cells, normalized to the top-left and stored sorted — the
    /// canonical form of the shape.
    cells: Vec<(i32, i32)>,
}

impl QuintetMask {
    /// Build a mask from arbitrary occupied cells. The shape is de-duplicated,
    /// normalized to the top-left, and sorted, so two coordinate lists that
    /// describe the same shape (any translation) yield equal masks.
    pub fn from_coords(coords: &[(i32, i32)]) -> Self {
        let mut cells = coords.to_vec();
        cells.sort_unstable();
        cells.dedup();
        normalize(&mut cells);
        QuintetMask { cells }
    }

    /// Parse a small ascii block where `x` (or `X`/`#`) marks an occupied cell
    /// and any other character is empty. Rows need not be padded. Handy for
    /// tests and for naming exact shapes.
    pub fn from_rows(rows: &[&str]) -> Self {
        let coords: Vec<(i32, i32)> = rows
            .iter()
            .enumerate()
            .flat_map(|(y, row)| {
                row.chars().enumerate().filter_map(move |(x, c)| {
                    matches!(c, 'x' | 'X' | '#').then_some((x as i32, y as i32))
                })
            })
            .collect();
        QuintetMask::from_coords(&coords)
    }

    /// The occupied cells, normalized and sorted.
    pub fn cells(&self) -> &[(i32, i32)] {
        &self.cells
    }

    /// How many cells the shape occupies.
    pub fn count(&self) -> usize {
        self.cells.len()
    }

    /// Does the shape occupy `(x, y)` (in normalized mask space)?
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.cells.binary_search(&(x, y)).is_ok()
    }

    /// Width of the shape's bounding box, in cells.
    pub fn width(&self) -> i32 {
        self.cells
            .iter()
            .map(|&(x, _)| x)
            .max()
            .map_or(0, |m| m + 1)
    }

    /// Height of the shape's bounding box, in cells.
    pub fn height(&self) -> i32 {
        self.cells
            .iter()
            .map(|&(_, y)| y)
            .max()
            .map_or(0, |m| m + 1)
    }

    /// Are all occupied cells reachable from one another via orthogonal steps?
    /// An empty shape is not connected.
    pub fn is_connected(&self) -> bool {
        if self.cells.is_empty() {
            return false;
        }
        let mut seen = vec![self.cells[0]];
        let mut stack = vec![self.cells[0]];
        while let Some((x, y)) = stack.pop() {
            for (dx, dy) in ORTHOGONAL {
                let n = (x + dx, y + dy);
                if self.contains(n.0, n.1) && !seen.contains(&n) {
                    seen.push(n);
                    stack.push(n);
                }
            }
        }
        seen.len() == self.cells.len()
    }

    /// Do the occupied cells form a single diagonal line (every step a corner-only
    /// `(1, ±1)` move)? Such shapes are explicitly banned even though they are
    /// also disconnected.
    pub fn is_diagonal_line(&self) -> bool {
        if self.cells.len() < 2 {
            return false;
        }
        // Cells are sorted by (x, y). A ±1-slope line has every consecutive
        // (sorted-by-x) step equal to the very first one, with dx == 1.
        let first_step = (
            self.cells[1].0 - self.cells[0].0,
            self.cells[1].1 - self.cells[0].1,
        );
        if first_step.0 != 1 || first_step.1.abs() != 1 {
            return false;
        }
        self.cells
            .windows(2)
            .all(|w| (w[1].0 - w[0].0, w[1].1 - w[0].1) == first_step)
    }

    /// Is this a legal quintet: exactly five occupied cells, orthogonally
    /// connected, and not a banned diagonal line?
    pub fn is_valid(&self) -> bool {
        self.count() == QUINTET_CELLS && self.is_connected() && !self.is_diagonal_line()
    }
}

/// Shift `cells` so the bounding box's top-left corner sits at `(0, 0)`.
fn normalize(cells: &mut [(i32, i32)]) {
    let min_x = cells.iter().map(|&(x, _)| x).min().unwrap_or(0);
    let min_y = cells.iter().map(|&(_, y)| y).min().unwrap_or(0);
    for cell in cells.iter_mut() {
        cell.0 -= min_x;
        cell.1 -= min_y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_rows_parses_and_normalizes() {
        // The P-pentomino from the task's allowed-style example, but shifted down
        // and right — it must normalize back to the top-left.
        let shifted = QuintetMask::from_rows(&["ooooo", "ooxxo", "ooxoo", "ooxoo", "ooxoo"]);
        let anchored = QuintetMask::from_rows(&["xxooo", "xoooo", "xoooo", "xoooo"]);
        assert_eq!(shifted, anchored);
        assert_eq!(shifted.count(), 5);
        assert_eq!((anchored.width(), anchored.height()), (2, 4));
    }

    #[test]
    fn valid_connected_quintet_passes() {
        // An L/P-style 5-cell shape, orthogonally connected.
        let m = QuintetMask::from_rows(&["xxooo", "xoooo", "xoooo", "xoooo"]);
        assert!(m.is_connected());
        assert!(!m.is_diagonal_line());
        assert!(m.is_valid());
    }

    #[test]
    fn diagonal_line_fails() {
        let m = QuintetMask::from_rows(&["xoooo", "oxooo", "ooxoo", "oooxo", "oooox"]);
        assert_eq!(m.count(), 5);
        assert!(m.is_diagonal_line());
        assert!(!m.is_connected());
        assert!(!m.is_valid());
    }

    #[test]
    fn disconnected_shape_fails() {
        // Four-in-a-row plus a stranded fifth cell touching nothing.
        let m = QuintetMask::from_rows(&["xxxxo", "ooooo", "ooooo", "ooooo", "oooox"]);
        assert_eq!(m.count(), 5);
        assert!(!m.is_connected());
        assert!(!m.is_valid());
    }

    #[test]
    fn corner_only_touch_is_not_connected() {
        // Two cells touching only through a corner: disconnected.
        let m = QuintetMask::from_rows(&["xooo", "oxoo", "xxxo"]);
        // (0,0) touches (1,1) only diagonally; the bottom run is connected, but
        // the top-left cell hangs off a corner.
        assert!(!m.is_connected());
        assert!(!m.is_valid());
    }

    #[test]
    fn wrong_cell_count_fails() {
        let four = QuintetMask::from_rows(&["xxoo", "xxoo"]);
        assert_eq!(four.count(), 4);
        assert!(!four.is_valid());
        let six = QuintetMask::from_rows(&["xxxoo", "xxxoo"]);
        assert_eq!(six.count(), 6);
        assert!(!six.is_valid());
    }

    #[test]
    fn straight_line_is_valid_and_not_diagonal() {
        // The I-pentomino: connected, not a diagonal line.
        let m = QuintetMask::from_rows(&["xxxxx"]);
        assert!(m.is_valid());
        assert!(!m.is_diagonal_line());
    }

    #[test]
    fn single_cell_is_not_a_diagonal_line() {
        let m = QuintetMask::from_coords(&[(0, 0)]);
        assert!(!m.is_diagonal_line());
    }

    #[test]
    fn anti_diagonal_line_is_detected() {
        let m = QuintetMask::from_rows(&["oooox", "oooxo", "ooxoo", "oxooo", "xoooo"]);
        assert!(m.is_diagonal_line());
        assert!(!m.is_valid());
    }
}
