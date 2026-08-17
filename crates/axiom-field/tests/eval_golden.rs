//! The committed evaluation goldens: `(graph, context) -> FieldValue`, asserted
//! **bit-exactly**, through the public surface only.
//!
//! CPU-to-CPU determinism is an equality, not a tolerance: for the twenty-three
//! operators of the exact tier, the same graph and the same context must produce
//! byte-identical `f32` lanes on every target, `wasm32` included. That holds
//! because their arithmetic is exact — `sqrt` is IEEE-754 exact and the one
//! reciprocal (`Normalize`) has its evaluation order fixed. These tables are what
//! makes a drift in any operator's arithmetic — or in the order of a single
//! expression — a failing test rather than a rendering that quietly changed.
//!
//! **The four transcendental rows (`Sin`, `Cos`, `Pow`, `Exp`) are committed
//! bit-exactly too, and they carry one caveat the other twenty-three do not.**
//! `f32::sin`/`cos`/`exp`/`powf` are deterministic for a given input on a given
//! target, but Rust does not guarantee them bit-identical *across* targets — they
//! reach the platform's libm. A failure in one of those four rows on a new target
//! is therefore a report about that target's libm, not necessarily a regression
//! in this layer; a failure in any of the other twenty-three is always a
//! regression. `crates/axiom-field/ARCHITECTURE.md` records this as the known
//! limit of the transcendental tier, and the tier is deliberately not allowed to
//! weaken the assertion for anything else.
//!
//! A value is committed as five words: its `FieldType` code, then the four lane
//! bit patterns.

use axiom_field::{
    EvalContext, FieldBuilder, FieldGraph, FieldId, FieldOp, FieldType, FieldValue, FIELD_OP_COUNT,
};
use axiom_kernel::Seconds;
use axiom_math::{Vec2, Vec3, Vec4};
use axiom_noise::{FbmConfig, Frequency};
use axiom_recipe::{NodeId, Param, Scalar};

/// The one context every golden below is evaluated at. Deliberately nothing
/// round: a lane that is accidentally read from the wrong source has to show up.
fn context() -> EvalContext {
    EvalContext::new(
        Vec3::new(0.37, 0.91, -0.22),
        Vec2::new(0.25, 0.75),
        Vec3::new(0.6, 0.8, 0.0),
        Seconds::finite_or_zero(1.5),
    )
}

/// The seed both spatial probes sample with.
const SEED: u64 = 4242;

/// The matrix the `Transform` probe applies: a non-uniform scale and a
/// translation, so a column read out of order cannot pass.
const MATRIX: [[f32; 4]; 4] = [
    [2.0, 0.0, 0.0, 0.0],
    [0.0, 3.0, 0.0, 0.0],
    [0.0, 0.0, 4.0, 0.0],
    [1.0, 2.0, 3.0, 1.0],
];

/// A value as the five words a golden row commits: the type code, then the four
/// lanes.
fn words(value: FieldValue) -> [u32; 5] {
    let lanes = value.as_vec4();
    [
        u32::from(value.ty().code()),
        lanes.x.to_bits(),
        lanes.y.to_bits(),
        lanes.z.to_bits(),
        lanes.w.to_bits(),
    ]
}

/// The eight nodes every probe graph starts with, and the parameter table they
/// read: slot 0 is a scalar knob, slots 1..5 the columns of [`MATRIX`].
///
/// * `0` `Point` (`Vec3`), `1` `Uv` (`Vec2`), `2` `Normal` (`Vec3`),
///   `3` `Time` (`Scalar`)
/// * `4` `Param` slot 0 (`Scalar` `0.75`)
/// * `5` `Const` `2.0`, `6` `Const` `0.25`, `7` `Const` `(1, 2, 3)`
fn prelude() -> FieldBuilder {
    let (build, knob) = FieldBuilder::new(FieldId::of_name("field/golden"), 1)
        .declare("knob", FieldValue::scalar(Scalar::new(0.75)));
    let build = (0..4).fold(build, |build, column| {
        let lanes = MATRIX[column];
        build
            .declare(
                ["col0", "col1", "col2", "col3"][column],
                FieldValue::vec4(Vec4::new(lanes[0], lanes[1], lanes[2], lanes[3])),
            )
            .0
    });
    let (build, _point) = build.push(FieldOp::Point, Vec::new(), Vec::new());
    let (build, _uv) = build.push(FieldOp::Uv, Vec::new(), Vec::new());
    let (build, _normal) = build.push(FieldOp::Normal, Vec::new(), Vec::new());
    let (build, _time) = build.push(FieldOp::Time, Vec::new(), Vec::new());
    let (build, _knob) = build.push_param(knob, FieldType::Scalar);
    let (build, _two) = build.push_const(FieldValue::scalar(Scalar::new(2.0)));
    let (build, _quarter) = build.push_const(FieldValue::scalar(Scalar::new(0.25)));
    let (build, _triple) = build.push_const(FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0)));
    build
}

/// The value of a probe graph: the prelude, then one node of `op`, evaluated at
/// [`context`]. The graph is validated first, so a golden can never be taken over
/// a graph the language would reject.
fn probe(op: FieldOp, params: Vec<Param>, inputs: &[u32]) -> [u32; 5] {
    let (build, node) = prelude().push(
        op,
        params,
        inputs.iter().map(|id| NodeId::from_raw(*id)).collect(),
    );
    let field = build.build(node);
    assert_eq!(field.validate(), Ok(()), "{op:?}'s probe graph must type");
    words(
        field
            .evaluate(&context())
            .expect("a validated graph evaluates"),
    )
}

/// The five parameter words of a `Const` node carrying `value`.
fn const_words(value: FieldValue) -> Vec<Param> {
    let mut params = vec![Param::int(u32::from(value.ty().code()))];
    let lanes = value.as_vec4();
    params.extend(
        [lanes.x, lanes.y, lanes.z, lanes.w]
            .iter()
            .map(|lane| Param::from_bits(lane.to_bits())),
    );
    params
}

/// The two seed words, low half first — the encoding `Noise` and `Fbm` share.
fn seed_words(seed: u64) -> Vec<Param> {
    vec![
        Param::from_bits(seed as u32),
        Param::from_bits((seed >> 32) as u32),
    ]
}

/// The config the `Fbm` probe samples with.
fn fbm_config() -> FbmConfig {
    FbmConfig::new(4, Frequency::finite_or_zero(1.5))
}

/// The parameter words and input ids of one probe, one row per operator in
/// discriminant order.
fn probe_spec(op: FieldOp) -> (Vec<Param>, &'static [u32]) {
    let table: [(Vec<Param>, &'static [u32]); FIELD_OP_COUNT] = [
        (const_words(FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0))), &[]),
        (Vec::new(), &[]),
        (Vec::new(), &[]),
        (Vec::new(), &[]),
        (Vec::new(), &[]),
        (
            vec![Param::int(0), Param::int(u32::from(FieldType::Scalar.code()))],
            &[],
        ),
        (Vec::new(), &[0, 7]),
        (Vec::new(), &[0, 7]),
        (Vec::new(), &[0, 5]),
        (Vec::new(), &[0, 7]),
        (Vec::new(), &[0, 7]),
        (Vec::new(), &[0]),
        (Vec::new(), &[0, 6, 5]),
        (Vec::new(), &[0, 7, 6]),
        (Vec::new(), &[6, 5, 0]),
        (Vec::new(), &[0, 7]),
        (Vec::new(), &[0]),
        (Vec::new(), &[0]),
        (vec![Param::int(3)], &[3, 4, 5]),
        (vec![Param::int(1)], &[0]),
        (seed_words(SEED), &[0]),
        (
            seed_words(SEED)
                .into_iter()
                .chain([
                    Param::from_bits(fbm_config().octaves),
                    Param::from_bits(fbm_config().frequency.get().to_bits()),
                    Param::from_bits(fbm_config().lacunarity.get().to_bits()),
                    Param::from_bits(fbm_config().gain.get().to_bits()),
                ])
                .collect(),
            &[0],
        ),
        (
            vec![Param::int(1), Param::int(2), Param::int(3), Param::int(4)],
            &[0],
        ),
        // The transcendental tier. `Pow` reads a strictly positive base — the
        // documented rule makes every other base `0.0`, and a golden of zeroes
        // would prove nothing about the arithmetic.
        (Vec::new(), &[0]),
        (Vec::new(), &[0]),
        (Vec::new(), &[7, 5]),
        (Vec::new(), &[6]),
    ];
    let (params, inputs) = &table[op.code() as usize];
    (params.clone(), inputs)
}

/// The committed golden: one row per operator, in discriminant order, each
/// `[type code, x, y, z, w]` with the lanes as `f32` bit patterns.
#[rustfmt::skip]
const GOLDEN_OPS: [[u32; 5]; FIELD_OP_COUNT] = [
    [2, 0x3F800000, 0x40000000, 0x40400000, 0x00000000], // Const      (1, 2, 3)
    [2, 0x3EBD70A4, 0x3F68F5C3, 0xBE6147AE, 0x00000000], // Point      (0.37, 0.91, -0.22)
    [1, 0x3E800000, 0x3F400000, 0x00000000, 0x00000000], // Uv         (0.25, 0.75)
    [2, 0x3F19999A, 0x3F4CCCCD, 0x00000000, 0x00000000], // Normal     (0.6, 0.8, 0)
    [0, 0x3FC00000, 0x00000000, 0x00000000, 0x00000000], // Time       1.5
    [0, 0x3F400000, 0x00000000, 0x00000000, 0x00000000], // Param      0.75
    [2, 0x3FAF5C29, 0x403A3D71, 0x4031EB85, 0x00000000], // Add        (1.37, 2.91, 2.78)
    [2, 0xBF2147AE, 0xBF8B851E, 0xC04E147B, 0x00000000], // Sub        (-0.63, -1.09, -3.22)
    [2, 0x3F3D70A4, 0x3FE8F5C3, 0xBEE147AE, 0x00000000], // Mul        (0.74, 1.82, -0.44)
    [2, 0x3EBD70A4, 0x3F68F5C3, 0xBE6147AE, 0x00000000], // Min        (0.37, 0.91, -0.22)
    [2, 0x3F800000, 0x40000000, 0x40400000, 0x00000000], // Max        (1, 2, 3)
    [2, 0x3EBD70A4, 0x3F68F5C3, 0x3E6147AE, 0x00000000], // Abs        (0.37, 0.91, 0.22)
    [2, 0x3EBD70A4, 0x3F68F5C3, 0x3E800000, 0x00000000], // Clamp      (0.37, 0.91, 0.25)
    [2, 0x3F070A3E, 0x3F975C29, 0x3F15C290, 0x00000000], // Mix        (0.5275, 1.1825, 0.585)
    [2, 0x3C5C8CAB, 0x3EA38B6D, 0x00000000, 0x00000000], // Smoothstep (0.0134613, 0.319423, 0)
    [0, 0x3FC3D70B, 0x00000000, 0x00000000, 0x00000000], // Dot        1.53
    [0, 0x3F80DAD1, 0x00000000, 0x00000000, 0x00000000], // Length     1.00668
    [2, 0x3EBC2EF1, 0x3F676A28, 0xBE5FC91E, 0x00000000], // Normalize  (0.367546, 0.903964, -0.218541)
    [2, 0x3FC00000, 0x3F400000, 0x40000000, 0x00000000], // Compose    (1.5, 0.75, 2)
    [0, 0x3F68F5C3, 0x00000000, 0x00000000, 0x00000000], // Component  0.91 (lane 1)
    [0, 0x3EA99126, 0x00000000, 0x00000000, 0x00000000], // Noise      0.331186
    [0, 0xBD7B2F0C, 0x00000000, 0x00000000, 0x00000000], // Fbm        -0.0613242
    [2, 0x3FDEB852, 0x40975C29, 0x4007AE14, 0x00000000], // Transform  (1.74, 4.73, 2.12)
    [2, 0x3EB925A9, 0x3F4A1CEB, 0xBE5F7796, 0x00000000], // Sin        (0.361615, 0.789504, -0.218230)
    [2, 0x3F6EAD01, 0x3F1D1E71, 0x3F79D46A, 0x00000000], // Cos        (0.932327, 0.613117, 0.975897)
    [2, 0x3F800000, 0x40800000, 0x41100000, 0x00000000], // Pow        (1, 4, 9) = (1, 2, 3) ^ 2
    [0, 0x3FA45AF2, 0x00000000, 0x00000000, 0x00000000], // Exp        1.28403 = e ^ 0.25
];

#[test]
fn every_operator_evaluates_to_its_committed_golden() {
    let actual: Vec<[u32; 5]> = FieldOp::ALL
        .iter()
        .map(|op| {
            let (params, inputs) = probe_spec(*op);
            probe(*op, params, inputs)
        })
        .collect();
    assert_eq!(
        actual,
        GOLDEN_OPS.to_vec(),
        "an operator's arithmetic moved; every mirror of this language is now wrong"
    );
}

#[test]
fn the_golden_rows_carry_the_types_the_signature_table_promises() {
    // The golden is not just numbers: each row's type code is the operator's
    // declared output type, so a row cannot drift into a different width.
    let expected = [
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec2,
        FieldType::Vec3,
        FieldType::Scalar,
        FieldType::Scalar,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Scalar,
        FieldType::Scalar,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Scalar,
        FieldType::Scalar,
        FieldType::Scalar,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Vec3,
        FieldType::Scalar,
    ];
    GOLDEN_OPS
        .iter()
        .zip(expected)
        .enumerate()
        .for_each(|(index, (row, ty))| {
            assert_eq!(row[0], u32::from(ty.code()), "row {index}");
        });
}

/// The reference case the whole design exists to serve: **a spatial gradient
/// mixed with fbm, driving a scalar**, then selecting between two colours.
///
/// ```text
/// Mix(Const(a), Const(b),
///     Smoothstep(0, 1, Add(Mul(Component(Point, 1), k),
///                          Mul(Fbm(seed, Point), w))))
/// ```
///
/// `k` (the gradient's slope) and `w` (the fbm's weight) are *parameters*, so
/// retuning the look is a value change that cannot move the structural digest —
/// the property the whole parameter table exists for.
fn reference_field() -> FieldGraph {
    let (build, slope) = FieldBuilder::new(FieldId::of_name("field/reference"), 1)
        .declare("gradient_slope", FieldValue::scalar(Scalar::new(0.8)));
    let (build, weight) = build.declare("fbm_weight", FieldValue::scalar(Scalar::new(0.35)));

    let (build, point) = build.push(FieldOp::Point, Vec::new(), Vec::new());
    let (build, height) = build.push(FieldOp::Component, vec![Param::int(1)], vec![point]);
    let (build, slope_node) = build.push_param(slope, FieldType::Scalar);
    let (build, gradient) = build.push(FieldOp::Mul, Vec::new(), vec![height, slope_node]);

    let (build, grain) = build.push_fbm(SEED, fbm_config(), point);
    let (build, weight_node) = build.push_param(weight, FieldType::Scalar);
    let (build, weighted) = build.push(FieldOp::Mul, Vec::new(), vec![grain, weight_node]);

    let (build, driver) = build.push(FieldOp::Add, Vec::new(), vec![gradient, weighted]);
    let (build, low) = build.push_const(FieldValue::scalar(Scalar::new(0.0)));
    let (build, high) = build.push_const(FieldValue::scalar(Scalar::new(1.0)));
    let (build, mask) = build.push(FieldOp::Smoothstep, Vec::new(), vec![low, high, driver]);

    let (build, rock) = build.push_const(FieldValue::vec3(Vec3::new(0.12, 0.11, 0.10)));
    let (build, moss) = build.push_const(FieldValue::vec3(Vec3::new(0.18, 0.32, 0.14)));
    let (build, albedo) = build.push(FieldOp::Mix, Vec::new(), vec![rock, moss, mask]);
    build.build(albedo)
}

/// The committed golden of the reference case at [`context`].
const GOLDEN_REFERENCE: [u32; 5] = [2, 0x3E2B8D43, 0x3E8D7EE3, 0x3E06D90E, 0x00000000];

#[test]
fn the_composed_reference_case_evaluates_to_its_committed_golden() {
    let field = reference_field();
    assert_eq!(field.validate(), Ok(()));
    let value = field
        .evaluate(&context())
        .expect("the reference field evaluates");
    assert_eq!(words(value), GOLDEN_REFERENCE);
    assert_eq!(value.ty(), FieldType::Vec3);
}

#[test]
fn the_reference_case_is_a_gradient_that_actually_varies_with_the_point() {
    let field = reference_field();
    let at = |y: f32| {
        field
            .evaluate(&EvalContext::new(
                Vec3::new(0.37, y, -0.22),
                Vec2::ZERO,
                Vec3::UNIT_Y,
                Seconds::finite_or_zero(0.0),
            ))
            .expect("the reference field evaluates")
            .as_vec4()
    };
    let low = at(-2.0);
    let high = at(2.0);
    // Below the mask's lower edge the mix is all rock; above the upper edge it is
    // all moss. Between them the fbm perturbs the boundary, which is the whole
    // point of the composition.
    assert_eq!((low.x, low.y, low.z), (0.12, 0.11, 0.10));
    assert_eq!((high.x, high.y, high.z), (0.18, 0.32, 0.14));
    let middle = at(0.5);
    assert!(middle.y > low.y);
    assert!(middle.y < high.y);
}

#[test]
fn canonicalising_the_reference_case_does_not_change_what_it_computes() {
    let field = reference_field();
    let canonical = field.canonicalize().expect("the reference field types");
    // The parameter table survives folding untouched — the digest is keyed on it.
    assert_eq!(canonical.params(), field.params());
    assert_eq!(canonical.digest(), field.digest());
    assert_eq!(
        canonical
            .evaluate(&context())
            .map(words)
            .expect("the canonical form evaluates"),
        GOLDEN_REFERENCE,
        "canonicalisation must not change what a field computes"
    );
}

/// **Marble veining — the pattern the transcendental tier exists for**, authored
/// entirely as a graph.
///
/// ```text
/// Mix(dark, light,
///     Pow(Smoothstep(-1, 1, Sin(Add(Mul(Component(Point, 0), frequency),
///                                   Mul(Fbm(seed, Point), warp)))),
///         sharpness))
/// ```
///
/// A sine along one axis is the vein family; the fbm warps its phase so the
/// veins wander instead of striping; `Pow` sharpens the light bands against the
/// dark stone. **Not one line of new Rust** — the whole look is `Sin`, `Pow` and
/// operators that already existed, which is the property the library tier of
/// `ARCHITECTURE.md` claims and this test is the evidence for.
///
/// `frequency`, `warp` and `sharpness` are *parameters*, so retuning the marble
/// cannot move the structural digest.
fn marble_veining(vein_frequency: f32) -> FieldGraph {
    let (build, frequency) = FieldBuilder::new(FieldId::of_name("field/marble"), 1)
        .declare(
            "vein_frequency",
            FieldValue::scalar(Scalar::new(vein_frequency)),
        );
    let (build, warp) = build.declare("vein_warp", FieldValue::scalar(Scalar::new(2.5)));
    let (build, sharpness) = build.declare("vein_sharpness", FieldValue::scalar(Scalar::new(3.0)));

    let (build, point) = build.push(FieldOp::Point, Vec::new(), Vec::new());
    let (build, along) = build.push(FieldOp::Component, vec![Param::int(0)], vec![point]);
    let (build, frequency_node) = build.push_param(frequency, FieldType::Scalar);
    let (build, scaled) = build.push(FieldOp::Mul, Vec::new(), vec![along, frequency_node]);

    let (build, grain) = build.push_fbm(SEED, fbm_config(), point);
    let (build, warp_node) = build.push_param(warp, FieldType::Scalar);
    let (build, warped) = build.push(FieldOp::Mul, Vec::new(), vec![grain, warp_node]);

    let (build, phase) = build.push(FieldOp::Add, Vec::new(), vec![scaled, warped]);
    let (build, wave) = build.push(FieldOp::Sin, Vec::new(), vec![phase]);
    let (build, low) = build.push_const(FieldValue::scalar(Scalar::new(-1.0)));
    let (build, high) = build.push_const(FieldValue::scalar(Scalar::new(1.0)));
    let (build, mask) = build.push(FieldOp::Smoothstep, Vec::new(), vec![low, high, wave]);

    let (build, sharpness_node) = build.push_param(sharpness, FieldType::Scalar);
    let (build, vein) = build.push(FieldOp::Pow, Vec::new(), vec![mask, sharpness_node]);

    let (build, stone) = build.push_const(FieldValue::vec3(Vec3::new(0.05, 0.05, 0.06)));
    let (build, calcite) = build.push_const(FieldValue::vec3(Vec3::new(0.92, 0.90, 0.86)));
    let (build, albedo) = build.push(FieldOp::Mix, Vec::new(), vec![stone, calcite, vein]);
    build.build(albedo)
}

/// The marble's red channel sampled along `x` at a fixed `y`/`z`.
fn marble_scan(field: &FieldGraph) -> Vec<f32> {
    (0..200)
        .map(|step| {
            let x = step as f32 * 0.02 - 2.0;
            field
                .evaluate(&EvalContext::new(
                    Vec3::new(x, 0.31, -0.17),
                    Vec2::ZERO,
                    Vec3::UNIT_Y,
                    Seconds::finite_or_zero(0.0),
                ))
                .expect("the marble field evaluates")
                .as_vec4()
                .x
        })
        .collect()
}

/// How many times a scan crosses the midpoint between stone and calcite — the
/// vein count, and the thing a sine is here to produce.
fn crossings(scan: &[f32]) -> usize {
    let midpoint = 0.5 * (0.05 + 0.92);
    scan.windows(2)
        .filter(|pair| (pair[0] < midpoint) != (pair[1] < midpoint))
        .count()
}

#[test]
fn the_marble_pattern_is_a_legal_graph_of_existing_operators_only() {
    let field = marble_veining(9.0);
    assert_eq!(field.validate(), Ok(()));
    // Small enough to be a *library graph*, not an engine feature.
    assert!(field.node_count() <= 20, "{} nodes", field.node_count());
    let value = field
        .evaluate(&context())
        .expect("the marble field evaluates");
    assert_eq!(value.ty(), FieldType::Vec3);
}

#[test]
fn the_marble_pattern_actually_veins() {
    let scan = marble_scan(&marble_veining(9.0));
    let (low, high) = scan
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), sample| {
            (low.min(*sample), high.max(*sample))
        });
    assert!(
        high - low > 0.5,
        "the veins must span most of the stone-to-calcite range: {low}..{high}"
    );
    assert!(
        crossings(&scan) >= 4,
        "a sine at this frequency must produce several veins, not one band"
    );
}

#[test]
fn retuning_the_marble_changes_the_look_without_moving_the_structural_digest() {
    let field = marble_veining(9.0);
    let denser = marble_veining(22.0);
    assert_eq!(denser.digest(), field.digest(), "structure did not change");
    assert_ne!(denser.params(), field.params(), "the tuning did change");
    assert!(
        crossings(&marble_scan(&denser)) > crossings(&marble_scan(&field)),
        "a higher vein frequency must produce more veins"
    );
}

#[test]
fn evaluation_is_bit_stable_across_repeats_and_a_serialization_round_trip() {
    let field = reference_field();
    let once = field.evaluate(&context()).expect("evaluates");
    let twice = field.evaluate(&context()).expect("evaluates");
    assert_eq!(words(once), words(twice));

    let decoded = FieldGraph::deserialize(&field.serialize()).expect("the field round trips");
    assert_eq!(
        words(decoded.evaluate(&context()).expect("evaluates")),
        words(once),
        "the bytes are the determinism proof; a decoded field must compute the same value"
    );
}
