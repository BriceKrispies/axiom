//! [`SceneVariant`]: the scene's detail dial, and the only thing that changes
//! between two builds of the geometry.
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

/// Which detail level the scene's geometry is built at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneVariant {
    /// The shipping density — what the browser page builds by default.
    Base,
    /// Everything refined: more path stations, more rings, more floors, a finer
    /// terrain grid and a finer implicit lattice.
    Dense,
    /// Everything coarsened, down to the lowest count each operator accepts.
    Coarse,
}

impl SceneVariant {
    /// Every variant, in a fixed order — the list a test sweeps.
    pub const ALL: [SceneVariant; 3] = [
        SceneVariant::Base,
        SceneVariant::Dense,
        SceneVariant::Coarse,
    ];

    /// The tessellation counts this variant builds every object at.
    pub fn params(self) -> DetailParams {
        DETAIL_TABLE[self.index()]
    }

    /// A stable lowercase name — the label the detail slider's read-out prints
    /// and the one the tests report by.
    pub fn label(self) -> &'static str {
        ["base", "dense", "coarse"][self.index()]
    }

    /// The variant the detail **dial** names, ordered coarse → base → dense so
    /// the slider runs the way a density slider should. Anything out of range is
    /// [`SceneVariant::Base`], the shipping density.
    pub fn from_index(index: usize) -> SceneVariant {
        [SceneVariant::Coarse, SceneVariant::Base, SceneVariant::Dense]
            .get(index)
            .copied()
            .unwrap_or(SceneVariant::Base)
    }

    /// This variant's position on the detail dial — the inverse of
    /// [`SceneVariant::from_index`].
    pub fn dial_index(self) -> usize {
        [1, 2, 0][self.index()]
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

/// The three variants' counts, indexed by [`SceneVariant`]'s discriminant.
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
    fn every_variant_round_trips_through_its_dial_position() {
        SceneVariant::ALL.into_iter().for_each(|variant| {
            assert_eq!(SceneVariant::from_index(variant.dial_index()), variant);
            assert!(!variant.label().is_empty());
        });
        // The dial runs coarse → base → dense, and out-of-range is the shipping
        // density rather than a panic.
        assert_eq!(SceneVariant::from_index(0), SceneVariant::Coarse);
        assert_eq!(SceneVariant::from_index(1), SceneVariant::Base);
        assert_eq!(SceneVariant::from_index(2), SceneVariant::Dense);
        assert_eq!(SceneVariant::from_index(9), SceneVariant::Base);
    }

    #[test]
    fn dense_refines_and_coarse_coarsens_every_count() {
        let base = SceneVariant::Base.params();
        let dense = SceneVariant::Dense.params();
        let coarse = SceneVariant::Coarse.params();
        assert!(dense.sweep_samples > base.sweep_samples);
        assert!(coarse.sweep_samples < base.sweep_samples);
        assert!(dense.terrain_cells > base.terrain_cells);
        assert!(coarse.terrain_cells < base.terrain_cells);
        assert!(dense.ring_segments > base.ring_segments);
        assert!(coarse.icosphere_subdivisions < base.icosphere_subdivisions);
    }
}
