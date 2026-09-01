//! The world's positional noise basis — **the engine's**, bound to this game's
//! constants.
//!
//! The algorithm used to live here, ported from Claude-of-Duty
//! `src/world/util.js:28-77` (`hash3`, `fade`, `noise3`, `fbm3`). It is now
//! `axiom_noise`'s positional value basis — [`axiom_noise::hash_01`],
//! [`axiom_noise::value_noise_01`], [`axiom_noise::value_fbm_01`] — promoted
//! into the layer under the Branchless and Coverage Laws, and pinned there
//! against the same Node-captured goldens that pinned it here
//! (`crates/axiom-noise/tests/positional_basis_golden.rs`, asserted with exact
//! equality). See `docs/work-manifests/shmup-promotion/00-manifest.md`.
//!
//! What stays here is what is genuinely this game's: **the constants**. The
//! per-axis frequency drift and the amplitude gain are the identity of *this*
//! world's surface variation — change them and every wall, road and prop
//! reskins — so they are content, not capability, and the composition leaf is
//! where content lives. The four-scalar call shape stays too, because ~50 call
//! sites across `world/` read as `fbm3(x * 0.5 + 3.7, y * 0.42, 0.5, 2)` and
//! rewriting each to build a vector would be churn that makes them harder, not
//! easier, to diff against the source.
//!
//! The basis remains **position-deterministic** — a function of `(x, y, z)`
//! alone, with no RNG stream threaded through it. That is why adding a draw to
//! some other subsystem's `Rng` fork cannot reshuffle the wear pattern on a
//! wall: the wall's noise never consulted the RNG in the first place. The layer
//! documents that property as the reason the basis exists alongside the seeded
//! gradient one.

use axiom_kernel::Ratio;
use axiom_math::DVec3;
use axiom_noise::value_fbm_01;

/// Per-octave frequency drift, per axis (`util.js:70-72`).
///
/// Three factors near — but deliberately not on — `2.0`. A uniform doubling
/// would land every octave on the same lattice planes and grid the surface up
/// visibly; these walk the octaves off each other.
const DRIFT: DVec3 = DVec3::new(2.03, 2.01, 1.97);

/// Per-octave amplitude falloff (`util.js:66,69`). Halving.
///
/// The source starts its amplitude at `0.5` as well; that starting value is
/// unobservable, because the sum is normalised by the accumulated amplitude and
/// a common factor cancels exactly. See [`axiom_noise::value_fbm_01`].
fn gain() -> Ratio {
    Ratio::finite_or_zero(0.5)
}

/// The source's defaulted `fbm3(x, y, z, octaves = 3)` (`util.js:64`). Rust has
/// no default arguments; call sites that want the source's default pass this
/// constant explicitly.
pub const FBM3_DEFAULT_OCTAVES: u32 = 3;

/// Fractal Brownian motion over the engine's positional value basis, in
/// `[0, 1]`.
///
/// The `f64` in and out is deliberate and matches the layer: this drives
/// geometry displacement and texture masks at bake time, where `f32` rounding
/// is observable in the mesh. Callers that need an `f32` narrow at their own
/// boundary, which several do with an explicit `as f32`.
pub fn fbm3(x: f64, y: f64, z: f64, octaves: u32) -> f64 {
    value_fbm_01(DVec3::new(x, y, z), octaves, DRIFT, gain()).get()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The binding, not the basis: that this file still hands the layer *this
    /// world's* constants. The basis itself is pinned against the JavaScript in
    /// `crates/axiom-noise/tests/positional_basis_golden.rs`; re-asserting those
    /// values here would be a second copy of the same goldens, which is how two
    /// pins drift apart.
    ///
    /// These five are the reference's own `fbm3(x, y, z, 3)` outputs, so a
    /// binding that passed the wrong drift, the wrong gain or the axes in the
    /// wrong order fails here even though the layer is correct.
    #[test]
    fn the_binding_reproduces_the_sources_field() {
        let cases = [
            ((0.0, 0.0, 0.0), 0.805_187_729_885_801_67),
            ((1.0, 1.0, 1.0), 0.192_657_311_317_080_58),
            ((0.5, 0.5, 0.5), 0.278_884_401_711_962_88),
            ((-1.5, 2.25, -3.75), 0.625_255_407_693_224_74),
            ((10.1, -4.2, 7.7), 0.621_788_564_649_644_94),
        ];
        for ((x, y, z), want) in cases {
            assert_eq!(
                fbm3(x, y, z, FBM3_DEFAULT_OCTAVES),
                want,
                "fbm3({x}, {y}, {z}, 3)"
            );
        }
    }

    /// The axes are not interchangeable — the drift differs per axis, so a
    /// binding that transposed them would still look plausible.
    #[test]
    fn the_axes_are_not_interchangeable() {
        assert_ne!(fbm3(1.0, 2.0, 3.0, 4), fbm3(3.0, 2.0, 1.0, 4));
    }

    #[test]
    fn the_field_is_position_deterministic() {
        assert_eq!(fbm3(2.5, -1.0, 0.25, 3), fbm3(2.5, -1.0, 0.25, 3));
    }
}
