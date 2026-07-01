//! [`TerrainApi`] — deterministic coherent value-noise heightfields.
//!
//! A height is the bilinear interpolation of the four surrounding **lattice
//! values**, each a draw from an entropy stream keyed by its lattice site. Because
//! a lattice value is a pure function of its *world* lattice coordinate, every
//! heightfield that covers a given world point computes the same value there — so
//! adjacent tiles share a seamless edge. Branchless and integer-only (heights are
//! `i32`; no naked floats cross the boundary).

use axiom_entropy::EntropyApi;
use axiom_space::SpaceApi;

use crate::height_field::HeightField;

/// Cells per lattice unit — the noise feature size (larger = smoother terrain).
const LATTICE_SPACING: u32 = 8;
/// The height range each lattice value spans, `[0, HEIGHT_RANGE)`.
const HEIGHT_RANGE: u64 = 1024;
/// The terrain noise version. Bump to deliberately re-key generation (+ regolden).
const TERRAIN_VERSION: u32 = 1;

/// The deterministic terrain facade.
#[derive(Debug)]
pub struct TerrainApi;

impl TerrainApi {
    /// A `width × height` heightfield whose top-left cell is world `(origin_x,
    /// origin_y)`. Heights are coherent value noise; because lattice values are a
    /// pure function of world lattice coordinates, two heightfields that overlap
    /// in world space agree on the overlap (seamless tiling).
    pub fn heightfield(
        seed: u64,
        origin_x: u32,
        origin_y: u32,
        width: u32,
        height: u32,
    ) -> HeightField {
        let heights = (0..height)
            .flat_map(|cy| {
                (0..width).map(move |cx| cell_height(seed, origin_x + cx, origin_y + cy))
            })
            .collect();
        HeightField::new(width, height, heights)
    }
}

/// The coherent height at world cell `(wx, wy)`: bilinear interpolation of the
/// four surrounding lattice values. Branchless.
fn cell_height(seed: u64, wx: u32, wy: u32) -> i32 {
    let (lx0, fx) = (wx / LATTICE_SPACING, wx % LATTICE_SPACING);
    let (ly0, fy) = (wy / LATTICE_SPACING, wy % LATTICE_SPACING);
    let v00 = lattice_value(seed, lx0, ly0);
    let v10 = lattice_value(seed, lx0 + 1, ly0);
    let v01 = lattice_value(seed, lx0, ly0 + 1);
    let v11 = lattice_value(seed, lx0 + 1, ly0 + 1);
    let top = lerp(v00, v10, fx);
    let bottom = lerp(v01, v11, fx);
    lerp(top, bottom, fy) as i32
}

/// The lattice value at integer lattice coordinate `(lx, ly)` — a draw from an
/// entropy stream keyed by the lattice site, so it is global (shared by every
/// heightfield covering it) and reproducible.
fn lattice_value(seed: u64, lx: u32, ly: u32) -> i64 {
    let site = SpaceApi::child(
        &SpaceApi::child(&SpaceApi::root(), u64::from(lx)),
        u64::from(ly),
    );
    let mut stream = EntropyApi::stream(seed, &site, TERRAIN_VERSION);
    (stream.next_u64() % HEIGHT_RANGE) as i64
}

/// Integer linear interpolation between `a` and `b` at `t / LATTICE_SPACING`.
fn lerp(a: i64, b: i64, t: u32) -> i64 {
    a + (b - a) * i64::from(t) / i64::from(LATTICE_SPACING)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heightfield_is_deterministic() {
        let a = TerrainApi::heightfield(7, 0, 0, 16, 8);
        let b = TerrainApi::heightfield(7, 0, 0, 16, 8);
        assert_eq!(a, b);
        assert_eq!(a.to_bytes(), b.to_bytes());
        assert_eq!(a.width(), 16);
        assert_eq!(a.height(), 8);
        assert_eq!(a.heights().len(), 128);
    }

    #[test]
    fn adjacent_tiles_share_a_seamless_edge() {
        // Lattice values are keyed by WORLD coords, so a tile at origin 0 and a
        // tile at origin 15 agree on the shared world column 15 (seam == 0).
        let seed = 99;
        let a = TerrainApi::heightfield(seed, 0, 0, 16, 6);
        let b = TerrainApi::heightfield(seed, 15, 0, 16, 6);
        for cy in 0..6 {
            assert_eq!(
                a.at(15, cy),
                b.at(0, cy),
                "shared world column 15 must be seamless"
            );
        }
    }

    #[test]
    fn the_field_varies_and_is_coherent() {
        let f = TerrainApi::heightfield(3, 0, 0, 32, 1);
        let heights = f.heights();
        let min = *heights.iter().min().unwrap();
        let max = *heights.iter().max().unwrap();
        assert!(max > min);
        // Coherent — a horizontal step never jumps more than twice the lattice
        // gradient (value noise is interpolated, not white noise).
        let bound = (HEIGHT_RANGE as u32 / LATTICE_SPACING) * 2;
        for w in heights.windows(2) {
            assert!((w[0] - w[1]).unsigned_abs() < bound);
        }
    }

    #[test]
    fn distinct_seeds_or_origins_differ() {
        let base = TerrainApi::heightfield(7, 0, 0, 16, 8);
        assert_ne!(base, TerrainApi::heightfield(8, 0, 0, 16, 8));
        assert_ne!(base, TerrainApi::heightfield(7, 64, 0, 16, 8));
    }

    #[test]
    fn at_reads_cells_and_guards_out_of_range() {
        let f = TerrainApi::heightfield(7, 0, 0, 4, 3);
        assert_eq!(f.at(0, 0), f.heights()[0]);
        assert_eq!(f.at(3, 2), f.heights()[2 * 4 + 3]);
        assert_eq!(f.at(4, 0), 0);
        assert_eq!(f.at(0, 3), 0);
    }

    #[test]
    fn golden_heightfield_digest_is_stable() {
        let f = TerrainApi::heightfield(7, 0, 0, 16, 8);
        assert_eq!(f.digest().raw(), 13_556_001_842_823_796_327);
    }

    #[test]
    fn types_are_debug() {
        let f = TerrainApi::heightfield(7, 0, 0, 2, 2);
        assert!(!format!("{f:?}").is_empty());
        assert!(!format!("{:?}", TerrainApi).is_empty());
    }
}
