//! [`CrucibleVariant`]: the scene's detail dial, and the only thing that changes
//! between two builds of the crucible.
//!
//! The point of the variant is the **topology-change proof**. A procedural
//! geometry library earns its keep only if the same authored scene can be
//! re-tessellated at a different density without being re-authored, so the
//! variant carries nothing but tessellation counts: how many stations a sweep is
//! sampled at, how many floors a building stacks, how many rings a sphere has,
//! how fine the terrain grid and the implicit field are. Every downstream
//! builder reads its counts from here and from nowhere else, which is what makes
//! "vertex and index counts differ between variants" a property a test can
//! assert on every object at once.
//!
//! Nothing here is random and nothing here is measured. A variant is a pure
//! value; `params()` is a pure function of it.

/// Which detail level the crucible scene is built at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrucibleVariant {
    /// The shipping density — what the browser page builds by default.
    Base,
    /// Everything refined: more path stations, more rings, more floors, a finer
    /// terrain grid and a finer implicit lattice.
    Dense,
    /// Everything coarsened, down to the lowest count each operator accepts.
    Coarse,
}

impl CrucibleVariant {
    /// Every variant, in a fixed order — the list a test sweeps.
    pub const ALL: [CrucibleVariant; 3] = [
        CrucibleVariant::Base,
        CrucibleVariant::Dense,
        CrucibleVariant::Coarse,
    ];

    /// The tessellation counts this variant builds every object at.
    pub fn params(self) -> DetailParams {
        DETAIL_TABLE[self.index()]
    }

    /// A stable lowercase name — the value the page's `?detail=` query accepts
    /// and the label the legend prints.
    pub fn label(self) -> &'static str {
        ["base", "dense", "coarse"][self.index()]
    }

    /// Parse a `?detail=` value; anything unrecognised (including an absent
    /// parameter) is [`CrucibleVariant::Base`].
    pub fn from_label(label: &str) -> CrucibleVariant {
        CrucibleVariant::ALL
            .into_iter()
            .find(|variant| variant.label() == label)
            .unwrap_or(CrucibleVariant::Base)
    }

    /// The variant's index into the parameter table.
    fn index(self) -> usize {
        self as usize
    }
}

/// The tessellation counts one variant builds the scene at.
///
/// Every field is a raw count validated at its use site by the operator's own
/// newtype (`Segments`, `Rings`, `Samples`, `Subdivisions`), so an out-of-range
/// value fails loudly at construction instead of silently producing a sliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetailParams {
    /// Stations a swept bone (a leg, the neck, the muzzle, an ear, the tail) is
    /// sampled at along its own path.
    pub sweep_samples: u32,
    /// Radial divisions for circular profiles, lathes and cylinders.
    pub ring_segments: u32,
    /// Latitudinal rings on a UV sphere / capsule.
    pub sphere_rings: u32,
    /// Radial divisions on a UV sphere.
    pub sphere_segments: u32,
    /// Recursive refinement of the icosphere the dog's skull is built from.
    pub icosphere_subdivisions: u32,
    /// Heightfield cells along each axis (the grid is `cells + 1` samples wide).
    pub terrain_cells: u32,
}

/// The three variants' counts, indexed by [`CrucibleVariant`]'s discriminant.
const DETAIL_TABLE: [DetailParams; 3] = [
    // Base
    DetailParams {
        sweep_samples: 10,
        ring_segments: 20,
        sphere_rings: 14,
        sphere_segments: 22,
        icosphere_subdivisions: 2,
        terrain_cells: 44,
    },
    // Dense
    DetailParams {
        sweep_samples: 18,
        ring_segments: 40,
        sphere_rings: 28,
        sphere_segments: 44,
        icosphere_subdivisions: 3,
        terrain_cells: 88,
    },
    // Coarse
    DetailParams {
        sweep_samples: 4,
        ring_segments: 6,
        sphere_rings: 4,
        sphere_segments: 6,
        icosphere_subdivisions: 0,
        terrain_cells: 12,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_round_trips_through_its_label() {
        CrucibleVariant::ALL.into_iter().for_each(|variant| {
            assert_eq!(CrucibleVariant::from_label(variant.label()), variant);
        });
        assert_eq!(CrucibleVariant::from_label("nonsense"), CrucibleVariant::Base);
    }

    #[test]
    fn dense_refines_and_coarse_coarsens_every_count() {
        let base = CrucibleVariant::Base.params();
        let dense = CrucibleVariant::Dense.params();
        let coarse = CrucibleVariant::Coarse.params();
        assert!(dense.sweep_samples > base.sweep_samples);
        assert!(coarse.sweep_samples < base.sweep_samples);
        assert!(dense.terrain_cells > base.terrain_cells);
        assert!(coarse.terrain_cells < base.terrain_cells);
        assert!(dense.ring_segments > base.ring_segments);
        assert!(coarse.icosphere_subdivisions < base.icosphere_subdivisions);
    }
}
