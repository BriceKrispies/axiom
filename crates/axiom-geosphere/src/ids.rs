//! Neutral topology identity vocabulary.
//!
//! A [`RegionId`] names one region — a single icosphere site (dual-mesh cell
//! centre), which is also the index into an [`crate::Icosphere`]'s `sites` and
//! into every per-region array a consumer hangs off the topology. It carries no
//! behaviour of its own; it is the noun the [`crate::RegionGraph`] and the ring
//! validator traffic in, so a caller can name the regions the graph hands back.

/// A region (icosphere site) index. Regions are numbered `0..region_count` in
/// the deterministic order [`crate::build_icosphere`] interns their unit sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RegionId(pub u32);

impl RegionId {
    /// This region's index into the flat per-region arrays (`sites`, CSR
    /// `offsets`, and any consumer's scalar fields).
    pub fn index(self) -> usize {
        self.0 as usize
    }
}
