//! Seeded generation of the course's **control points**, and the bounded
//! correction pass that makes them legal.
//!
//! Generation is two phases, deliberately separated:
//!
//! 1. **Author** a heading-step and grade signal per control point from the
//!    section plan ([`crate::track::section`]). Bends and hills are emitted as
//!    *events* — a smooth half-sine bump over several control points — separated
//!    by straights, rather than as per-point noise. That is what makes a bend
//!    read as one long sweeper you can commit to instead of a jitter.
//! 2. **Correct** the signal against the hard constraints in
//!    [`CourseTuning`]: clamp the magnitudes, then run a *fixed* number of
//!    relaxation sweeps that limit how fast the heading step and the grade may
//!    change between adjacent points. Every loop here has a compile-time bound
//!    and a deterministic outcome — there is no "retry until valid".
//!
//! Only then are positions integrated, so a position can never encode an
//! illegal turn: the constraint holds on the signal, and the signal is what the
//! geometry is built from.

use axiom_math::Vec3;

use crate::draw::Draw;
use crate::tuning::CourseTuning;

use super::section::SectionKind;

/// One authored point on the route, before the spline smooths between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlPoint {
    /// World position of the road centre at this point.
    pub position: Vec3,
    /// Heading (radians, `0` = `+Z`, increasing turns toward `+X`).
    pub heading: f32,
    /// Grade (rise over run) leaving this point.
    pub grade: f32,
    /// The road's half-width here (m).
    pub half_width: f32,
    /// Which section this point belongs to.
    pub section: SectionKind,
}

/// Hard ceiling on how many events one section may emit. Every generation loop
/// is written as a bounded `for`, so a pathological profile cannot hang the
/// build — it simply stops emitting and the remaining points stay straight.
const MAX_EVENTS_PER_SECTION: usize = 512;

/// Generate the course's control points for `seed`.
///
/// The result is a pure function of `(seed, tuning)`.
pub fn control_points(seed: u64, tuning: &CourseTuning) -> Vec<ControlPoint> {
    let mut draw = Draw::seeded(seed);
    let mut yaw_step: Vec<f32> = Vec::new();
    let mut grade: Vec<f32> = Vec::new();
    let mut half_width: Vec<f32> = Vec::new();
    let mut section: Vec<SectionKind> = Vec::new();

    for kind in SectionKind::ALL {
        let profile = kind.profile();
        let count = (profile.length / tuning.control_spacing).round().max(2.0) as usize;
        emit_bends(&mut draw, &profile, tuning, count, &mut yaw_step);
        emit_hills(&mut draw, &profile, tuning, count, &mut grade);
        emit_width(&mut draw, &profile, tuning, count, &mut half_width);
        section.extend(std::iter::repeat(kind).take(count));
    }

    relax(&mut yaw_step, tuning.max_yaw_step, tuning.max_yaw_step_delta, tuning.correction_passes);
    relax(&mut grade, tuning.max_grade, tuning.max_grade_delta, tuning.correction_passes);
    // The first and last few points are flattened so the start line and the
    // finish arch always sit on straight, level road regardless of the draw.
    flatten_ends(&mut yaw_step, &mut grade, 4);

    integrate(&yaw_step, &grade, &half_width, &section, tuning)
}

/// Emit `count` heading steps for one section as alternating straights and
/// smooth bend events.
fn emit_bends(
    draw: &mut Draw,
    profile: &super::section::SectionProfile,
    tuning: &CourseTuning,
    count: usize,
    out: &mut Vec<f32>,
) {
    let start = out.len();
    out.resize(start + count, 0.0);
    let peak = profile.curviness * tuning.max_yaw_step;
    let mut i = 0usize;
    for _ in 0..MAX_EVENTS_PER_SECTION {
        if i >= count {
            break;
        }
        i += draw.range_u32(profile.straight_points.0, profile.straight_points.1) as usize;
        if i >= count {
            break;
        }
        let span = draw
            .range_u32(profile.bend_points.0, profile.bend_points.1)
            .max(1) as usize;
        // 0.55..1.0 of the section's peak, so bends within a section still vary
        // in severity without any of them exceeding the profile.
        let amplitude = draw.sign() * peak * draw.range(0.55, 1.0);
        for k in 0..span {
            if i >= count {
                break;
            }
            // A half-sine bump: curvature ramps in and out, so the bend has an
            // entry, an apex and an exit rather than a step.
            let t = (k as f32 + 0.5) / span as f32;
            out[start + i] = amplitude * (std::f32::consts::PI * t).sin();
            i += 1;
        }
    }
}

/// Emit `count` grades for one section as alternating flats and smooth hills.
fn emit_hills(
    draw: &mut Draw,
    profile: &super::section::SectionProfile,
    tuning: &CourseTuning,
    count: usize,
    out: &mut Vec<f32>,
) {
    let start = out.len();
    out.resize(start + count, 0.0);
    let peak = profile.hilliness * tuning.max_grade;
    let mut i = 0usize;
    for _ in 0..MAX_EVENTS_PER_SECTION {
        if i >= count {
            break;
        }
        i += draw.range_u32(profile.hill_gap.0, profile.hill_gap.1) as usize;
        if i >= count {
            break;
        }
        let span = draw
            .range_u32(profile.hill_points.0, profile.hill_points.1)
            .max(1) as usize;
        let amplitude = draw.sign() * peak * draw.range(0.5, 1.0);
        for k in 0..span {
            if i >= count {
                break;
            }
            let t = (k as f32 + 0.5) / span as f32;
            out[start + i] = amplitude * (std::f32::consts::PI * t).sin();
            i += 1;
        }
    }
}

/// Emit `count` half-widths for one section: the section's nominal width with a
/// slow, smooth wander inside its jitter band.
fn emit_width(
    draw: &mut Draw,
    profile: &super::section::SectionProfile,
    tuning: &CourseTuning,
    count: usize,
    out: &mut Vec<f32>,
) {
    let phase = draw.range(0.0, std::f32::consts::TAU);
    let rate = draw.range(0.05, 0.16);
    for i in 0..count {
        let wander = profile.width_jitter * (phase + rate * i as f32).sin();
        out.push(
            (profile.half_width + wander)
                .clamp(tuning.min_half_width, tuning.max_half_width),
        );
    }
}

/// Clamp a signal to `±limit` and then relax it so no two adjacent values differ
/// by more than `max_delta`, in exactly `passes` bounded sweeps.
///
/// This is the whole "no impossible instantaneous reversal" guarantee. A sweep
/// walks forward pulling each value toward its predecessor, then backward doing
/// the same; both directions are needed or the constraint is only satisfied on
/// one side of a violation. `passes` is fixed, so the cost is `O(n · passes)`
/// and the result is deterministic even where a single sweep would not fully
/// converge — the final clamp guarantees the hard bound regardless.
fn relax(signal: &mut [f32], limit: f32, max_delta: f32, passes: u32) {
    for value in signal.iter_mut() {
        *value = value.clamp(-limit, limit);
    }
    for _ in 0..passes {
        for i in 1..signal.len() {
            let previous = signal[i - 1];
            signal[i] = signal[i].clamp(previous - max_delta, previous + max_delta);
        }
        for i in (0..signal.len().saturating_sub(1)).rev() {
            let next = signal[i + 1];
            signal[i] = signal[i].clamp(next - max_delta, next + max_delta);
        }
    }
    for value in signal.iter_mut() {
        *value = value.clamp(-limit, limit);
    }
}

/// Ramp the first and last `margin` points of both signals to zero, so the
/// start line and the finish arch are always on straight, level road.
fn flatten_ends(yaw_step: &mut [f32], grade: &mut [f32], margin: usize) {
    let n = yaw_step.len().min(grade.len());
    for i in 0..margin.min(n) {
        let k = i as f32 / margin.max(1) as f32;
        yaw_step[i] *= k;
        grade[i] *= k;
        let j = n - 1 - i;
        yaw_step[j] *= k;
        grade[j] *= k;
    }
}

/// Walk the corrected signals into world positions.
fn integrate(
    yaw_step: &[f32],
    grade: &[f32],
    half_width: &[f32],
    section: &[SectionKind],
    tuning: &CourseTuning,
) -> Vec<ControlPoint> {
    let count = yaw_step.len().min(grade.len()).min(half_width.len()).min(section.len());
    let mut points = Vec::with_capacity(count);
    let mut position = Vec3::ZERO;
    let mut heading = 0.0f32;
    for i in 0..count {
        heading += yaw_step[i];
        points.push(ControlPoint {
            position,
            heading,
            grade: grade[i],
            half_width: half_width[i],
            section: section[i],
        });
        // The step is a unit direction scaled by the control spacing, so the
        // spacing is a true 3D arc length and a steep grade does not silently
        // stretch the course.
        let pitch = grade[i].atan();
        let step = Vec3::new(
            heading.sin() * pitch.cos(),
            pitch.sin(),
            heading.cos() * pitch.cos(),
        );
        position = position.add(step.mul_scalar(tuning.control_spacing));
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generated() -> Vec<ControlPoint> {
        control_points(crate::DEFAULT_SEED, &CourseTuning::DEFAULT)
    }

    #[test]
    fn the_same_seed_generates_identical_control_points() {
        let a = generated();
        let b = generated();
        assert_eq!(a, b);
        assert!(a.len() > 150, "a 9 km course at 40 m spacing has many points");
    }

    #[test]
    fn different_seeds_generate_different_courses() {
        let a = control_points(1, &CourseTuning::DEFAULT);
        let b = control_points(2, &CourseTuning::DEFAULT);
        assert_eq!(a.len(), b.len(), "the pacing plan fixes the point count");
        assert_ne!(a, b, "but the road inside it differs");
        let drift = a
            .iter()
            .zip(&b)
            .map(|(p, q)| p.position.distance(q.position))
            .fold(0.0f32, f32::max);
        assert!(drift > 100.0, "the two courses diverge substantially: {drift} m");
    }

    #[test]
    fn every_generated_value_is_finite() {
        for p in generated() {
            assert!(p.position.x.is_finite() && p.position.y.is_finite() && p.position.z.is_finite());
            assert!(p.heading.is_finite());
            assert!(p.grade.is_finite());
            assert!(p.half_width.is_finite());
        }
    }

    #[test]
    fn the_hard_constraints_hold_on_every_generated_seed() {
        let t = CourseTuning::DEFAULT;
        for seed in [0u64, 1, 7, 99, 4242, 0xDEAD_BEEF, u64::MAX] {
            let points = control_points(seed, &t);
            for w in points.windows(2) {
                let yaw_step = w[1].heading - w[0].heading;
                assert!(
                    yaw_step.abs() <= t.max_yaw_step + 1.0e-4,
                    "seed {seed}: heading step {yaw_step} exceeds {}",
                    t.max_yaw_step
                );
                let grade_delta = w[1].grade - w[0].grade;
                assert!(
                    grade_delta.abs() <= t.max_grade_delta + 1.0e-4,
                    "seed {seed}: grade change {grade_delta} exceeds {}",
                    t.max_grade_delta
                );
            }
            for p in &points {
                assert!(p.grade.abs() <= t.max_grade + 1.0e-4, "seed {seed}: grade {}", p.grade);
                assert!(
                    (t.min_half_width..=t.max_half_width).contains(&p.half_width),
                    "seed {seed}: half width {}",
                    p.half_width
                );
            }
        }
    }

    /// The curvature-continuity bound is what stops an instant reversal, so it
    /// is asserted independently of the magnitude bound above.
    #[test]
    fn adjacent_heading_steps_never_jump() {
        let t = CourseTuning::DEFAULT;
        let points = control_points(crate::DEFAULT_SEED, &t);
        let steps: Vec<f32> = points.windows(2).map(|w| w[1].heading - w[0].heading).collect();
        for w in steps.windows(2) {
            assert!(
                (w[1] - w[0]).abs() <= t.max_yaw_step_delta + 1.0e-4,
                "heading step jumped by {}",
                w[1] - w[0]
            );
        }
    }

    #[test]
    fn the_start_and_the_finish_are_straight_and_level() {
        let points = generated();
        for p in points.iter().take(3) {
            assert!(p.grade.abs() < 0.02, "the start line is level");
        }
        let steps: Vec<f32> = points.windows(2).map(|w| w[1].heading - w[0].heading).collect();
        assert!(steps[0].abs() < 0.02, "the start line is straight");
        assert!(
            steps[steps.len() - 1].abs() < 0.02,
            "and so is the finish"
        );
    }

    #[test]
    fn the_sections_appear_in_order_and_cover_every_point() {
        let points = generated();
        let mut seen: Vec<SectionKind> = Vec::new();
        for p in &points {
            if seen.last() != Some(&p.section) {
                seen.push(p.section);
            }
        }
        assert_eq!(seen, SectionKind::ALL.to_vec());
    }

    /// The relaxation is the safety net, so it is tested directly on a signal
    /// engineered to violate both bounds.
    #[test]
    fn relaxation_enforces_both_bounds_on_a_hostile_signal() {
        let mut signal = vec![10.0, -10.0, 10.0, -10.0, 10.0, 0.0, 0.0, 0.0, 9.0];
        relax(&mut signal, 1.0, 0.25, 6);
        for v in &signal {
            assert!(v.abs() <= 1.0 + 1.0e-6, "magnitude bound: {v}");
        }
        for w in signal.windows(2) {
            assert!(
                (w[1] - w[0]).abs() <= 0.25 + 1.0e-6,
                "delta bound: {} -> {}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn relaxation_of_a_degenerate_signal_is_harmless() {
        let mut empty: Vec<f32> = Vec::new();
        relax(&mut empty, 1.0, 0.1, 3);
        assert!(empty.is_empty());
        let mut single = vec![5.0];
        relax(&mut single, 1.0, 0.1, 3);
        assert_eq!(single, vec![1.0]);
    }

    /// A profile whose straights are shorter than one point must still
    /// terminate — the event loops are bounded by construction, and this proves
    /// the bound is the thing doing the work.
    #[test]
    fn event_emission_terminates_under_a_degenerate_profile() {
        let mut draw = Draw::seeded(3);
        let profile = super::super::section::SectionProfile {
            length: 400.0,
            curviness: 1.0,
            bend_points: (0, 0),
            straight_points: (0, 0),
            hilliness: 1.0,
            hill_points: (0, 0),
            hill_gap: (0, 0),
            half_width: 8.0,
            width_jitter: 0.0,
        };
        let mut bends = Vec::new();
        emit_bends(&mut draw, &profile, &CourseTuning::DEFAULT, 10, &mut bends);
        assert_eq!(bends.len(), 10);
        let mut hills = Vec::new();
        emit_hills(&mut draw, &profile, &CourseTuning::DEFAULT, 10, &mut hills);
        assert_eq!(hills.len(), 10);
        let mut widths = Vec::new();
        emit_width(&mut draw, &profile, &CourseTuning::DEFAULT, 10, &mut widths);
        assert!(widths.iter().all(|w| (*w - 8.0).abs() < 1.0e-5));
    }

    #[test]
    fn flattening_the_ends_is_a_no_op_on_a_short_signal() {
        let mut yaw = vec![1.0, 1.0];
        let mut grade = vec![1.0, 1.0];
        flatten_ends(&mut yaw, &mut grade, 0);
        assert_eq!(yaw, vec![1.0, 1.0]);
        assert_eq!(grade, vec![1.0, 1.0]);
    }
}
