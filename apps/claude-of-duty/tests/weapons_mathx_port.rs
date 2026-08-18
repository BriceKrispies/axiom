//! The ported viewmodel math kit, pinned against the JavaScript it came from.
//!
//! Every `expected` value here was captured by running the original
//! `C:/dev/Claude-of-Duty/src/weapons/mathx.js` under Node (v24) and printing
//! `JSON.stringify(..., null, 2)`. They are golden values, not
//! recomputations — see `tests/core_port.rs` for the same discipline applied
//! to `rng.js`.

use axiom_claude_of_duty::rng::Rng;
use axiom_claude_of_duty::weapons::mathx::{
    clamp, clamp01, damp, ease_in_cubic, ease_in_out_sine, ease_out_back, ease_out_cubic, lerp,
    smoothstep, smootherstep, wrap_pi, Noise1, Spring, Spring3, DEG, EASE_OUT_BACK_DEFAULT_K, TAU,
};

/// `sin`/`cos`/`ln`/`sqrt`/`exp` are not bit-guaranteed across libm
/// implementations, so anything built from them is compared within an
/// absolute tolerance rather than exactly — the `1e-12` figure established in
/// `tests/core_port.rs`.
fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "expected {expected:.17}, got {actual:.17}"
    );
}

#[test]
fn constants_match_the_source() {
    assert_eq!(TAU, 6.283185307179586);
    assert_eq!(DEG, 0.017453292519943295);
}

#[test]
fn clamp_and_clamp01_reproduce_the_javascript_ternary_chain() {
    let xs = [-2.0, -0.5, 0.0, 0.25, 0.5, 0.75, 1.0, 1.5, 3.0];
    let expected_clamp = [-1.0, -0.5, 0.0, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0];
    for (x, want) in xs.iter().zip(expected_clamp) {
        assert_eq!(clamp(*x, -1.0, 2.0), want);
    }

    let expected_clamp01 = [0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0];
    for (x, want) in xs.iter().zip(expected_clamp01) {
        assert_eq!(clamp01(*x), want);
    }
}

#[test]
fn lerp_reproduces_the_javascript_values() {
    let cases = [
        ((0.0, 10.0, 0.0), 0.0),
        ((0.0, 10.0, 0.25), 2.5),
        ((0.0, 10.0, 0.5), 5.0),
        ((0.0, 10.0, 1.0), 10.0),
        ((0.0, 10.0, 1.5), 15.0),
        ((-5.0, 5.0, 0.5), 0.0),
    ];
    for ((a, b, t), want) in cases {
        assert_eq!(lerp(a, b, t), want);
    }
}

#[test]
fn smoothstep_and_smootherstep_reproduce_the_javascript_curves() {
    let xs = [0.0, 0.25, 0.5, 0.75, 1.0];

    let expected_smoothstep = [0.0, 0.15625, 0.5, 0.84375, 1.0];
    for (x, want) in xs.iter().zip(expected_smoothstep) {
        assert_eq!(smoothstep(0.0, 1.0, *x), want);
    }
    // `b - a || 1e-6` — a degenerate zero-width range falls back rather than
    // dividing by zero.
    assert_eq!(smoothstep(2.0, 2.0, 2.0), 0.0);

    let expected_smootherstep = [0.0, 0.103515625, 0.5, 0.896484375, 1.0];
    for (x, want) in xs.iter().zip(expected_smootherstep) {
        assert_eq!(smootherstep(0.0, 1.0, *x), want);
    }
    assert_eq!(smootherstep(2.0, 2.0, 2.0), 0.0);
}

#[test]
fn the_easing_functions_reproduce_the_javascript_curves_at_the_five_check_points() {
    let ts = [0.0, 0.25, 0.5, 0.75, 1.0];

    assert_eq!(EASE_OUT_BACK_DEFAULT_K, 1.6);
    let expected_default_k = [0.0, 0.803125, 1.075, 1.059375, 1.0];
    for (t, want) in ts.iter().zip(expected_default_k) {
        assert_eq!(ease_out_back(*t, EASE_OUT_BACK_DEFAULT_K), want);
    }
    let expected_custom_k = [0.0, 0.9296875, 1.1875, 1.1015625, 1.0];
    for (t, want) in ts.iter().zip(expected_custom_k) {
        assert_eq!(ease_out_back(*t, 2.5), want);
    }

    let expected_out_cubic = [0.0, 0.578125, 0.875, 0.984375, 1.0];
    for (t, want) in ts.iter().zip(expected_out_cubic) {
        assert_eq!(ease_out_cubic(*t), want);
    }

    let expected_in_cubic = [0.0, 0.015625, 0.125, 0.421875, 1.0];
    for (t, want) in ts.iter().zip(expected_in_cubic) {
        assert_eq!(ease_in_cubic(*t), want);
    }

    let expected_in_out_sine = [
        0.0,
        0.1464466094067262,
        0.49999999999999994,
        0.8535533905932737,
        1.0,
    ];
    for (t, want) in ts.iter().zip(expected_in_out_sine) {
        assert_close(ease_in_out_sine(*t), want);
    }
}

#[test]
fn damp_reproduces_the_javascript_exponential_approach() {
    let cases = [
        ((10.0, 0.0, 8.0, 0.016), 8.798533791446438),
        ((10.0, 0.0, 8.0, 1.0 / 60.0), 8.751733190429475),
        ((0.0, 5.0, 4.0, 0.1), 1.6483997698218031),
        // current == target: the exponential term is multiplied by zero, so
        // this lands on the target exactly regardless of libm.
        ((1.0, 1.0, 8.0, 0.016), 1.0),
    ];
    for ((current, target, rate, dt), want) in cases {
        assert_close(damp(current, target, rate, dt), want);
    }
}

#[test]
fn wrap_pi_reproduces_the_javascript_angle_wrap() {
    let cases = [
        (-10.0, 2.5663706143591725),
        (-std::f64::consts::PI - 0.1, 3.0415926535897935),
        (-std::f64::consts::PI, -3.141592653589793),
        (0.0, 0.0),
        (std::f64::consts::PI, -3.141592653589793),
        (std::f64::consts::PI + 0.1, -3.0415926535897935),
        (10.0, -2.5663706143591725),
        (TAU * 3.0 + 0.4, 0.3999999999999986),
    ];
    for (a, want) in cases {
        assert_close(wrap_pi(a), want);
    }
}

#[test]
fn spring_step_reproduces_the_javascript_critically_damped_trace() {
    let mut s = Spring::new(12.0, 1.0, 0.0);
    let dts = [0.016, 0.016, 0.016, 0.016, 0.016, 0.1, 0.016, 0.016];
    let targets = [1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 0.0, 0.0];
    let expected = [
        (0.29895435542369686, 18.684647213981055),
        (0.5699462030997883, 16.936990479755714),
        (0.7541797997373662, 11.514599789848615),
        (0.8655140817128892, 6.958392623470196),
        (1.2275438745630585, 22.62686205313557),
        (1.8607111806555767, 6.331673060925183),
        (1.3252539038652018, -33.46607979939843),
        (0.8190698520828371, -31.63650323639779),
    ];
    for (i, (dt, target)) in dts.iter().zip(targets).enumerate() {
        let x = s.step(*dt, target);
        assert_close(x, expected[i].0);
        assert_close(s.v, expected[i].1);
    }
}

#[test]
fn spring_step_reproduces_the_javascript_lively_trace_with_a_recoil_kick() {
    // f=10, z=0.5 is underdamped ("lively"): it overshoots. `kick` is the
    // recoil-impulse path — an instantaneous velocity add mid-sequence.
    let mut s = Spring::new(10.0, 0.5, 0.0);
    let expected = [
        (0.3351000839261837, 20.94375524538648),
        (0.6690171354509677, 20.869815720299005),
        (0.8906462972885844, 13.851822614851038),
        (1.000776246647992, 6.883121834962972),
        (1.0370318804528502, 2.265977112803649),
        (1.0366437637802992, -0.024257292034438414),
        (0.9977101718417651, -2.4333494961583826),
        (0.985568294103235, -0.7588673586581348),
        (0.9863784812175952, 0.05063669464751621),
        (0.991211686802142, 0.3020753490341724),
        (0.9957591958115091, 0.2842193130854404),
        (0.9986881058428436, 0.1830568769584118),
    ];
    for i in 0..6 {
        let x = s.step(0.016, 1.0);
        assert_close(x, expected[i].0);
        assert_close(s.v, expected[i].1);
    }
    s.kick(-5.0);
    for i in 6..12 {
        let x = s.step(0.016, 1.0);
        assert_close(x, expected[i].0);
        assert_close(s.v, expected[i].1);
    }
}

#[test]
fn spring_set_reproduces_the_javascript_snap_and_default_matches_the_source_defaults() {
    let mut s = Spring::new(12.0, 1.0, 3.0);
    s.set(-2.0);
    assert_eq!((s.x, s.v, s.target), (-2.0, 0.0, -2.0));

    // `new Spring()` — f=12, z=1, value=0.
    let default = Spring::default();
    assert_eq!((default.f, default.z, default.x, default.v, default.target), (12.0, 1.0, 0.0, 0.0, 0.0));
}

#[test]
fn spring_step_to_target_advances_without_a_supplied_target() {
    // `step(dt)` with the defaulted `target = this.target` the source allows
    // and Rust's signature does not; `step_to_target` is the port's stand-in.
    let mut a = Spring::new(12.0, 1.0, 0.0);
    let mut b = Spring::new(12.0, 1.0, 0.0);
    a.step(0.016, 1.0);
    b.step(0.016, 1.0);
    let x_a = a.step_to_target(0.016);
    let x_b = b.step(0.016, 1.0);
    assert_eq!(x_a, x_b);
    assert_eq!(a.target, b.target);
}

#[test]
fn spring3_step_reproduces_the_javascript_trace() {
    let mut s3 = Spring3::new(12.0, 1.0);
    let dts = [0.016, 0.016, 0.016, 0.1, 0.016];
    let expected = [
        (0.29895435542369686, -0.5979087108473937, 0.14947717771184843),
        (0.5699462030997883, -1.1398924061995765, 0.28497310154989414),
        (0.7541797997373662, -1.5083595994747323, 0.3770898998686831),
        (0.9615892419012858, -1.9231784838025716, 0.4807946209506429),
        (0.9798892724133613, -1.9597785448267226, 0.48994463620668066),
    ];
    for (dt, want) in dts.iter().zip(expected) {
        s3.step(*dt, 1.0, -2.0, 0.5);
        assert_close(s3.x(), want.0);
        assert_close(s3.y(), want.1);
        assert_close(s3.z(), want.2);
    }

    s3.kick(1.0, 2.0, 3.0);
    assert_close(s3.a.v, 2.143751907004721);
    assert_close(s3.b.v, -0.2875038140094426);
    assert_close(s3.c.v, 3.5718759535023605);

    s3.reset();
    assert_eq!((s3.x(), s3.y(), s3.z()), (0.0, 0.0, 0.0));
}

#[test]
fn spring3_write_to_and_f_getter_reproduce_the_javascript_values() {
    let mut s3 = Spring3::new(8.0, 0.7);
    s3.step(0.016, 1.0, 2.0, 3.0);

    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    s3.write_to(&mut x, &mut y, &mut z, 2.0);
    assert_close(x, 0.46654893596946007);
    assert_close(y, 0.9330978719389201);
    assert_close(z, 1.39964680790838);

    assert_eq!(s3.f(), 8.0);
    // Default constructor matches `new Spring3()` (f=12, z=1).
    let default = Spring3::default();
    assert_eq!((default.f(), default.a.z), (12.0, 1.0));
}

#[test]
fn spring3_z_source_quirk_the_getter_and_setter_disagree() {
    // The class body declares `get z()` twice; the later declaration
    // (returning `this.c.x`, the z-position) wins on the prototype, shadowing
    // the earlier one (`this.a.z`, the damping ratio). Only one `set z()`
    // exists, and it still writes the damping ratio. `viewmodel.js` depends on
    // exactly this split (`this.recPos.z = r.damping` to set, later
    // `pz += this.recPos.z` to read the position back), so the port keeps the
    // same split rather than unifying it.
    let mut s3 = Spring3::new(8.0, 0.7);
    s3.step(0.016, 1.0, 2.0, 3.0);

    // Reading `z()` gives the position (c.x), NOT the damping ratio (0.7).
    assert_close(s3.z(), 0.69982340395419);
    assert_ne!(s3.z(), s3.a.z);

    // Writing via `set_z` still changes the damping ratio on all three
    // springs, unaffected by the getter's shadowing.
    s3.set_z(0.9);
    assert_eq!((s3.a.z, s3.b.z, s3.c.z), (0.9, 0.9, 0.9));
    // ... and the position getter is untouched by that write.
    assert_close(s3.z(), 0.69982340395419);
}

#[test]
fn noise1_reproduces_the_javascript_field_at_fixed_inputs() {
    let mut rng = Rng::new(12345);
    let noise = Noise1::new(&mut rng, 512);

    let xs = [0.0, 0.5, 1.0, 3.25, 10.1, 100.75, 511.9, 512.5, -3.2];
    let expected_at = [
        -0.39494550228118896,
        -0.45178989320993423,
        -0.3899098038673401,
        0.7961459022480994,
        0.40153769829124225,
        -0.23469366953941062,
        -0.3803952043652494,
        -0.45178989320993423,
        0.0020772558450699724,
    ];
    for (x, want) in xs.iter().zip(expected_at) {
        assert_close(noise.at(*x), want);
    }
    // x=0.5 and x=512.5 land on the same table cell (512.5 wraps to the same
    // fractional offset within a 512-entry table) and agree exactly — the
    // wraparound is the point of a looping table.
    assert_eq!(noise.at(0.5), noise.at(512.5));

    let expected_fbm_default = [
        -0.2926663166829996,
        -0.4366633497452816,
        -0.4770526194602273,
        0.3090508839963085,
        0.12753002494054752,
        -0.14018778094684176,
        -0.10867858448920553,
        -0.1709689030254428,
        0.1447571495751485,
    ];
    for (x, want) in xs.iter().zip(expected_fbm_default) {
        assert_close(noise.fbm(*x, 3, 0.5), want);
    }

    let expected_fbm_custom = [
        -0.19401836535644815,
        -0.310108809482648,
        -0.40633086522777606,
        0.2046254049284228,
        0.14410080813519266,
        -0.16062702137376045,
        -0.07424346272721641,
        -0.11609478445276555,
        0.14126492038666547,
    ];
    for (x, want) in xs.iter().zip(expected_fbm_custom) {
        assert_close(noise.fbm(*x, 5, 0.6), want);
    }

    // The smoothed table's head and tail, recovered via `at()` at integer
    // points where the Catmull-Rom spline evaluates to the table entry
    // exactly (t=0 collapses the cubic to `0.5 * 2 * b == b`) — this pins the
    // RNG draw order (512 `signed()` draws) and the wraparound smoothing pass.
    let expected_head = [
        -0.39494550228118896,
        -0.3899098038673401,
        0.33754634857177734,
        0.8076438903808594,
        0.5100755095481873,
    ];
    for (i, want) in expected_head.iter().enumerate() {
        assert_eq!(noise.at(i as f64), *want);
    }
    let expected_tail = [
        0.570725679397583,
        0.34283196926116943,
        -0.07115491479635239,
        -0.28916215896606445,
        -0.17260581254959106,
    ];
    for (i, want) in expected_tail.iter().enumerate() {
        assert_eq!(noise.at((507 + i) as f64), *want);
    }

    // `size`/`t` are public, matching the source's plain instance fields.
    assert_eq!(noise.size, 512);
    assert_eq!(noise.t.len(), 512);
}

#[test]
fn noise1_fbm_falls_back_to_a_norm_of_one_when_oct_is_zero() {
    // `norm || 1` — with zero octaves the accumulator never moves off zero,
    // so the division-by-zero fallback is what keeps this finite.
    let mut rng = Rng::new(1);
    let noise = Noise1::new(&mut rng, 512);
    assert_eq!(noise.fbm(3.0, 0, 0.5), 0.0);
}
