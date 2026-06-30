//! The six stations of the crucible and their fixed floor layout.
//!
//! Every station owns a rectangular cell in a 3×2 floor grid. A station authors
//! its bodies in *local* space; the harness adds the station's world-space
//! `origin()` so the six cells never overlap and the whole room renders at once.
//! The enum is the stable identity a `CrucibleReport` names and the camera aims at.

use axiom::prelude::Vec3;

/// One proving station. The order here is the canonical station order used by the
/// report, the camera carousel, and the overview layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrucibleStation {
    /// Body kinds: static / dynamic / kinematic / disabled bodies under gravity.
    BodyBay,
    /// Narrow-phase contacts: sphere/plane, sphere/sphere, sphere/box, box/plane.
    ContactBay,
    /// Material response: restitution ladder + density-driven mass exchange.
    MaterialBay,
    /// Spatial queries: raycast hit/miss and overlap-sphere membership.
    QueryBay,
    /// Stress: a deterministic stack/pile exercising the broad phase + solver.
    StressBay,
    /// Replay: the hidden second world proving same-input determinism.
    ReplayBay,
}

/// Floor-grid spacing between station cell centres.
const COL_SPACING: f32 = 18.0;
const ROW_SPACING: f32 = 14.0;

impl CrucibleStation {
    /// The six stations in canonical order.
    pub const ALL: [CrucibleStation; 6] = [
        CrucibleStation::BodyBay,
        CrucibleStation::ContactBay,
        CrucibleStation::MaterialBay,
        CrucibleStation::QueryBay,
        CrucibleStation::StressBay,
        CrucibleStation::ReplayBay,
    ];

    /// The station's stable index (0..6) in canonical order.
    pub fn index(self) -> u32 {
        match self {
            CrucibleStation::BodyBay => 0,
            CrucibleStation::ContactBay => 1,
            CrucibleStation::MaterialBay => 2,
            CrucibleStation::QueryBay => 3,
            CrucibleStation::StressBay => 4,
            CrucibleStation::ReplayBay => 5,
        }
    }

    /// A short stable name for the report / overlay.
    pub fn name(self) -> &'static str {
        match self {
            CrucibleStation::BodyBay => "body-bay",
            CrucibleStation::ContactBay => "contact-bay",
            CrucibleStation::MaterialBay => "material-bay",
            CrucibleStation::QueryBay => "query-bay",
            CrucibleStation::StressBay => "stress-bay",
            CrucibleStation::ReplayBay => "replay-bay",
        }
    }

    /// The station's world-space origin (floor centre) in the 3×2 grid.
    pub fn origin(self) -> Vec3 {
        let i = self.index();
        let col = (i % 3) as f32 - 1.0; // -1, 0, +1
        let row = (i / 3) as f32 - 0.5; // -0.5, +0.5
        Vec3::new(col * COL_SPACING, 0.0, row * ROW_SPACING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_indices_are_unique_and_dense() {
        let mut seen = std::collections::BTreeSet::new();
        for s in CrucibleStation::ALL {
            assert!(seen.insert(s.index()), "duplicate index for {}", s.name());
        }
        assert_eq!(seen, (0..6).collect());
    }

    #[test]
    fn origins_are_distinct_per_station() {
        let mut points = Vec::new();
        for s in CrucibleStation::ALL {
            let o = s.origin();
            assert!(
                !points.iter().any(|p: &Vec3| p.x == o.x && p.z == o.z),
                "overlapping origin for {}",
                s.name()
            );
            points.push(o);
        }
        assert_eq!(points.len(), 6);
    }

    #[test]
    fn names_are_stable_and_kebab() {
        assert_eq!(CrucibleStation::BodyBay.name(), "body-bay");
        assert_eq!(CrucibleStation::ReplayBay.index(), 5);
    }
}
