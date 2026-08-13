//! The detail / tessellation vocabulary every generator and operator speaks.
//!
//! These are **caller-chosen, backend-neutral, deterministic** quantities. This
//! module never asks what device it is on, never reads a frame time, and never
//! consults a hardware capability: choosing a budget is a policy decision that
//! belongs to whoever is composing the geometry. What lives here is only the
//! bounded, validated vocabulary for *expressing* that choice, so an operator
//! can refuse an absurd request instead of allocating without limit.

use axiom_mesh::{MeshError, MeshErrorCode, MeshResult};

/// The largest number of radial/linear divisions any generator will honour.
pub const MAX_SEGMENTS: u32 = 4096;
/// The largest number of latitudinal divisions any generator will honour.
pub const MAX_RINGS: u32 = 4096;
/// The deepest recursive refinement any operator will honour. Each level
/// multiplies triangle count by four, so 8 is already ~65k triangles per input
/// triangle.
pub const MAX_SUBDIVISIONS: u32 = 8;
/// The largest number of samples any path/curve operator will honour.
pub const MAX_SAMPLES: u32 = 65_536;

fn invalid_tessellation(message: &'static str) -> MeshError {
    MeshError::new(MeshErrorCode::InvalidTessellation, message)
}

/// Radial or linear divisions around/along a generated surface.
///
/// At least 3 — two segments cannot enclose an area, so a 2-segment cylinder is
/// a degenerate sliver rather than a coarse cylinder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Segments(u32);

impl Segments {
    /// Validate a segment count in `3..=MAX_SEGMENTS`.
    pub fn new(value: u32) -> MeshResult<Segments> {
        ((value >= 3) & (value <= MAX_SEGMENTS))
            .then_some(Segments(value))
            .ok_or_else(|| invalid_tessellation("segments must be in 3..=4096"))
    }

    /// The validated count.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Latitudinal divisions from one pole/edge of a generated surface to the other.
///
/// At least 2 — one ring degenerates a sphere into a disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rings(u32);

impl Rings {
    /// Validate a ring count in `2..=MAX_RINGS`.
    pub fn new(value: u32) -> MeshResult<Rings> {
        ((value >= 2) & (value <= MAX_RINGS))
            .then_some(Rings(value))
            .ok_or_else(|| invalid_tessellation("rings must be in 2..=4096"))
    }

    /// The validated count.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Levels of recursive refinement. Zero is legal and means "do not refine".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Subdivisions(u32);

impl Subdivisions {
    /// Validate a subdivision level in `0..=MAX_SUBDIVISIONS`.
    pub fn new(value: u32) -> MeshResult<Subdivisions> {
        (value <= MAX_SUBDIVISIONS)
            .then_some(Subdivisions(value))
            .ok_or_else(|| invalid_tessellation("subdivisions must be in 0..=8"))
    }

    /// The validated level.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The number of points a curve or path is sampled at.
///
/// At least 2 — a single sample has no extent to sweep along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Samples(u32);

impl Samples {
    /// Validate a sample count in `2..=MAX_SAMPLES`.
    pub fn new(value: u32) -> MeshResult<Samples> {
        ((value >= 2) & (value <= MAX_SAMPLES))
            .then_some(Samples(value))
            .ok_or_else(|| invalid_tessellation("samples must be in 2..=65536"))
    }

    /// The validated count.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A ceiling on how much geometry an operator may produce.
///
/// An operator that would exceed the budget fails with
/// [`MeshErrorCode::BudgetExceeded`] rather than allocating. This is how a
/// caller bounds work whose output size is not obvious from the inputs — a
/// marching-cubes extraction, a deep subdivision — without the layer inventing a
/// hard-coded cap of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DetailBudget {
    max_triangles: u32,
}

impl DetailBudget {
    /// The budget an operator uses when the caller expresses no preference:
    /// one million triangles, far above any single reasonable generated object
    /// and far below anything that could exhaust memory.
    pub const DEFAULT_MAX_TRIANGLES: u32 = 1_000_000;

    /// Validate a triangle ceiling. Must be at least 1.
    pub fn new(max_triangles: u32) -> MeshResult<DetailBudget> {
        (max_triangles >= 1)
            .then_some(DetailBudget { max_triangles })
            .ok_or_else(|| invalid_tessellation("a detail budget must allow at least one triangle"))
    }

    /// The ceiling.
    pub const fn max_triangles(self) -> u32 {
        self.max_triangles
    }

    /// Accept `triangles`, or report [`MeshErrorCode::BudgetExceeded`].
    pub fn admit(self, triangles: usize) -> MeshResult<()> {
        (triangles <= self.max_triangles as usize)
            .then_some(())
            .ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::BudgetExceeded,
                    "the operation would produce more triangles than the detail budget allows",
                )
            })
    }
}

impl Default for DetailBudget {
    fn default() -> Self {
        DetailBudget {
            max_triangles: DetailBudget::DEFAULT_MAX_TRIANGLES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_accept_their_domain_and_reject_outside_it() {
        assert_eq!(Segments::new(3).unwrap().get(), 3);
        assert_eq!(Segments::new(MAX_SEGMENTS).unwrap().get(), MAX_SEGMENTS);
        assert_eq!(
            Segments::new(2).unwrap_err().code(),
            MeshErrorCode::InvalidTessellation
        );
        assert_eq!(
            Segments::new(MAX_SEGMENTS + 1).unwrap_err().code(),
            MeshErrorCode::InvalidTessellation
        );
    }

    #[test]
    fn rings_accept_their_domain_and_reject_outside_it() {
        assert_eq!(Rings::new(2).unwrap().get(), 2);
        assert_eq!(Rings::new(MAX_RINGS).unwrap().get(), MAX_RINGS);
        assert_eq!(
            Rings::new(1).unwrap_err().code(),
            MeshErrorCode::InvalidTessellation
        );
        assert_eq!(
            Rings::new(MAX_RINGS + 1).unwrap_err().code(),
            MeshErrorCode::InvalidTessellation
        );
    }

    #[test]
    fn subdivisions_allow_zero_and_reject_beyond_the_cap() {
        assert_eq!(Subdivisions::new(0).unwrap().get(), 0);
        assert_eq!(
            Subdivisions::new(MAX_SUBDIVISIONS).unwrap().get(),
            MAX_SUBDIVISIONS
        );
        assert_eq!(
            Subdivisions::new(MAX_SUBDIVISIONS + 1).unwrap_err().code(),
            MeshErrorCode::InvalidTessellation
        );
    }

    #[test]
    fn samples_need_at_least_two_points() {
        assert_eq!(Samples::new(2).unwrap().get(), 2);
        assert_eq!(Samples::new(MAX_SAMPLES).unwrap().get(), MAX_SAMPLES);
        assert_eq!(
            Samples::new(1).unwrap_err().code(),
            MeshErrorCode::InvalidTessellation
        );
        assert_eq!(
            Samples::new(MAX_SAMPLES + 1).unwrap_err().code(),
            MeshErrorCode::InvalidTessellation
        );
    }

    #[test]
    fn a_budget_admits_up_to_its_ceiling_and_refuses_beyond() {
        let b = DetailBudget::new(10).unwrap();
        assert_eq!(b.max_triangles(), 10);
        assert_eq!(b.admit(10), Ok(()));
        assert_eq!(
            b.admit(11).unwrap_err().code(),
            MeshErrorCode::BudgetExceeded
        );
    }

    #[test]
    fn a_zero_budget_is_rejected() {
        assert_eq!(
            DetailBudget::new(0).unwrap_err().code(),
            MeshErrorCode::InvalidTessellation
        );
    }

    #[test]
    fn the_default_budget_is_generous_but_bounded() {
        let b = DetailBudget::default();
        assert_eq!(b.max_triangles(), DetailBudget::DEFAULT_MAX_TRIANGLES);
        assert_eq!(b.admit(1_000_000), Ok(()));
        assert_eq!(
            b.admit(1_000_001).unwrap_err().code(),
            MeshErrorCode::BudgetExceeded
        );
    }
}
