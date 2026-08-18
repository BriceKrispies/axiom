//! The ported noise library, pinned against the JavaScript it came from.
//!
//! Every `expected` value here was captured by transcribing the embedded GLSL
//! body of `C:/dev/Claude-of-Duty/src/materials/glsl/noise.js` to plain JS
//! doubles and running it under Node (v24) — the same discipline
//! `tests/core_port.rs` and `tests/weapons_mathx_port.rs` use, applied to
//! shader maths instead of gameplay maths. See
//! `docs/work-manifests/claude-of-duty-port/notes/materials-noise.md` for the
//! capture script and the periodicity findings below.
//!
//! ## Periodicity: what "`f(p) == f(p + per)`" actually requires
//!
//! Capturing these values surfaced a precise mathematical condition the
//! module doc states but this file pins directly: exact periodicity requires
//! **`per`'s components to be integers** (`floor(p + per) == floor(p) + per`
//! only holds for integer `per`), which matches the source's own description
//! of `per` as "period, in lattice cells" — every real call site passes an
//! integer cell count.
//!
//! Two functions need more than that:
//! - [`ow_cracks`] rescales `per` by `1.7` for its break-up mask, so *its*
//!   exact periodicity needs `per * 1.7` to also be integer. The tests below
//!   use `per = (10, 10)`, since `10 * 1.7 = 17`.
//! - [`ow_scratches`] (via [`ow_shear`]) needs `per` **square**
//!   (`per.x == per.y`) as well as integer `k`/`stretch`: shearing mixes
//!   `per.y * k` into the x-shift, which lands back on an exact lattice point
//!   of the sheared coordinate system only when `per.y * k` is an integer
//!   multiple of `per.x` — automatic when `per.x == per.y`, not otherwise.
//!   `per = (8, 6)` (used for `scratches_matches_the_javascript_samples`
//!   below) demonstrably does **not** tile exactly under a plain
//!   `p -> p + per` shift; `per = (8, 8)` does, and is what
//!   `scratches_is_periodic_under_a_square_per` uses.
//!
//! Neither of these is a bug: both generators trade a very slightly
//! non-seamless internal term for visual variety (a mask frequency that
//! isn't a harmonic of the base period; an anisotropic streak direction), and
//! the source's own comment on [`ow_shear`] already flags the `k`/`stretch`
//! half of the constraint. This file makes the full condition explicit and
//! proves it holds exactly once it is met, rather than asserting a vague
//! "close enough" periodicity.

use axiom_claude_of_duty::materials::noise::{
    gl_clamp, gl_fract, gl_mix, gl_mod, gl_smoothstep, ow_billow, ow_cracks, ow_fbm, ow_grad2,
    ow_hash11, ow_hash12, ow_hash22, ow_hash32, ow_hash42, ow_noise, ow_noise01, ow_remap,
    ow_ridged, ow_rot, ow_sat, ow_sat3, ow_scratches, ow_shear, ow_shear_per, ow_srgb, ow_value,
    ow_voronoi_edge, ow_warp, ow_worley, Vec2, Vec3,
};

/// `sin`/`cos`/`pow`/`sqrt` are not bit-guaranteed across libm
/// implementations, so anything built from them is compared within an
/// absolute tolerance rather than exactly — the `1e-12` figure established in
/// `tests/core_port.rs` and reused by `tests/weapons_mathx_port.rs`.
fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "expected {expected:.17}, got {actual:.17}"
    );
}

fn assert_close2(actual: Vec2, expected: (f64, f64)) {
    assert_close(actual.x, expected.0);
    assert_close(actual.y, expected.1);
}

/// `p`, `p.xyx` reordering points shared across every hash/noise capture.
fn pts() -> [Vec2; 5] {
    [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.3, -2.7),
        Vec2::new(4.2, 0.9),
        Vec2::new(-1.1, 3.6),
        Vec2::new(8.0, 8.0),
    ]
}

// ---------------------------------------------------------------------------
// The tiny GLSL-primitive helpers. Hand-verified identities, not JS captures
// — these are Rust-internal building blocks, not one of the source's named
// GLSL functions.
// ---------------------------------------------------------------------------

#[test]
fn gl_primitives_match_glsl_semantics_on_negative_input() {
    // GLSL fract/mod are always non-negative for a positive modulus, unlike
    // Rust's built-in `%` and `f64::fract`.
    assert_eq!(gl_fract(-1.25), 0.75);
    assert_eq!(gl_fract(2.5), 0.5);
    assert_eq!(gl_mod(-3.0, 8.0), 5.0);
    assert_eq!(gl_mod(11.0, 8.0), 3.0);
    assert_eq!(gl_mix(0.0, 10.0, 0.3), 3.0);
    assert_eq!(gl_clamp(5.0, 0.0, 3.0), 3.0);
    assert_eq!(gl_clamp(-5.0, 0.0, 3.0), 0.0);
    assert_eq!(gl_smoothstep(0.0, 1.0, 0.5), 0.5);
    assert_eq!(gl_smoothstep(0.0, 1.0, 0.0), 0.0);
    assert_eq!(gl_smoothstep(0.0, 1.0, 1.0), 1.0);
}

// ---------------------------------------------------------------------------
// Hashes — exact equality (only `+ - * fract`, no transcendentals).
// ---------------------------------------------------------------------------

#[test]
fn hash11_matches_the_javascript_samples() {
    let expected = [
        0.0,
        0.37786821016252503,
        0.9717766283837364,
        0.019737865824936307,
        0.5665111894108463,
    ];
    for (x, want) in [0.0, 0.317, 1.7, -3.4, 100.25].into_iter().zip(expected) {
        assert_eq!(ow_hash11(x), want);
    }
}

#[test]
fn hash12_matches_the_javascript_samples() {
    let expected = [
        0.0,
        0.6037997457424353,
        0.06860831470612538,
        0.3179285048663587,
        0.956348419904316,
    ];
    for (p, want) in pts().into_iter().zip(expected) {
        assert_eq!(ow_hash12(p), want);
    }
}

#[test]
fn hash22_matches_the_javascript_samples() {
    let expected = [
        (0.0, 0.0),
        (0.9297683236782177, 0.6277837077764161),
        (0.03554765281592154, 0.9782605681123187),
        (0.6236066399833362, 0.977988785440175),
        (0.006502284990347107, 0.8226038176635484),
    ];
    for (p, want) in pts().into_iter().zip(expected) {
        let got = ow_hash22(p);
        assert_eq!((got.x, got.y), want);
    }
}

#[test]
fn hash32_matches_the_javascript_samples() {
    let expected = [
        (0.0, 0.0, 0.0),
        (0.5264722554338732, 0.227160668065153, 0.780385305522941),
        (0.058678437769913216, 0.003823229444151366, 0.8256011583655436),
        (0.6081891881021875, 0.9608317531328794, 0.13927711995529535),
        (0.7145816403935896, 0.5307796553697699, 0.5976933777365048),
    ];
    for (p, want) in pts().into_iter().zip(expected) {
        let got = ow_hash32(p);
        assert_eq!((got.x, got.y, got.z), want);
    }
}

#[test]
fn hash42_matches_the_javascript_samples() {
    let expected = [
        (0.0, 0.0, 0.0, 0.0),
        (
            0.11347751300854725,
            0.06869414881748526,
            0.8038227945053222,
            0.485466270388315,
        ),
        (
            0.8946813915654275,
            0.7920317016551053,
            0.3833704314638453,
            0.960603125155103,
        ),
        (
            0.3552503722567053,
            0.7358394316888734,
            0.26749907250086835,
            0.9713000338106212,
        ),
        (
            0.10140284268709365,
            0.2888662700133864,
            0.7543753216596087,
            0.6593982712911384,
        ),
    ];
    for (p, want) in pts().into_iter().zip(expected) {
        let got = ow_hash42(p);
        assert_eq!((got.x, got.y, got.z, got.w), want);
    }
}

// ---------------------------------------------------------------------------
// Gradient/value noise — `owHash12` inside `owGrad2` runs through `cos`/`sin`,
// so these need the tolerance.
// ---------------------------------------------------------------------------

#[test]
fn grad2_matches_the_javascript_samples() {
    let per = Vec2::new(8.0, 6.0);
    let expected = [
        (0.4849986661021655, -0.8745148905988509),
        (-0.7128549537505432, -0.7013114963504526),
        (-0.6957250485847137, 0.7183081906617784),
        (0.5707212921216385, 0.8211438404561088),
        (0.13481517834607207, -0.9908707623537576),
    ];
    for (p, want) in pts().into_iter().zip(expected) {
        assert_close2(ow_grad2(p, per), want);
    }
}

#[test]
fn noise_and_noise01_match_the_javascript_samples() {
    let per = Vec2::new(8.0, 6.0);
    let expected_noise = [
        0.0,
        0.4821266579672369,
        -0.2436386892337285,
        0.07012416947462162,
        0.0,
    ];
    let expected_noise01 = [
        0.5,
        0.7410633289836185,
        0.3781806553831357,
        0.5350620847373108,
        0.5,
    ];
    for ((p, want_n), want_n01) in pts()
        .into_iter()
        .zip(expected_noise)
        .zip(expected_noise01)
    {
        assert_close(ow_noise(p, per), want_n);
        assert_close(ow_noise01(p, per), want_n01);
    }
}

#[test]
fn value_matches_the_javascript_samples() {
    let per = Vec2::new(8.0, 6.0);
    let expected = [
        0.15863981284394413,
        0.3789236361223314,
        0.30405975297133203,
        0.6901847461486199,
        0.9989344705475105,
    ];
    for (p, want) in pts().into_iter().zip(expected) {
        assert_close(ow_value(p, per), want);
    }
}

/// The whole point of the `per` argument: shifting the input by exactly the
/// period reproduces the same value. Both sides captured from the JS.
#[test]
fn noise_and_value_are_periodic_under_an_integer_per() {
    let per = Vec2::new(8.0, 6.0);
    // (unshifted, shifted-by-per) pairs, captured from the JS.
    let noise_pairs = [
        (0.0, 0.0),
        (0.4821266579672369, 0.4821266579672381),
        (-0.2436386892337285, -0.2436386892337282),
        (0.07012416947462162, 0.07012416947462083),
        (0.0, 0.0),
    ];
    let value_pairs = [
        (0.15863981284394413, 0.15863981284394413),
        (0.3789236361223314, 0.3789236361223319),
        (0.30405975297133203, 0.30405975297133164),
        (0.6901847461486199, 0.6901847461486197),
        (0.9989344705475105, 0.9989344705475105),
    ];
    for (p, (want_p, want_shifted)) in pts().into_iter().zip(noise_pairs) {
        assert_close(ow_noise(p, per), want_p);
        assert_close(ow_noise(p.add(per), per), want_shifted);
    }
    for (p, (want_p, want_shifted)) in pts().into_iter().zip(value_pairs) {
        assert_close(ow_value(p, per), want_p);
        assert_close(ow_value(p.add(per), per), want_shifted);
    }
}

// ---------------------------------------------------------------------------
// fbm family
// ---------------------------------------------------------------------------

#[test]
fn fbm_ridged_and_billow_match_the_javascript_samples() {
    let per = Vec2::new(8.0, 6.0);
    // (oct, gain) rows in the same order as the capture script.
    let oct_gain = [(1, 0.5), (3, 0.5), (4, 0.55), (12, 0.5)];
    let expected_fbm = [
        [0.0, 0.4821266579672369, -0.2436386892337285, 0.07012416947462162, 0.0],
        [0.0, 0.2686627621714109, -0.06136369085533936, 0.003433848213132386, 0.0],
        [0.0, 0.2109329985431938, -0.08791800651383977, -0.0041149756003070375, 0.0],
        [0.0, 0.2200370414041589, -0.05942864620259109, 0.012847217234735408, 0.0],
    ];
    let expected_ridged = [
        [1.0, 0.26819279838818316, 0.5720824324240722, 0.8646690601952622, 1.0],
        [1.0, 0.4932533725416587, 0.6146189857213031, 0.8586579240402855, 1.0],
        [1.0, 0.5151175061559472, 0.5828035566264307, 0.8649877099840845, 1.0],
        [1.0, 0.5056866729111433, 0.571193718740451, 0.8381878493122518, 1.0],
    ];
    let expected_billow = [
        [0.0, 0.4821266579672369, 0.2436386892337285, 0.07012416947462162, 0.0],
        [0.0, 0.32310731726559094, 0.21708052541177894, 0.07670805975786375, 0.0],
        [0.0, 0.30555864721138787, 0.24291777551888122, 0.07358353554049316, 0.0],
        [0.0, 0.3123556292525021, 0.25300175441138006, 0.09218861494190057, 0.0],
    ];

    for (row, (oct, gain)) in oct_gain.into_iter().enumerate() {
        for (col, p) in pts().into_iter().enumerate() {
            assert_close(ow_fbm(p, per, oct, gain), expected_fbm[row][col]);
            assert_close(ow_ridged(p, per, oct, gain), expected_ridged[row][col]);
            assert_close(ow_billow(p, per, oct, gain), expected_billow[row][col]);
        }
    }
}

#[test]
fn fbm_is_periodic_under_an_integer_per_for_every_octave_row() {
    let per = Vec2::new(8.0, 6.0);
    let oct_gain = [(1, 0.5), (3, 0.5), (4, 0.55), (12, 0.5)];
    let expected = [
        [(0.0, 0.0), (0.4821266579672369, 0.4821266579672381), (-0.2436386892337285, -0.2436386892337282), (0.07012416947462162, 0.07012416947462083), (0.0, 0.0)],
        [(0.0, 0.0), (0.2686627621714109, 0.2686627621714111), (-0.06136369085533936, -0.06136369085533775), (0.003433848213132386, 0.0034338482131318943), (0.0, 0.0)],
        [(0.0, 0.0), (0.2109329985431938, 0.21093299854319322), (-0.08791800651383977, -0.0879180065138385), (-0.0041149756003070375, -0.004114975600307411), (0.0, 0.0)],
        [(0.0, 0.0), (0.2200370414041589, 0.22003704140415895), (-0.05942864620259109, -0.05942864620258916), (0.012847217234735408, 0.012847217234735071), (0.0, 0.0)],
    ];
    for (row, (oct, gain)) in oct_gain.into_iter().enumerate() {
        for (col, p) in pts().into_iter().enumerate() {
            let (want_p, want_shifted) = expected[row][col];
            assert_close(ow_fbm(p, per, oct, gain), want_p);
            assert_close(ow_fbm(p.add(per), per, oct, gain), want_shifted);
        }
    }
}

/// The GLSL `for (int i = 0; i < 10; i++){ if (i >= oct) break; ... }` cap:
/// requesting 12 octaves behaves exactly as requesting 10, because stock
/// GLSL cannot loop past a compile-time bound. `oct = 12` is reachable from
/// `MatParams` data (nothing in this port clamps it before calling), so this
/// is a real caller-facing contract, not a hypothetical.
#[test]
fn fbm_octaves_are_capped_at_ten_exactly_as_the_glsl_loop_bound() {
    let per = Vec2::new(8.0, 6.0);
    let expected_at_cap = [
        0.0,
        0.2200370414041589,
        -0.05942864620259109,
        0.012847217234735408,
        0.0,
    ];
    for (p, want) in pts().into_iter().zip(expected_at_cap) {
        let at_ten = ow_fbm(p, per, 10, 0.5);
        let at_twelve = ow_fbm(p, per, 12, 0.5);
        assert_close(at_ten, want);
        assert_eq!(at_ten, at_twelve, "oct=12 must be identical to oct=10");
    }
}

// ---------------------------------------------------------------------------
// Domain warp — periodic in the affine sense: warp(p + per) == warp(p) + per.
// ---------------------------------------------------------------------------

#[test]
fn warp_matches_the_javascript_samples_and_shifts_affinely_with_per() {
    let per = Vec2::new(8.0, 6.0);
    let expected = [
        (0.0545611627074759, -0.010065160424845355),
        (1.2594370163291675, -2.7120745682307605),
        (4.161690901375301, 0.8347280476716293),
        (-1.1088997835208347, 3.635023548151945),
        (7.998163690022445, 8.029515823003681),
    ];
    for (p, want) in pts().into_iter().zip(expected) {
        let warped = ow_warp(p, per, 0.2, 3);
        assert_close2(warped, want);
        // Shifting the input by exactly `per` shifts the warped output by
        // exactly `per` too — a domain-warp displacement field is periodic
        // in this affine sense, not by returning an identical value.
        let warped_shifted = ow_warp(p.add(per), per, 0.2, 3);
        assert_close(warped_shifted.x, warped.x + per.x);
        assert_close(warped_shifted.y, warped.y + per.y);
    }
}

// ---------------------------------------------------------------------------
// Worley / Voronoi — `sqrt`/`normalize` need the tolerance.
// ---------------------------------------------------------------------------

#[test]
fn worley_matches_the_javascript_samples_and_is_periodic() {
    let per = Vec2::new(8.0, 6.0);
    let expected = [
        (0.4463884654416811, 0.7044919830052849, 0.2817400227058897, 0.9286212426941347),
        (0.4731391569500875, 0.5285157638686523, 0.6708717565024926, 0.05535040890026721),
        (0.3534908204910449, 0.3585430730174321, 0.9432082129742412, 0.8931138058360375),
        (0.6042757407525146, 0.8842617394455056, 0.6211028873767646, 0.8195576578655164),
        (0.5684594430484096, 0.808931433047861, 0.3330789613733032, 0.636825628981569),
    ];
    for (p, want) in pts().into_iter().zip(expected) {
        let w = ow_worley(p, per, 0.9);
        assert_close(w.f1, want.0);
        assert_close(w.f2, want.1);
        assert_close(w.id_x, want.2);
        assert_close(w.id_y, want.3);

        // Periodic: same F1/F2/id at p + per.
        let shifted = ow_worley(p.add(per), per, 0.9);
        assert_close(shifted.f1, want.0);
        assert_close(shifted.f2, want.1);
        assert_close(shifted.id_x, want.2);
        assert_close(shifted.id_y, want.3);
    }
}

#[test]
fn voronoi_edge_matches_the_javascript_samples_and_is_periodic() {
    let per = Vec2::new(8.0, 6.0);
    let expected = [
        0.1557351132972604,
        0.05568229411213413,
        0.005265210826759875,
        0.16939996092148302,
        0.13302092087039202,
    ];
    for (p, want) in pts().into_iter().zip(expected) {
        assert_close(ow_voronoi_edge(p, per, 0.9), want);
        assert_close(ow_voronoi_edge(p.add(per), per, 0.9), want);
    }
}

// ---------------------------------------------------------------------------
// Cracks — see the module doc for why `per * 1.7` must be integer for exact
// periodicity, which is why this uses `per = (10, 10)` rather than `(8, 6)`.
// ---------------------------------------------------------------------------

#[test]
fn cracks_matches_the_javascript_samples() {
    let per = Vec2::new(8.0, 6.0);
    let expected = [0.0, 0.29947400564048404, 0.3029099142875382, 0.0, 0.0];
    for (p, want) in pts().into_iter().zip(expected) {
        assert_close(ow_cracks(p, per, 0.9, 0.06, 0.35), want);
    }
}

#[test]
fn cracks_is_periodic_when_per_times_1_7_is_integer() {
    let per = Vec2::new(10.0, 10.0); // 10 * 1.7 == 17, an integer.
    let crack_pts: Vec<Vec2> = (0..12)
        .map(|i| Vec2::new((f64::from(i) * 1.37) % 10.0, (f64::from(i) * 2.19) % 10.0))
        .collect();
    let expected = [
        0.9881983197418401,
        0.0,
        0.0,
        0.003828122149306168,
        0.0,
        0.9807999809978905,
        0.022197082066275797,
        0.0,
        0.0,
        0.0,
        0.8873458312377492,
        0.0,
    ];
    for (p, want) in crack_pts.into_iter().zip(expected) {
        let c = ow_cracks(p, per, 0.9, 0.15, 0.1);
        assert_close(c, want);
        assert_close(ow_cracks(p.add(per), per, 0.9, 0.15, 0.1), want);
    }
}

// ---------------------------------------------------------------------------
// Utilities — exact equality except owRot/owSRGB (sin/cos/pow).
// ---------------------------------------------------------------------------

#[test]
fn sat_sat3_and_remap_match_the_javascript_samples_exactly() {
    let expected_sat = [0.0, 0.0, 0.0, 0.5, 1.0, 1.0];
    for (x, want) in [-1.5, -0.2, 0.0, 0.5, 1.0, 1.7].into_iter().zip(expected_sat) {
        assert_eq!(ow_sat(x), want);
    }

    let sat3 = ow_sat3(Vec3::new(-0.5, 0.5, 1.5));
    assert_eq!((sat3.x, sat3.y, sat3.z), (0.0, 0.5, 1.0));

    // (x, a, b, c, d) -> owRemap(x, a, b, c, d). The last row (a == b) is the
    // degenerate span the `max(b - a, 1e-5)` guard exists for.
    let cases = [
        ((0.5, 0.0, 1.0, -1.0, 1.0), 0.0),
        ((1.5, 0.0, 1.0, -1.0, 1.0), 1.0),
        ((0.25, 0.0, 1.0, 0.0, 10.0), 2.5),
        ((0.5, 0.5, 0.5, 0.0, 1.0), 0.0),
    ];
    for ((x, a, b, c, d), want) in cases {
        assert_eq!(ow_remap(x, a, b, c, d), want);
    }
}

#[test]
fn rot_matches_the_column_major_glsl_matrix() {
    // See `ow_rot`'s doc: GLSL's column-major `mat2` makes this a clockwise
    // rotation for positive angles, not the counter-clockwise form the same
    // arguments would suggest under a row-major reading.
    let cases = [
        ((Vec2::new(1.0, 0.0), std::f64::consts::FRAC_PI_2), (6.123233995736766e-17, -1.0)),
        ((Vec2::new(1.0, 0.0), std::f64::consts::FRAC_PI_4), (0.7071067811865476, -0.7071067811865475)),
        ((Vec2::new(2.0, 3.0), 0.7), (3.4623374362820503, 1.0060911873780833)),
    ];
    for ((p, a), want) in cases {
        assert_close2(ow_rot(p, a), want);
    }
    // A quarter turn of the +x axis lands on -y, confirming the direction is
    // clockwise (a counter-clockwise quarter turn would land on +y).
    let turned = ow_rot(Vec2::new(1.0, 0.0), std::f64::consts::FRAC_PI_2);
    assert!(turned.y < -0.999);
}

#[test]
fn srgb_matches_the_javascript_samples() {
    let expected = [
        (0.0, 0.0015479876160990713, 0.21404114048223255),
        (0.0031308049535603713, 0.003131594552688991, 1.0),
    ];
    for (c, want) in [Vec3::new(0.0, 0.02, 0.5), Vec3::new(0.04045, 0.04046, 1.0)]
        .into_iter()
        .zip(expected)
    {
        let out = ow_srgb(c);
        assert_close(out.x, want.0);
        assert_close(out.y, want.1);
        assert_close(out.z, want.2);
    }
}

#[test]
fn shear_and_shear_per_match_the_javascript_samples_exactly() {
    let sheared = ow_shear(Vec2::new(1.5, 2.5), 2.0, 3.0);
    assert_eq!((sheared.x, sheared.y), (6.5, 7.5));

    let sheared_per = ow_shear_per(Vec2::new(8.0, 6.0), 3.0);
    assert_eq!((sheared_per.x, sheared_per.y), (8.0, 18.0));
}

// ---------------------------------------------------------------------------
// Scratches — see the module doc for why exact periodicity needs a SQUARE
// per; `(8, 6)` below (matching the other functions' pin values) is
// deliberately non-square and is not asserted periodic.
// ---------------------------------------------------------------------------

#[test]
fn scratches_matches_the_javascript_samples() {
    let per = Vec2::new(8.0, 6.0);
    let expected = [0.0, 0.0, 0.9969674661781042, 0.3734108929971096, 0.0];
    for (p, want) in pts().into_iter().zip(expected) {
        assert_close(ow_scratches(p, per, 3.0, 2.0, 0.5), want);
    }
}

#[test]
fn scratches_is_periodic_under_a_square_per() {
    let per = Vec2::new(8.0, 8.0);
    let expected = [
        (0.015304417993954293, 0.015304417993955795),
        (0.046729896537588946, 0.04672989653762804),
        (0.5386684916253036, 0.5386684916251845),
    ];
    let square_pts = [Vec2::new(0.3, 1.2), Vec2::new(4.9, -2.1), Vec2::new(-3.4, 7.7)];
    for (p, (want_p, want_shifted)) in square_pts.into_iter().zip(expected) {
        assert_close(ow_scratches(p, per, 3.0, 2.0, 0.5), want_p);
        assert_close(ow_scratches(p.add(per), per, 3.0, 2.0, 0.5), want_shifted);
    }
}
