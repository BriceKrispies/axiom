//! The scatter facade: per-cell deterministic jittered-sub-grid placement.

use axiom_entropy::EntropyApi;
use axiom_kernel::Meters;
use axiom_space::{Address, SpaceApi};

use crate::ids::{CellCoord, ScatterRule, ScatterSite};

/// Fixed address segment keying the scatter domain ("scattr\0\x01"), so a cell's
/// stream derives from `(seed, domain/cell.x/cell.z, version)` reproducibly.
const SCATTER_DOMAIN: u64 = 0x_73_63_61_74_74_72_00_01;
/// Version salt for the scatter address space; bump to intentionally reshuffle.
const SCATTER_VERSION: u32 = 1;

/// Chunked deterministic point scatter.
///
/// Stateless: a cell's sites are a pure function of `(seed, cell, cell_size,
/// rule)`, so the field tiles identically on every platform and every visit.
#[derive(Debug)]
pub struct ScatterApi;

impl ScatterApi {
    /// The scattered sites of one `cell`, a `cell_size`-metre square. Placement is
    /// a jittered sub-grid: the cell is divided into `rule.sites_per_side²`
    /// sub-cells, each of which spawns one site (kept with probability
    /// `rule.fill`, wiggled up to `rule.jitter` of a sub-cell from its centre).
    ///
    /// Each site is derived from an independent, order-stable sub-stream of the
    /// cell's entropy address, so the result is deterministic and — because a
    /// site depends only on its own cell — seamless across cell boundaries, with
    /// the sub-grid providing an implicit minimum spacing that holds at the seam.
    pub fn chunk_sites(
        seed: u64,
        cell: CellCoord,
        cell_size: Meters,
        rule: &ScatterRule,
    ) -> Vec<ScatterSite> {
        let base = EntropyApi::stream(seed, &cell_address(cell), SCATTER_VERSION);
        let k = rule.sites_per_side;
        let size = cell_size.get();
        let sub = size / (k.max(1) as f32);
        let origin_x = cell.x as f32 * size;
        let origin_z = cell.z as f32 * size;
        let jitter = rule.jitter.get();
        let fill = rule.fill.get();
        (0..k * k)
            .map(|n| {
                let mut s = base.fork(u64::from(n));
                let keep = s.unit().get() < fill;
                let jx = (s.unit().get() - 0.5) * jitter;
                let jz = (s.unit().get() - 0.5) * jitter;
                let site_seed = s.next_u64();
                let x = origin_x + ((n % k) as f32 + 0.5 + jx) * sub;
                let z = origin_z + ((n / k) as f32 + 0.5 + jz) * sub;
                (
                    keep,
                    ScatterSite {
                        x: Meters::finite_or_zero(x),
                        z: Meters::finite_or_zero(z),
                        seed: site_seed,
                    },
                )
            })
            .filter(|(keep, _)| *keep)
            .map(|(_, site)| site)
            .collect()
    }
}

/// The per-cell entropy address: `root / SCATTER_DOMAIN / cell.x / cell.z`, with
/// each signed cell index reinterpreted into a `u64` segment so every cell (incl.
/// negatives) keys a distinct, reproducible stream.
fn cell_address(cell: CellCoord) -> Address {
    let domain = SpaceApi::child(&SpaceApi::root(), SCATTER_DOMAIN);
    let along_x = SpaceApi::child(&domain, u64::from(cell.x as u32));
    SpaceApi::child(&along_x, u64::from(cell.z as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Ratio;

    fn rule(sites_per_side: u32, jitter: f32, fill: f32) -> ScatterRule {
        ScatterRule {
            sites_per_side,
            jitter: Ratio::new(jitter).unwrap(),
            fill: Ratio::new(fill).unwrap(),
        }
    }

    #[test]
    fn is_deterministic_for_the_same_cell() {
        let r = rule(4, 0.8, 1.0);
        let a = ScatterApi::chunk_sites(9, CellCoord::new(2, -3), Meters::finite_or_zero(16.0), &r);
        let b = ScatterApi::chunk_sites(9, CellCoord::new(2, -3), Meters::finite_or_zero(16.0), &r);
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn full_fill_yields_one_site_per_subcell() {
        let sites = ScatterApi::chunk_sites(
            1,
            CellCoord::new(0, 0),
            Meters::finite_or_zero(10.0),
            &rule(5, 0.5, 1.0),
        );
        assert_eq!(sites.len(), 25);
    }

    #[test]
    fn zero_fill_yields_nothing_and_partial_fill_thins() {
        let size = Meters::finite_or_zero(10.0);
        let none = ScatterApi::chunk_sites(1, CellCoord::new(0, 0), size, &rule(6, 0.5, 0.0));
        assert!(none.is_empty());
        let some = ScatterApi::chunk_sites(1, CellCoord::new(0, 0), size, &rule(6, 0.5, 0.5));
        // Partial fill keeps a strict subset of the full 36-site grid.
        assert!(!some.is_empty());
        assert!(some.len() < 36);
    }

    #[test]
    fn zero_sites_per_side_is_empty() {
        let sites = ScatterApi::chunk_sites(
            1,
            CellCoord::new(0, 0),
            Meters::finite_or_zero(10.0),
            &rule(0, 0.5, 1.0),
        );
        assert!(sites.is_empty());
    }

    #[test]
    fn every_site_lies_inside_its_cell() {
        let size = 16.0;
        let cell = CellCoord::new(3, -2);
        let sites =
            ScatterApi::chunk_sites(7, cell, Meters::finite_or_zero(size), &rule(4, 1.0, 1.0));
        let (lo_x, hi_x) = (cell.x as f32 * size, (cell.x + 1) as f32 * size);
        let (lo_z, hi_z) = (cell.z as f32 * size, (cell.z + 1) as f32 * size);
        // Single-condition, message-less asserts: a short-circuit (`&&`) leaves its
        // skip-branch uncovered and a custom failure message leaves its format args
        // uncovered — both count against the region gate, which measures test code.
        for s in &sites {
            assert!(s.x.get() >= lo_x);
            assert!(s.x.get() <= hi_x);
            assert!(s.z.get() >= lo_z);
            assert!(s.z.get() <= hi_z);
        }
    }

    #[test]
    fn distinct_cells_produce_distinct_fields() {
        let r = rule(4, 0.8, 1.0);
        let size = Meters::finite_or_zero(16.0);
        let a = ScatterApi::chunk_sites(5, CellCoord::new(0, 0), size, &r);
        let b = ScatterApi::chunk_sites(5, CellCoord::new(7, 4), size, &r);
        // Different cells → different site seeds (independent streams).
        assert_ne!(a[0].seed, b[0].seed);
    }

    #[test]
    fn neighbouring_cells_keep_their_sub_grid_spacing_across_the_seam() {
        // Two horizontally-adjacent cells; sites from each hug their own sub-grid,
        // so no site in the right column of cell A lands atop a site in the left
        // column of cell B — the implicit min spacing survives the boundary.
        let size = 12.0;
        let r = rule(4, 0.6, 1.0); // moderate jitter: sub-cells never overlap
        let a = ScatterApi::chunk_sites(3, CellCoord::new(0, 0), Meters::finite_or_zero(size), &r);
        let b = ScatterApi::chunk_sites(3, CellCoord::new(1, 0), Meters::finite_or_zero(size), &r);
        let sub = size / 4.0;
        // Every cross-cell pair is at least (1 - jitter) sub-cells apart in X or Z.
        let min_gap = (1.0 - 0.6) * sub;
        let mut nearest = f32::MAX;
        for pa in &a {
            for pb in &b {
                let d =
                    ((pa.x.get() - pb.x.get()).powi(2) + (pa.z.get() - pb.z.get()).powi(2)).sqrt();
                nearest = nearest.min(d);
            }
        }
        assert!(nearest >= min_gap);
    }
}
