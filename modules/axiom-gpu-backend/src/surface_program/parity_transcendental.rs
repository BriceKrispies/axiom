//! **The transcendental tier's parity, and the measurement its tolerance comes
//! from.**
//!
//! `Sin`, `Cos`, `Pow` and `Exp` are the only operators in the algebra that both
//! sides *approximate* rather than compute. Every other operator is `+`, `*`,
//! `min`, `sqrt` or a table read, where the CPU and the GPU can differ only by
//! the contraction and reassociation the hardware is permitted; these four are
//! two different polynomial approximations of the same function, and no amount of
//! care in the emitter closes that gap.
//!
//! So the tier carries its own budget — and the budget is **measured**:
//!
//! * [`MEASURED_WORST_DELTA`] commits the worst absolute lane delta each of the
//!   four showed on a real adapter, as **data** rather than as a line of console
//!   output, and
//!   [`the_transcendental_tolerance_is_not_looser_than_the_hardware_needs`]
//!   re-measures it every run: it fails if the live delta has drifted clear of
//!   the record, if a declared tolerance is short of the delta, or if a declared
//!   tolerance sits more than an order of magnitude above it. A tolerance looser
//!   than the hardware needs is a tolerance that hides the next regression, so
//!   being *too generous* is a failure here, not a safety margin.
//! * [`the_exact_tier_did_not_widen`] is the other half: the twenty-three
//!   operators that were exact before this tier existed are still held to `1e-4`,
//!   and the per-operator table cannot have quietly relaxed one of them.
//!
//! **The measurement's finding is the opposite of the assumption that kept these
//! four out of the algebra:** on a real adapter the tier agrees to about `1e-6`
//! *relative*, so its budget is **tighter** than the algebra's `1e-4`, not wider.
//! `Pow` carries a larger *absolute* constant only because its case's outputs
//! reach ~104 where the others stay inside `[-3, 3]`.
//!
//! The measured numbers are recorded in `crates/axiom-field/ARCHITECTURE.md`.
//! They are a property of the adapter the test ran on; a constant is set from the
//! worst adapter measured, never from the best.
//!
//! This lives in its own file for the same reason `parity_vertex` and
//! `parity_lighting` do: `parity` is already near the engine file-size budget.

use axiom_field::{FieldOp, FieldType, FieldValue, FIELD_OP_COUNT};
use axiom_math::Vec3;
use axiom_recipe::Scalar;

use crate::surface_program::parity::{
    assert_parity_within, builder, case, compare, contexts, vec3_const, widen, worst_delta, Case,
    ParityGpu, TOLERANCE,
};

/// The absolute tolerance of the **transcendental tier** where its outputs live
/// in a unit-ish band: `Sin`, `Cos` and `Exp`.
///
/// **This number is a measurement, not a choice.** It was tightened to just
/// above the worst delta observed across the parity sweep's sampled contexts —
/// [`the_transcendental_tolerance_is_not_looser_than_the_hardware_needs`]
/// re-measures it every run and fails if this constant has drifted more than an
/// order of magnitude clear of it, because a tolerance looser than the hardware
/// needs is a tolerance that hides the next regression.
///
/// Note what it is **not**: wider than the exact tier's `1e-4`. The measurement
/// says these three agree to about `1e-6` *relative* on a real adapter, so the
/// tier gets a **tighter** budget than the algebra's default, not a looser one.
/// The per-operator table exists to state that honestly, not to excuse anything.
pub(super) const TRANSCENDENTAL_TOLERANCE: f32 = 1.0e-6;

/// The absolute tolerance of `Pow`, the one transcendental whose sampled outputs
/// leave the unit band.
///
/// Also a measurement. It is thirty times [`TRANSCENDENTAL_TOLERANCE`] for one
/// reason and it is **not** accuracy: `Pow`'s parity case reaches an output near
/// `104`, where `Sin`/`Cos`/`Exp` stay inside `[-3, 3]`. The measured *relative*
/// error is the same `~1e-6` — about ten `f32` ulps — for all four. An absolute
/// budget therefore has to follow the magnitude of the case, and saying so in a
/// second named constant is more honest than one loose number covering both.
pub(super) const POW_TOLERANCE: f32 = 3.0e-5;

/// One tolerance per operator, indexed by the operator code.
///
/// Written out in full rather than derived from a code range: the transcendental
/// tier happens to be the tail of today's code space, and a rule that said so
/// would silently hand the loose budget to the next operator appended.
#[rustfmt::skip]
const TOLERANCES: [f32; FIELD_OP_COUNT] = [
    TOLERANCE,                                              // Const
    TOLERANCE, TOLERANCE, TOLERANCE, TOLERANCE,             // Point / Uv / Normal / Time
    TOLERANCE,                                              // Param
    TOLERANCE, TOLERANCE, TOLERANCE, TOLERANCE, TOLERANCE,  // Add / Sub / Mul / Min / Max
    TOLERANCE,                                              // Abs
    TOLERANCE, TOLERANCE, TOLERANCE,                        // Clamp / Mix / Smoothstep
    TOLERANCE, TOLERANCE, TOLERANCE,                        // Dot / Length / Normalize
    TOLERANCE, TOLERANCE,                                   // Compose / Component
    TOLERANCE, TOLERANCE,                                   // Noise / Fbm
    TOLERANCE,                                              // Transform
    TRANSCENDENTAL_TOLERANCE, TRANSCENDENTAL_TOLERANCE,     // Sin / Cos
    POW_TOLERANCE, TRANSCENDENTAL_TOLERANCE,                // Pow / Exp
];

/// The absolute tolerance this operator is held to.
pub(super) fn tolerance_of(op: FieldOp) -> f32 {
    TOLERANCES[op.code() as usize]
}

/// The four transcendental cases, appended to the algebra-wide sweep.
///
/// Each one drives its operator over the object-space `Point`, whose sampled
/// range spans several units and both signs, so the comparison covers a real
/// argument range rather than a single convenient value.
pub(super) fn cases() -> Vec<Case> {
    vec![sin_case(), cos_case(), pow_case(), exp_case()]
}

/// `Sin` of the object-space point.
fn sin_case() -> Case {
    unary_case("p/sin", FieldOp::Sin)
}

/// `Cos` of the object-space point.
fn cos_case() -> Case {
    unary_case("p/cos", FieldOp::Cos)
}

/// `Exp` of the object-space point **scaled down**, so the sampled range stays
/// inside `e^x`'s useful span instead of overflowing to an infinity that no
/// tolerance can compare.
fn exp_case() -> Case {
    let (b, point) = builder("p/exp").push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, scale) = b.push_const(FieldValue::scalar(Scalar::new(0.25)));
    let (b, scaled) = b.push(FieldOp::Mul, Vec::new(), vec![point, scale]);
    let (b, applied) = b.push(FieldOp::Exp, Vec::new(), vec![scaled]);
    let (b, wide) = widen(b, applied, FieldType::Vec3);
    case(FieldOp::Exp, b.build(wide))
}

/// `Pow` of a **positive** base — the absolute value of the object-space point,
/// lifted clear of zero — raised to a `Vec3` of assorted exponents.
///
/// The positive base is the point: the operator's documented rule makes every
/// other base exactly `0.0` on both sides, which agrees trivially and would prove
/// nothing about the hardware's `pow`. The degenerate bases are pinned by the CPU
/// evaluator's own tests and by `emit_ops`; what this case measures is the
/// approximation.
fn pow_case() -> Case {
    let (b, point) = builder("p/pow").push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, magnitude) = b.push(FieldOp::Abs, Vec::new(), vec![point]);
    let (b, floor) = vec3_const(b, 0.5, 0.5, 0.5);
    let (b, base) = b.push(FieldOp::Add, Vec::new(), vec![magnitude, floor]);
    let (b, exponent) = vec3_const(b, 0.5, 2.0, 3.5);
    let (b, applied) = b.push(FieldOp::Pow, Vec::new(), vec![base, exponent]);
    let (b, wide) = widen(b, applied, FieldType::Vec3);
    case(FieldOp::Pow, b.build(wide))
}

/// A case for a one-input operator over `Point`.
fn unary_case(name: &str, op: FieldOp) -> Case {
    let (b, point) = builder(name).push(FieldOp::Point, Vec::new(), Vec::new());
    let (b, applied) = b.push(op, Vec::new(), vec![point]);
    let (b, wide) = widen(b, applied, FieldType::Vec3);
    case(op, b.build(wide))
}

/// How far above the measured worst case a declared tolerance may sit before it
/// stops being a measurement and starts being a hiding place.
const SLACK_LIMIT: f32 = 10.0;

/// How far the live measurement may drift above the committed one before the
/// record is stale and has to be retaken.
const DRIFT_LIMIT: f32 = 2.0;

/// **The measurement itself, committed as data.**
///
/// The worst absolute lane delta each operator showed across [`cases`] over the
/// sweep's sampled contexts, on the adapter recorded in
/// `crates/axiom-field/ARCHITECTURE.md` (Vulkan, discrete). Together with each
/// case's output magnitude — `Sin`/`Cos` ~1, `Exp` ~3, `Pow` ~104 — these say the
/// tier agrees to about `1e-6` *relative* on real hardware.
///
/// They are a table rather than a line of console output on purpose. A number
/// printed into a test log is read once and rots; a number committed here is
/// diffable, greppable, and **re-checked every run** by
/// [`the_transcendental_tolerance_is_not_looser_than_the_hardware_needs`], which
/// fails if the live delta has drifted clear of the record or if a declared
/// tolerance has drifted clear of the delta. Console output is banned in a module
/// anyway (the Module Law), and this is the better answer, not a workaround for
/// it.
const MEASURED_WORST_DELTA: [(FieldOp, f32); 4] = [
    (FieldOp::Sin, 5.07e-7),
    (FieldOp::Cos, 4.18e-7),
    (FieldOp::Pow, 2.29e-5),
    (FieldOp::Exp, 2.39e-7),
];

/// **The measurement, re-taken.** For each transcendental operator it measures
/// the live worst absolute lane delta and holds three relations, so neither the
/// record nor the tolerance can quietly stop describing the hardware:
///
/// 1. the live delta is within [`DRIFT_LIMIT`] of [`MEASURED_WORST_DELTA`] — the
///    committed record is still true;
/// 2. the declared tolerance covers the live delta — the budget is honest;
/// 3. the declared tolerance is no more than [`SLACK_LIMIT`] above it — the
///    budget is not a hiding place. Being *too generous* fails here.
#[test]
fn the_transcendental_tolerance_is_not_looser_than_the_hardware_needs() {
    let gpu = ParityGpu::acquire();
    assert_ne!(gpu.backend, wgpu::Backend::Noop);
    let contexts = contexts();
    cases().iter().for_each(|entry| {
        let (cpu, rendered) = compare(&gpu, entry, &contexts);
        let delta = worst_delta(&cpu, &rendered);
        let name = entry.name();
        // The signal has to actually move, or a tiny delta means nothing.
        let spread = cpu
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), lanes| {
                (low.min(lanes[0]), high.max(lanes[0]))
            });
        assert!(
            spread.1 - spread.0 > 0.1,
            "{name} must vary across the sampled contexts, or the measurement is vacuous"
        );
        let recorded = MEASURED_WORST_DELTA
            .iter()
            .find(|(op, _)| *op == entry.operator)
            .map_or(0.0, |(_, recorded)| *recorded);
        assert!(
            delta <= recorded * DRIFT_LIMIT,
            "{name}'s worst CPU/GPU delta is now {delta:e} against a committed measurement of \
             {recorded:e} on {:?}. Re-measure and re-record it rather than widening a tolerance.",
            gpu.backend
        );
        let tolerance = tolerance_of(entry.operator);
        assert!(
            recorded <= tolerance,
            "{name}'s recorded measurement {recorded:e} is outside its declared tolerance \
             {tolerance:e}"
        );
        assert!(
            delta <= tolerance,
            "{name}'s tolerance must cover the hardware: worst {delta:e} against {tolerance:e}"
        );
        assert!(
            tolerance <= delta * SLACK_LIMIT,
            "{name}'s tolerance is {tolerance:e} against a measured worst case of {delta:e} \
             — more than {SLACK_LIMIT}x of slack is a tolerance that hides the next \
             regression, not a budget. Tighten it."
        );
    });
}

/// Each of the four still agrees within its own budget, named individually so a
/// failure reports which transcendental drifted.
#[test]
fn every_transcendental_agrees_within_the_measured_tolerance() {
    let gpu = ParityGpu::acquire();
    let contexts = contexts();
    cases().iter().for_each(|entry| {
        let (cpu, rendered) = compare(&gpu, entry, &contexts);
        assert_parity_within(
            &entry.name(),
            tolerance_of(entry.operator),
            &cpu,
            &rendered,
        );
    });
}

/// **The exact tier did not widen.** Giving four operators their own budget is
/// only sound if it reached exactly those four, so this asserts the shape of the
/// tolerance table itself: every operator outside the tier is still at `1e-4`,
/// and each operator inside it carries the measured constant it is supposed to.
#[test]
fn the_exact_tier_did_not_widen() {
    assert_eq!(TOLERANCE, 1.0e-4);
    let tier = [
        (FieldOp::Sin, TRANSCENDENTAL_TOLERANCE),
        (FieldOp::Cos, TRANSCENDENTAL_TOLERANCE),
        (FieldOp::Pow, POW_TOLERANCE),
        (FieldOp::Exp, TRANSCENDENTAL_TOLERANCE),
    ];
    FieldOp::ALL.iter().for_each(|op| {
        let declared = tier
            .iter()
            .find(|(member, _)| member == op)
            .map_or(TOLERANCE, |(_, tolerance)| *tolerance);
        assert_eq!(
            tolerance_of(*op),
            declared,
            "{op:?} is held to the wrong tolerance"
        );
    });
    // Every declared budget is the measured number, not the `1e-3` bound the
    // manifest started from — and neither is looser than the algebra's default.
    assert!(TRANSCENDENTAL_TOLERANCE < TOLERANCE);
    assert!(POW_TOLERANCE < TOLERANCE);
}

/// A `Pow` whose base is not positive is `0.0` on **both** sides — the whole
/// reason the operator's rule is stated as it is, proved on the real device
/// rather than argued about.
#[test]
fn a_non_positive_pow_base_is_zero_on_the_gpu_too() {
    let gpu = ParityGpu::acquire();
    let contexts = contexts();
    // Bases: a negative constant, exactly zero, and a positive control.
    let (b, base) = builder("p/pow-degenerate").push_const(FieldValue::vec3(Vec3::new(
        -2.0, 0.0, 4.0,
    )));
    let (b, exponent) = vec3_const(b, 0.5, -1.0, 0.5);
    let (b, applied) = b.push(FieldOp::Pow, Vec::new(), vec![base, exponent]);
    let (b, wide) = widen(b, applied, FieldType::Vec3);
    let degenerate = case(FieldOp::Pow, b.build(wide));
    let (cpu, rendered) = compare(&gpu, &degenerate, &contexts);
    assert_eq!(
        cpu[0][0], 0.0,
        "a negative base is zero, never a NaN, on the CPU"
    );
    assert_eq!(cpu[0][1], 0.0, "a zero base is zero, never an infinity");
    assert_eq!(cpu[0][2], 2.0, "the positive control still computes");
    assert_parity_within(&degenerate.name(), POW_TOLERANCE, &cpu, &rendered);
}
