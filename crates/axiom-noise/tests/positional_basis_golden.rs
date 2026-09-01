//! The positional value-noise basis, pinned against the JavaScript it was
//! promoted from.
//!
//! These are golden values, captured by running the original `hash3`/`noise3`/
//! `fbm3` from `C:/dev/Claude-of-Duty/src/world/util.js` under Node 24
//! (`toPrecision(17)`) — the reference `apps/axiom-shmup` ported and that this
//! layer now owns. They span zero, unit, fractional, large, negative and
//! irrational inputs.
//!
//! **Asserted with `assert_eq!`, not a tolerance.** The whole basis is built
//! from `+ - *`, exact 32-bit integer arithmetic, and a division by a power of
//! two: no transcendental, nothing that rounds. A tolerance here would be
//! slack this code has not earned and would hide exactly the drift the goldens
//! exist to catch.
//!
//! An edit that changes one of these has silently stopped being this basis, and
//! every field built on it has moved.

use axiom_kernel::Ratio;
use axiom_math::DVec3;
use axiom_noise::{hash_01, value_fbm_01, value_noise_01};

/// The reference's per-axis frequency drift and amplitude gain.
fn drift() -> DVec3 {
    DVec3::new(2.03, 2.01, 1.97)
}

fn half() -> Ratio {
    Ratio::new(0.5).unwrap()
}

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

fn each(expected: [f64; 10], f: impl Fn(DVec3) -> f64, label: &str) {
    POINTS
        .into_iter()
        .zip(expected)
        .for_each(|((x, y, z), want)| {
            let got = f(DVec3::new(x, y, z));
            assert_eq!(got, want, "{label}({x}, {y}, {z})");
        });
}

#[test]
fn hash_01_matches_the_javascript_exactly() {
    each(
        [
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
        ],
        |p| hash_01(p).get(),
        "hash_01",
    );
}

#[test]
fn value_noise_01_matches_the_javascript_exactly() {
    each(
        [
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
        ],
        |p| value_noise_01(p).get(),
        "value_noise_01",
    );
}

#[test]
fn value_fbm_01_at_three_octaves_matches_the_javascript_exactly() {
    each(
        [
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
        ],
        |p| value_fbm_01(p, 3, drift(), half()).get(),
        "value_fbm_01/3",
    );
}

#[test]
fn value_fbm_01_at_one_octave_matches_the_javascript_and_the_base_noise() {
    each(
        [
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
        ],
        |p| value_fbm_01(p, 1, drift(), half()).get(),
        "value_fbm_01/1",
    );
}

/// Five octaves compounds the per-axis drift further than any other pinned
/// case, so it is the one that would catch a lacunarity applied in the wrong
/// order or to the wrong axis.
#[test]
fn value_fbm_01_at_five_octaves_matches_the_javascript_exactly() {
    each(
        [
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
        ],
        |p| value_fbm_01(p, 5, drift(), half()).get(),
        "value_fbm_01/5",
    );
}
