//! The pure value-type vocabulary `ScatterApi` traffics in.

use axiom_kernel::{Meters, Ratio};

/// An integer 2-D cell coordinate addressing one scatter cell on the ground
/// plane. Cells tile the world; each is scattered independently and seamlessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellCoord {
    /// Cell index along world X.
    pub x: i32,
    /// Cell index along world Z.
    pub z: i32,
}

impl CellCoord {
    /// A cell coordinate at `(x, z)`.
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// The placement rule for a cell: a **jittered sub-grid**. Each cell is divided
/// into `sites_per_side × sites_per_side` sub-cells; each sub-cell may spawn one
/// site, wiggled from its centre by up to `jitter` of a sub-cell. `fill` thins
/// the grid into clumps and clearings by dropping a fraction of sub-cells. The
/// sub-grid gives an implicit minimum spacing that holds across cell boundaries.
#[derive(Debug, Clone, Copy)]
pub struct ScatterRule {
    /// Sub-grid resolution per cell side; up to `sites_per_side²` sites per cell.
    pub sites_per_side: u32,
    /// How far a site wiggles from its sub-cell centre, as a fraction `[0, 1]` of
    /// a sub-cell (`0` = a perfect grid, `1` = anywhere in the sub-cell).
    pub jitter: Ratio,
    /// Fraction `[0, 1]` of sub-cells that spawn a site (`1` = full grid, lower =
    /// clearings). The keep decision uses only the site's own seed, so it is
    /// seamless across cells.
    pub fill: Ratio,
}

/// One deterministically-placed scatter site: a ground position plus a stable
/// per-site `seed` the caller expands into per-instance attributes (yaw, scale,
/// species). The module places; the caller dresses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScatterSite {
    /// World X of the site (metres).
    pub x: Meters,
    /// World Z of the site (metres).
    pub z: Meters,
    /// A stable seed unique to this site, for deriving its attributes.
    pub seed: u64,
}
