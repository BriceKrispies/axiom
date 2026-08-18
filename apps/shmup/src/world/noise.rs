//! Ported from Claude-of-Duty `src/world/util.js:28-77` (`hash3`, `fade`, `noise3`, `fbm3`).
//!
//! The positional noise basis every geometry builder in `src/world/util.js` sits
//! on: a pure integer hash of a 3D lattice point, value-noise interpolation over
//! it, and fractal-Brownian summation of octaves of that noise. All three are
//! **position-deterministic** — a function of `(x, y, z)` alone, with no RNG
//! stream threaded through them — which is exactly why editing an unrelated
//! system (say, adding a draw to some other builder's `Rng` fork) cannot
//! reshuffle the wear pattern on a wall: the wall's noise never consulted the
//! RNG in the first place.
//!
//! `chamferBox`, `weatherProp`, `wallPanel` and the rest of `util.js`'s geometry
//! builders that *consume* this basis are out of scope for this port — they
//! build `THREE.BufferGeometry`/`THREE.Shape`/`ExtrudeGeometry`, which belongs
//! with the geometry back end arriving in the Assembler port. This module is
//! the maths underneath them, portable on its own.

/// Smoothstep-style fade curve used to interpolate between lattice corners.
/// Not exported by the source (`util.js:38-40`); kept private here too.
fn fade(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// `Math.round` rounds half-way values toward `+Infinity` (`Math.round(-1.5) ==
/// -1`), unlike Rust's `f64::round` which rounds half away from zero
/// (`(-1.5_f64).round() == -2.0`). `(v + 0.5).floor()` reproduces the JS rule
/// exactly for every finite `v`.
fn round_half_up(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// `Math.round(v) | 0`-style narrowing to the 32-bit int JS's `^`/`Math.imul`
/// operate on. Rust's `f64 as i32` **saturates** on overflow where JS's
/// `ToInt32` **wraps** modulo 2^32; the two diverge only once `|v| >= 2^31`
/// (`v * 31337` would need `|v| >~ 68000`), far outside any world coordinate
/// `hash3` is ever called with, so the divergence is unreachable in practice
/// and left uncorrected rather than adding dead-code wraparound handling.
fn round_half_up_bits(v: f64) -> u32 {
    round_half_up(v) as i32 as u32
}

/// Deterministic 3D value hash in `[0,1)`. No `Math.random` anywhere.
///
/// Ported from `util.js:30-36`, bit-for-bit: every intermediate is 32-bit
/// wrapping arithmetic (`Math.imul` == [`u32::wrapping_mul`], `^` == `^` on the
/// same bit pattern, `>>>` == [`u32`]'s logical `>>`).
pub fn hash3(x: f64, y: f64, z: f64) -> f64 {
    let mut h = (round_half_up_bits(x * 1013.0) ^ 0x27d4_eb2d).wrapping_mul(0x85eb_ca6b);
    h = (h ^ round_half_up_bits(y * 1619.0)).wrapping_mul(0xc2b2_ae35);
    h = (h ^ round_half_up_bits(z * 31337.0)).wrapping_mul(0x27d4_eb2f);
    h ^= h >> 15;
    f64::from(h) / 4294967296.0
}

/// Smooth value noise, period ~1 unit. Ported from `util.js:43-62`.
pub fn noise3(x: f64, y: f64, z: f64) -> f64 {
    let xi = x.floor();
    let yi = y.floor();
    let zi = z.floor();
    let xf = fade(x - xi);
    let yf = fade(y - yi);
    let zf = fade(z - zi);
    let mut acc = 0.0;
    for dz in 0..2 {
        let wz = if dz == 1 { zf } else { 1.0 - zf };
        for dy in 0..2 {
            let wy = if dy == 1 { yf } else { 1.0 - yf };
            for dx in 0..2 {
                let wx = if dx == 1 { xf } else { 1.0 - xf };
                acc += hash3(xi + f64::from(dx), yi + f64::from(dy), zi + f64::from(dz)) * wx * wy * wz;
            }
        }
    }
    acc
}

/// The source's defaulted `fbm3(x, y, z, octaves = 3)` (`util.js:64`). Rust has
/// no default arguments; call sites that want the source's default pass this
/// constant explicitly.
pub const FBM3_DEFAULT_OCTAVES: u32 = 3;

/// Fractal Brownian motion: `octaves` layers of [`noise3`], each half the
/// amplitude and roughly double the frequency of the last, normalised back to
/// `[0,1]`-ish range. Ported from `util.js:64-77`.
pub fn fbm3(x: f64, y: f64, z: f64, octaves: u32) -> f64 {
    let mut a = 0.5;
    let mut sum = 0.0;
    let mut norm = 0.0;
    let mut x = x;
    let mut y = y;
    let mut z = z;
    for _ in 0..octaves {
        sum += noise3(x, y, z) * a;
        norm += a;
        a *= 0.5;
        x *= 2.03;
        y *= 2.01;
        z *= 1.97;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points captured by running the original `hash3`/`noise3`/`fbm3` from
    /// `C:/dev/Claude-of-Duty/src/world/util.js` under Node 24
    /// (`toPrecision(17)`), spanning zero, unit, fractional, large, negative and
    /// irrational inputs. These are golden values, not recomputations: a future
    /// edit to this file that changes one of them has silently stopped being the
    /// source's noise basis, and every wear/grime pattern downstream has moved.
    const POINTS: [(f64, f64, f64); 10] = [
        (0.0, 0.0, 0.0),
        (1.0, 1.0, 1.0),
        (0.5, 0.5, 0.5),
        (-1.5, 2.25, -3.75),
        (10.1, -4.2, 7.7),
        (0.001, 0.002, 0.003),
        (100.0, 200.0, 300.0),
        (-0.5, -0.5, -0.5),
        (3.14159, 2.71828, 1.41421),
        (-7.3, 0.0, 5.5),
    ];

    /// `hash3` is built entirely from `Math.round` (exact for these inputs),
    /// `Math.imul` and bitwise ops — all exact 32-bit integer arithmetic with no
    /// transcendental involved — so the golden comparison is exact equality.
    #[test]
    fn hash3_matches_the_javascript_exactly() {
        let expected = [
            0.805_187_729_885_801_67,
            0.018_226_084_765_046_835,
            0.685_069_836_676_120_76,
            0.157_002_225_751_057_27,
            0.721_455_277_642_235_16,
            0.567_252_699_052_914_98,
            0.940_780_254_779_383_54,
            0.762_639_577_733_352_78,
            0.613_841_699_436_306_95,
            0.088_863_741_839_304_566,
        ];
        for ((x, y, z), want) in POINTS.into_iter().zip(expected) {
            assert_eq!(hash3(x, y, z), want, "hash3({x}, {y}, {z})");
        }
    }

    /// `noise3` interpolates `hash3` outputs with `fade`, which is pure `+ - *`
    /// arithmetic — still exact.
    #[test]
    fn noise3_matches_the_javascript_exactly() {
        let expected = [
            0.805_187_729_885_801_67,
            0.018_226_084_765_046_835,
            0.409_660_508_652_450_52,
            0.745_753_695_692_883_41,
            0.570_969_519_622_206_32,
            0.805_160_741_588_014_21,
            0.940_780_254_779_383_54,
            0.591_920_981_038_128_96,
            0.617_810_163_977_083_77,
            0.177_105_942_103_080_39,
        ];
        for ((x, y, z), want) in POINTS.into_iter().zip(expected) {
            assert_eq!(noise3(x, y, z), want, "noise3({x}, {y}, {z})");
        }
    }

    /// `fbm3` additionally divides by an accumulated `norm` — still only `+ - *
    /// /`, so still exact equality rather than a tolerance.
    #[test]
    fn fbm3_default_octaves_matches_the_javascript_exactly() {
        let expected = [
            0.805_187_729_885_801_67,
            0.192_657_311_317_080_58,
            0.278_884_401_711_962_88,
            0.625_255_407_693_224_74,
            0.621_788_564_649_644_94,
            0.805_082_710_812_823_34,
            0.756_471_461_151_374_65,
            0.486_284_579_323_641_61,
            0.514_793_247_386_824_30,
            0.383_661_136_566_716_58,
        ];
        for ((x, y, z), want) in POINTS.into_iter().zip(expected) {
            assert_eq!(
                fbm3(x, y, z, FBM3_DEFAULT_OCTAVES),
                want,
                "fbm3({x}, {y}, {z}, 3)"
            );
        }
    }

    /// `octaves = 1` is `fbm3` degenerating to a single `noise3` call (the loop
    /// runs once, `sum/norm` is `noise3 * 0.5 / 0.5`) — pins that the loop
    /// bounds and the accumulator both ported correctly at the edge.
    #[test]
    fn fbm3_one_octave_matches_the_javascript_and_equals_noise3() {
        let expected = [
            0.805_187_729_885_801_67,
            0.018_226_084_765_046_835,
            0.409_660_508_652_450_52,
            0.745_753_695_692_883_41,
            0.570_969_519_622_206_32,
            0.805_160_741_588_014_21,
            0.940_780_254_779_383_54,
            0.591_920_981_038_128_96,
            0.617_810_163_977_083_77,
            0.177_105_942_103_080_39,
        ];
        for ((x, y, z), want) in POINTS.into_iter().zip(expected) {
            assert_eq!(fbm3(x, y, z, 1), want, "fbm3({x}, {y}, {z}, 1)");
            assert_eq!(fbm3(x, y, z, 1), noise3(x, y, z));
        }
    }

    /// `octaves = 5` exercises the loop beyond the source's default, including
    /// the `x *= 2.03 / y *= 2.01 / z *= 1.97` frequency drift compounding
    /// further than any other pinned case.
    #[test]
    fn fbm3_five_octaves_matches_the_javascript_exactly() {
        let expected = [
            0.805_187_729_885_801_67,
            0.216_156_577_397_796_62,
            0.312_044_709_767_078_04,
            0.621_293_327_180_270_64,
            0.594_136_939_024_982_99,
            0.804_785_650_436_944_61,
            0.737_018_708_822_944_73,
            0.513_473_852_883_690_85,
            0.509_707_790_082_222_33,
            0.409_937_853_639_993_85,
        ];
        for ((x, y, z), want) in POINTS.into_iter().zip(expected) {
            assert_eq!(fbm3(x, y, z, 5), want, "fbm3({x}, {y}, {z}, 5)");
        }
    }

    /// `Math.round`'s half-up rule diverges from Rust's half-away-from-zero
    /// `f64::round` exactly at `.5` boundaries — this is the source-quirk that
    /// `round_half_up` exists to reproduce, pinned directly rather than only
    /// indirectly through `hash3`.
    #[test]
    fn round_half_up_matches_javascripts_math_round_at_the_half_boundary() {
        assert_eq!(round_half_up(-1.5), -1.0); // Math.round(-1.5) === -1
        assert_eq!(round_half_up(-2.5), -2.0); // Math.round(-2.5) === -2
        assert_eq!(round_half_up(1.5), 2.0); // Math.round(1.5) === 2
        assert_eq!(round_half_up(0.5), 1.0); // Math.round(0.5) === 1
        assert_eq!(round_half_up(-0.5), 0.0); // Math.round(-0.5) === -0 (sign is unobservable through round_half_up_bits)
    }

    #[test]
    fn fade_is_the_smoothstep_polynomial() {
        assert_eq!(fade(0.0), 0.0);
        assert_eq!(fade(1.0), 1.0);
        assert_eq!(fade(0.5), 0.5);
    }
}
