//! The seeded permutation lattice shared by the gradient and cellular bases.

use axiom_math::DVec2;

/// The permutation table's period. A power of two, so the wrap is a mask.
const PERIOD: usize = 256;

/// The number of evenly-spread unit gradient directions a cell can take.
const GRADIENT_COUNT: usize = 16;

/// A seeded 2D lattice: a shuffled permutation table plus one jittered feature
/// point per cell.
///
/// This is the third basis in the layer, and unlike the other two it is
/// **seeded** — it is built from a [`RandomSource`], so the same lattice
/// yields a different field per world or per run. Compare the table in the
/// crate docs: `value_noise_01` is deliberately unseeded (a field that cannot
/// be reshuffled by an unrelated subsystem) and `value_noise` keys the kernel
/// digest; this one carries its randomness in a table because that is what
/// makes it cheap enough to sample thousands of times while baking an atlas.
///
/// Both a **gradient** basis ([`crate::perlin_2d`]) and a **cellular** basis
/// ([`crate::worley_f1`]) read the same table, which is why they live behind
/// one type rather than two. The source they were promoted from kept them
/// together for the same reason: two structures would let the two fields
/// decorrelate from each other, and a bake that layers cracks over grain wants
/// them registered.
#[derive(Debug, Clone, PartialEq)]
pub struct PermutationLattice {
    /// The permutation, doubled so a `+1` neighbour lookup never needs a
    /// second wrap.
    permutation: [u8; PERIOD * 2],
    /// One jittered feature point per table entry, in `[0, 1)^2` — the cell
    /// offsets a cellular basis measures distance to.
    features: [DVec2; PERIOD],
}

impl PermutationLattice {
    /// Build a lattice from a permutation table and its feature points.
    ///
    /// ## Why this takes the table rather than a generator
    ///
    /// Because **the choice of generator is a reproducibility contract owned by
    /// the caller**, not something the engine may decide. An app reproducing a
    /// reference implementation needs a specific generator — the exact
    /// *sequence* is what its captured output is pinned against — and
    /// substituting another does not merely change some numbers, it moves every
    /// texture the sequence bakes.
    ///
    /// Taking the finished table rather than `&mut impl Rng` keeps this a pure
    /// function of its inputs: the same table always yields the same field, the
    /// lattice is constructible in a test with no generator at all, and no
    /// hidden mutable channel crosses the engine boundary. The shuffle is the
    /// *seeding policy*, which belongs with whoever owns the sequence; the
    /// noise is the algorithm, which belongs here.
    ///
    /// `permutation` is expected to be a permutation of `0..256`, and
    /// `features` to lie in `[0, 1)^2`. Neither is validated: a table that is
    /// not a permutation simply yields a lower-quality field — some cells share
    /// a gradient — which is a quality question for the caller, not a failure
    /// this constructor could usefully report.
    pub fn from_table(permutation: [u8; PERIOD], features: [DVec2; PERIOD]) -> Self {
        PermutationLattice {
            // Doubled so a `+1` neighbour lookup never needs a second wrap.
            permutation: core::array::from_fn(|i| permutation[i % PERIOD]),
            features,
        }
    }

    /// The table entry for an integer lattice cell.
    ///
    /// Two folds through the permutation — one per axis — which is what
    /// decorrelates `(x, y)` from `(y, x)`. Coordinates wrap with a Euclidean
    /// remainder so negative cells land on the tile they belong to rather than
    /// mirroring about the origin.
    pub(crate) fn cell(&self, x: i64, y: i64) -> u8 {
        let wrapped_x = x.rem_euclid(PERIOD as i64) as usize;
        let wrapped_y = y.rem_euclid(PERIOD as i64) as usize;
        let first = self.permutation[wrapped_x] as usize;
        self.permutation[(first + wrapped_y) & (PERIOD - 1)]
    }

    /// The unit gradient direction for a cell, one of [`GRADIENT_COUNT`]
    /// evenly-spread directions.
    pub(crate) fn gradient(&self, x: i64, y: i64) -> DVec2 {
        let index = (self.cell(x, y) as usize) & (GRADIENT_COUNT - 1);
        let angle = (index as f64 / GRADIENT_COUNT as f64) * core::f64::consts::TAU;
        DVec2::new(angle.cos(), angle.sin())
    }

    /// The jittered feature point of a cell, as an offset within it.
    pub(crate) fn feature(&self, x: i64, y: i64) -> DVec2 {
        self.features[self.cell(x, y) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::DeterministicRng;

    /// A table built by the same Fisher-Yates a caller would run, so the tests
    /// exercise a realistic permutation rather than the identity.
    fn shuffled(seed: u64) -> PermutationLattice {
        let mut rng = DeterministicRng::seeded(seed);
        let mut table: [u8; PERIOD] = core::array::from_fn(|i| i as u8);
        (1..PERIOD).rev().for_each(|i| {
            let j = rng.next_bounded(i as u64 + 1) as usize;
            table.swap(i, j);
        });
        let features = core::array::from_fn(|_| {
            let x = (rng.next_bounded(1 << 24) as f64) / f64::from(1u32 << 24);
            let y = (rng.next_bounded(1 << 24) as f64) / f64::from(1u32 << 24);
            DVec2::new(x, y)
        });
        PermutationLattice::from_table(table, features)
    }

    fn lattice() -> PermutationLattice {
        shuffled(0x5eed_1234)
    }

    #[test]
    fn the_same_table_builds_the_same_lattice() {
        assert_eq!(lattice(), lattice());
    }

    /// The constructor is pure: no generator, no hidden state, and an
    /// identity table is as valid an input as a shuffled one.
    #[test]
    fn a_table_alone_is_enough_to_build_one() {
        let identity: [u8; PERIOD] = core::array::from_fn(|i| i as u8);
        let l = PermutationLattice::from_table(identity, [DVec2::splat(0.5); PERIOD]);
        assert_eq!(l.feature(0, 0), DVec2::splat(0.5));
        assert_eq!(l.cell(0, 0), 0);
    }

    #[test]
    fn a_different_table_builds_a_different_lattice() {
        assert_ne!(lattice(), shuffled(7));
    }

    /// A shuffle must be a permutation — every value present exactly once —
    /// which a swap-based fold can only break by having the wrong bounds.
    #[test]
    fn the_table_is_a_permutation_of_every_byte() {
        let l = lattice();
        let mut seen = [0u32; PERIOD];
        (0..PERIOD).for_each(|i| seen[l.permutation[i] as usize] += 1);
        assert!(seen.iter().all(|&n| n == 1), "not a permutation: {seen:?}");
    }

    #[test]
    fn the_table_is_doubled_so_a_neighbour_lookup_never_wraps_twice() {
        let l = lattice();
        (0..PERIOD).for_each(|i| assert_eq!(l.permutation[i], l.permutation[i + PERIOD]));
    }

    #[test]
    fn feature_points_lie_inside_their_cell() {
        let l = lattice();
        l.features.iter().for_each(|f| {
            assert!((0.0..1.0).contains(&f.x), "feature x out of cell: {}", f.x);
            assert!((0.0..1.0).contains(&f.y), "feature y out of cell: {}", f.y);
        });
    }

    #[test]
    fn cells_decorrelate_under_transposition() {
        let l = lattice();
        let differing = (0..64)
            .filter(|&i| l.cell(i, i + 1) != l.cell(i + 1, i))
            .count();
        assert!(differing > 50, "only {differing}/64 transposed pairs differ");
    }

    #[test]
    fn negative_cells_wrap_onto_the_tile_rather_than_mirroring() {
        let l = lattice();
        assert_eq!(l.cell(-1, -1), l.cell(PERIOD as i64 - 1, PERIOD as i64 - 1));
        assert_eq!(l.cell(-(PERIOD as i64), 0), l.cell(0, 0));
    }

    #[test]
    fn gradients_are_unit_length() {
        let l = lattice();
        (-8..8).for_each(|x| {
            (-8..8).for_each(|y| {
                let g = l.gradient(x, y);
                assert!((g.length() - 1.0).abs() < 1.0e-12);
            });
        });
    }

    #[test]
    fn gradients_and_features_are_stable_for_a_cell() {
        let l = lattice();
        assert_eq!(l.gradient(3, -5), l.gradient(3, -5));
        assert_eq!(l.feature(3, -5), l.feature(3, -5));
    }
}
