//! The FX atlas bake's noise toolkit — **the engine's**, in the call shape the
//! source uses.
//!
//! The algorithms used to live here, ported from Claude-of-Duty
//! `src/fx/noise.js:1-158`. They are now `axiom_noise`'s seeded lattice basis,
//! promoted under the Branchless and Coverage Laws:
//!
//! | this module        | `axiom_noise`                        |
//! |--------------------|--------------------------------------|
//! | [`Noise`]          | [`PermutationLattice`]               |
//! | [`Noise::perlin`]  | [`axiom_noise::perlin_2d`]           |
//! | [`Noise::fbm`]     | [`axiom_noise::perlin_fbm_2d`]       |
//! | [`Noise::ridged`]  | [`axiom_noise::perlin_ridged_2d`]    |
//! | [`Noise::warped`]  | [`axiom_noise::perlin_warped_2d`]    |
//! | [`Noise::worley`]  | [`axiom_noise::worley_f1`]           |
//! | [`Noise::worley_edge`] | [`axiom_noise::worley_edge`]     |
//!
//! ## The seeding seam
//!
//! [`PermutationLattice::from_table`] takes the finished table, and **the
//! shuffle stays here**, because the choice of generator is a reproducibility
//! contract owned by this app rather than a detail the engine may pick.
//! [`crate::rng::Rng`] is xoshiro128\*\* precisely because the source's is;
//! substituting the kernel's splitmix64 would move every atlas this bake
//! produces.
//!
//! An earlier draft had the engine take `&mut impl RandomSource` instead. The
//! rulebook rejected it — `engine_no_retained_state` bans a hidden mutable
//! channel across an engine API — and it was right for a better reason than the
//! one it states: taking the table makes the lattice a *pure function of its
//! inputs*, constructible in a test with no generator at all. Fisher-Yates is
//! not the interesting part; the noise is.
//!
//! The draw order is unchanged — 255 descending integer draws for the shuffle,
//! then 256 pairs of unit draws for the jitter — so the sequence this crate's
//! `Rng` sees is exactly what it was and no other subsystem sharing the stream
//! shifts.
//!
//! ## What stays
//!
//! [`clamp01`], [`smoothstep`] and [`encode_srgb`] — three scalar helpers the
//! rest of `fx` imports from here. `smoothstep` in particular carries a source
//! quirk that is *not* general: its divisor is `b - a || 1e-6`, JavaScript
//! falsiness, which substitutes only for an exact zero. A naive
//! `(b - a).max(1e-6)` would also clamp every **reversed** edge pair — and
//! `atlas.js` uses reversed pairs throughout to invert a falloff direction, so
//! that "fix" would flip their sign. It is app behaviour, and it stays here.

use axiom_kernel::Ratio;
use axiom_math::DVec2;
use axiom_noise::{
    perlin_2d, perlin_fbm_2d, perlin_ridged_2d, perlin_warped_2d, worley_edge, worley_f1,
    Lacunarity, PermutationLattice, WarpStrength,
};

use crate::rng::Rng;

/// Per-octave frequency drift for [`Noise::fbm`] and [`Noise::warped`]
/// (`noise.js:65-77`). Every call site in the source passes only `(x, y, oct)`,
/// so the defaults are never overridden and are named here rather than threaded
/// as parameters.
fn fbm_lacunarity() -> Lacunarity {
    Lacunarity::new(2.03).unwrap_or(Lacunarity::DOUBLING)
}

/// Per-octave frequency drift for [`Noise::ridged`] (`noise.js:80-91`).
/// Deliberately different from [`fbm_lacunarity`]: the two fields are layered
/// on top of each other in several atlas recipes, and a shared drift would let
/// their octaves land on the same lattice planes and beat against each other.
fn ridged_lacunarity() -> Lacunarity {
    Lacunarity::new(2.11).unwrap_or(Lacunarity::DOUBLING)
}

/// The identity warp. A non-finite strength cannot displace anything
/// meaningfully, so it falls back to leaving the sample where it is rather
/// than to an arbitrary magnitude.
fn unwarped() -> WarpStrength {
    WarpStrength::new(0.0).unwrap_or_else(|_| unreachable!("zero is finite"))
}

/// Per-octave amplitude falloff, shared by all three fractal forms.
fn gain() -> Ratio {
    Ratio::finite_or_zero(0.5)
}

/// The permutation table's period, and the count of jittered feature points.
const PERIOD: usize = 256;

/// Fisher-Yates over `0..256`, descending, then one jittered feature point per
/// entry — `noise.js:19-33`, draw for draw.
///
/// The whole of the seeding seam. It lives here and not in the engine because
/// the *sequence* is this game's contract: 255 integer draws then 512 unit
/// draws, in that order, off the same `Rng` every other fx subsystem shares.
fn seeded_table(rng: &mut Rng) -> ([u8; PERIOD], [DVec2; PERIOD]) {
    let mut table: [u8; PERIOD] = core::array::from_fn(|i| i as u8);
    for i in (1..PERIOD).rev() {
        let j = rng.int(0, i as i32) as usize;
        table.swap(i, j);
    }
    let features = core::array::from_fn(|_| {
        let x = rng.float();
        let y = rng.float();
        DVec2::new(x, y)
    });
    (table, features)
}

pub use axiom_math::clamp01;

/// `smoothstep(a, b, x)`, `noise.js:154-157`.
///
/// **Source quirk, preserved exactly:** the divisor is `b - a || 1e-6` — JS
/// `||` only falls back when `b - a` is exactly `0` (falsy), not when it is
/// merely small or negative. See the module doc for why the obvious "fix"
/// would be a behaviour change.
pub use axiom_math::smoothstep;

/// sRGB encode for atlases sampled as sRGB textures. `noise.js:161-164`.
pub fn encode_srgb(v: f64) -> f64 {
    let v = clamp01(v);
    let below = f64::from(u8::from(v <= 0.0031308));
    let low = v * 12.92;
    let high = 1.055 * v.powf(1.0 / 2.4) - 0.055;
    below * low + (1.0 - below) * high
}

/// The CPU noise toolkit, `class Noise` (`noise.js:16-142`) — a thin binding
/// over [`PermutationLattice`] that keeps the source's method names and its
/// baked-in fractal constants.
pub struct Noise {
    lattice: PermutationLattice,
}

impl Noise {
    /// `constructor(rng)`, `noise.js:17-33`. Consumes `rng` in the source's
    /// exact draw order — see the module doc.
    pub fn new(rng: &mut Rng) -> Self {
        let (table, features) = seeded_table(rng);
        Noise {
            lattice: PermutationLattice::from_table(table, features),
        }
    }

    /// Perlin gradient noise, roughly `-1..1`. `noise.js:43-59`.
    pub fn perlin(&self, x: f64, y: f64) -> f64 {
        perlin_2d(&self.lattice, DVec2::new(x, y)).get()
    }

    /// fBm in `0..1`. `noise.js:65-77`.
    pub fn fbm(&self, x: f64, y: f64, oct: i32) -> f64 {
        perlin_fbm_2d(
            &self.lattice,
            DVec2::new(x, y),
            oct.max(0) as u32,
            fbm_lacunarity(),
            gain(),
        )
        .get()
    }

    /// Ridged multifractal in `0..1` — veins, cracks, filaments.
    /// `noise.js:80-91`.
    pub fn ridged(&self, x: f64, y: f64, oct: i32) -> f64 {
        perlin_ridged_2d(
            &self.lattice,
            DVec2::new(x, y),
            oct.max(0) as u32,
            ridged_lacunarity(),
            gain(),
        )
        .get()
    }

    /// Domain-warped fBm. `noise.js:94-98`.
    pub fn warped(&self, x: f64, y: f64, warp: f64, oct: i32) -> f64 {
        perlin_warped_2d(
            &self.lattice,
            DVec2::new(x, y),
            WarpStrength::new(warp as f32).unwrap_or(unwarped()),
            oct.max(0) as u32,
            fbm_lacunarity(),
            gain(),
        )
        .get()
    }

    /// F1 Worley distance, `0..~1`. `noise.js:101-117`.
    pub fn worley(&self, x: f64, y: f64) -> f64 {
        worley_f1(&self.lattice, DVec2::new(x, y)).get()
    }

    /// F2-F1 Worley — cell walls, i.e. crack networks. `noise.js:120-138`.
    pub fn worley_edge(&self, x: f64, y: f64) -> f64 {
        worley_edge(&self.lattice, DVec2::new(x, y)).get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothstep_endpoints() {
        assert_eq!(smoothstep(0.0, 1.0, 0.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 1.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
    }

    /// The quirk the module doc argues for: a reversed edge pair inverts the
    /// ramp rather than being clamped up to a positive divisor.
    #[test]
    fn a_reversed_edge_pair_inverts_the_ramp() {
        assert_eq!(smoothstep(1.0, 0.0, 0.0), 1.0);
        assert_eq!(smoothstep(1.0, 0.0, 1.0), 0.0);
        // Only an exact-zero span takes the fallback.
        assert_eq!(smoothstep(0.5, 0.5, 1.0), 1.0);
    }

    #[test]
    fn encode_srgb_clamps() {
        assert_eq!(encode_srgb(-1.0), 0.0);
        assert!((encode_srgb(2.0) - 1.0).abs() < 1e-15);
        // The knee: both arms agree where they meet.
        let knee = 0.0031308;
        assert!((encode_srgb(knee) - knee * 12.92).abs() < 1e-12);
    }

    #[test]
    fn noise_is_deterministic_for_a_fixed_seed() {
        let n1 = Noise::new(&mut Rng::new(1234));
        let n2 = Noise::new(&mut Rng::new(1234));
        assert_eq!(n1.perlin(1.7, 3.1), n2.perlin(1.7, 3.1));
        assert_eq!(n1.worley(2.2, -1.4), n2.worley(2.2, -1.4));
        assert_eq!(n1.worley_edge(2.2, -1.4), n2.worley_edge(2.2, -1.4));
    }

    /// The seeding seam actually consumes this crate's generator: a different
    /// seed must give a different field.
    #[test]
    fn a_different_seed_gives_a_different_field() {
        let a = Noise::new(&mut Rng::new(1));
        let b = Noise::new(&mut Rng::new(2));
        assert_ne!(a.perlin(1.7, 3.1), b.perlin(1.7, 3.1));
    }

    /// The draw order is the contract: after building a lattice the generator
    /// must have advanced by exactly 255 integer draws plus 512 unit draws.
    #[test]
    fn the_lattice_consumes_the_documented_number_of_draws() {
        let mut used = Rng::new(7);
        let _ = Noise::new(&mut used);

        let mut counted = Rng::new(7);
        (1..256usize)
            .rev()
            .for_each(|i| {
                let _ = counted.int(0, i as i32);
            });
        (0..512).for_each(|_| {
            let _ = counted.float();
        });

        assert_eq!(used.float(), counted.float(), "draw order diverged");
    }

    #[test]
    fn every_field_stays_in_its_documented_range() {
        let n = Noise::new(&mut Rng::new(99));
        (0..64).for_each(|i| {
            let (x, y) = (f64::from(i) * 0.53, f64::from(-i) * 0.21);
            assert!((-1.0..=1.0).contains(&n.perlin(x, y)));
            assert!((0.0..=1.0).contains(&n.fbm(x, y, 4)));
            assert!((0.0..=1.0).contains(&n.ridged(x, y, 3)));
            assert!((0.0..=1.0).contains(&n.warped(x, y, 0.5, 4)));
            assert!((0.0..=1.0).contains(&n.worley(x, y)));
            assert!((0.0..=1.0).contains(&n.worley_edge(x, y)));
        });
    }

    /// The two fractal forms use different drift constants on purpose; a shared
    /// one would let their octaves beat against each other where the atlas
    /// recipes layer them.
    #[test]
    fn the_two_fractal_forms_use_different_drift() {
        assert_ne!(fbm_lacunarity().get(), ridged_lacunarity().get());
    }
}
