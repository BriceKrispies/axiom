//! **Geometry compilation**: authored sections in, one arc-length-uniform table
//! of [`TrackSample`]s out.
//!
//! # Why there is exactly one integration
//!
//! Every section states a *signal* — a heading rate, a grade, a bank, a width —
//! as a function of how far along itself it is. The compiler lays those signals
//! end to end into one array over the whole course, corrects the array, and then
//! integrates it **once**, from the start line to the finish. Position, tangent
//! and heading are therefore continuous by construction rather than by
//! agreement: there is nothing to agree, because nothing restarts at a section
//! boundary. An S-bend cannot be faked by teleporting between unrelated sampled
//! points here, because no section ever writes a position at all.
//!
//! The correction pass is the same bounded relaxation the previous generator
//! used, moved from control points to samples: clamp each signal's magnitude,
//! then limit how fast it may change between adjacent samples, in a fixed number
//! of forward+backward sweeps. Every loop has a compile-time bound; there is no
//! "retry until valid". What the pass *cannot* do is silently rewrite an author's
//! intent — where a clamp actually bites, the count comes back in
//! [`GeometryClamps`] and the validator turns it into a warning naming the
//! section.
//!
//! ```text
//! [ExpandedSection]  ──signals──▶  curvature[]  grade[]  bank[]  half_width[]
//!                                        │
//!                                   correct (clamp + rate-limit, bounded)
//!                                        │
//!                                   integrate once
//!                                        ▼
//!                                  Vec<TrackSample>
//! ```

use axiom_math::Vec3;

use crate::course::compiler::ExpandedSection;
use crate::course::error::{CourseError, CourseErrorCode, CourseResult};
use crate::course::specification::{
    BankingMode, RoadModifierSpec, SectionId, SectionKind, ValidationThresholds,
};
use crate::track::{unit_or, TrackSample};
use crate::tuning::CourseTuning;

/// One compiled section: where it is, what it is, and what it inherits.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSection {
    /// The stable authored name.
    pub id: SectionId,
    /// Dense index — what a [`TrackSample`] carries and the plan indexes by.
    pub index: u16,
    /// Where the section starts along the course (m).
    pub start_m: f32,
    /// Where it ends (m).
    pub end_m: f32,
    /// The environment/scenery profile it names.
    pub environment: SectionKind,
    /// The speed a competent player is expected to carry here (m/s).
    pub expected_speed_mps: f32,
    /// The lane count it authored.
    pub lanes: u32,
    /// Which primitive it was built from — for dumps and diagnostics.
    pub primitive: &'static str,
}

impl CompiledSection {
    /// How much road the section covers (m).
    pub fn length_m(&self) -> f32 {
        self.end_m - self.start_m
    }

    /// Whether `distance` falls inside this section.
    pub fn contains(&self, distance_m: f32) -> bool {
        (distance_m >= self.start_m) & (distance_m < self.end_m)
    }
}

/// How often each correction actually bit, per section index.
///
/// A non-zero count is not an error — clamping is what keeps the road drivable —
/// but it *is* a report that the compiled road is not the road that was
/// authored, and the validator says so by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GeometryClamps {
    /// Samples whose curvature was clamped to the minimum-radius limit.
    pub curvature: u32,
    /// Samples whose grade was clamped to the maximum-grade limit.
    pub grade: u32,
    /// Samples whose banking was clamped.
    pub bank: u32,
    /// Samples whose half-width was clamped to the road's legal band.
    pub width: u32,
}

impl GeometryClamps {
    /// Whether anything was clamped at all.
    pub fn any(&self) -> bool {
        (self.curvature | self.grade | self.bank | self.width) != 0
    }
}

/// The finished road.
#[derive(Debug, Clone)]
pub struct CompiledGeometry {
    /// The arc-length-uniform sample table.
    pub samples: Vec<TrackSample>,
    /// The compiled sections, in course order.
    pub sections: Vec<CompiledSection>,
    /// Total course length (m).
    pub length_m: f32,
    /// Per-section clamp counts, indexed by [`CompiledSection::index`].
    pub clamps: Vec<GeometryClamps>,
}

/// The bounds the correction pass enforces, resolved once from the course
/// tuning and the authored thresholds.
#[derive(Debug, Clone, Copy)]
struct Limits {
    spacing: f32,
    max_curvature: f32,
    max_curvature_step: f32,
    max_grade: f32,
    max_grade_step: f32,
    max_bank: f32,
    max_bank_step: f32,
    min_half_width: f32,
    max_half_width: f32,
    max_width_step: f32,
    lane_width: f32,
    lane_shoulder: f32,
    bank_per_curvature: f32,
}

impl Limits {
    fn resolve(tuning: &CourseTuning, thresholds: &ValidationThresholds) -> Limits {
        Limits {
            spacing: tuning.sample_spacing,
            max_curvature: 1.0 / thresholds.min_turn_radius_m.max(1.0),
            max_curvature_step: thresholds.max_curvature_step,
            max_grade: thresholds.max_grade,
            max_grade_step: thresholds.max_grade_step,
            max_bank: thresholds.max_bank_rad,
            max_bank_step: thresholds.max_bank_step,
            min_half_width: tuning.min_half_width,
            max_half_width: tuning.max_half_width,
            max_width_step: WIDTH_RAMP_PER_METRE * tuning.sample_spacing,
            lane_width: tuning.lane_width,
            lane_shoulder: tuning.lane_shoulder,
            bank_per_curvature: tuning.bank_per_curvature,
        }
    }

    fn half_width_for_lanes(&self, lanes: f32) -> f32 {
        lanes * self.lane_width * 0.5 + self.lane_shoulder
    }
}

/// How fast the tarmac may widen or narrow (metres of half-width per metre of
/// course).
///
/// The road has to breathe *through* a lane transition rather than stepping at
/// it, and this is also what keeps a join between two sections of different
/// widths from being a visible ledge in the mesh.
pub const WIDTH_RAMP_PER_METRE: f32 = 0.06;

/// Bounded relaxation sweeps applied to every corrected signal.
const CORRECTION_PASSES: u32 = 6;

/// Box-smoothing passes applied to the banking signal, so the road rolls into a
/// corner rather than stepping into it.
const BANK_SMOOTHING_PASSES: u32 = 8;

/// Compile the expanded course into its sample table.
pub fn compile(
    sections: &[ExpandedSection],
    tuning: &CourseTuning,
    thresholds: &ValidationThresholds,
) -> CourseResult<CompiledGeometry> {
    let limits = Limits::resolve(tuning, thresholds);
    (!sections.is_empty()).then_some(()).ok_or_else(|| {
        CourseError::new(
            CourseErrorCode::EmptyCourse,
            "the expanded course has no sections".to_string(),
        )
    })?;

    // Every authored lane count has to fit the tarmac the course allows. A
    // count that does not is rejected here rather than silently clamped: a
    // section that believes it has seven lanes and is compiled with five is a
    // course whose traffic plan refers to lanes that do not exist.
    sections.iter().try_for_each(|section| {
        let width = limits.half_width_for_lanes(section.lanes as f32);
        ((width >= limits.min_half_width - 1.0e-3) & (width <= limits.max_half_width + 1.0e-3))
            .then_some(())
            .ok_or_else(|| {
                CourseError::new(
                    CourseErrorCode::InvalidLaneCount,
                    format!(
                        "{} lanes needs a {width:.2} m half-width, outside the course's \
                         {:.2}..{:.2} m band",
                        section.lanes, limits.min_half_width, limits.max_half_width
                    ),
                )
                .in_section(section.id.as_str())
                .in_field("lanes")
            })
    })?;

    let plan = lay_out(sections, &limits);
    let mut signals = author_signals(sections, &plan, &limits);
    let clamps = correct(&mut signals, &plan, &limits);
    let mut samples = integrate(&signals, &plan, &limits);
    // The environment and the expected speed are per-section labels, stamped
    // onto every sample the section owns so the renderer, the HUD and the
    // validator can all read them from the one table.
    samples.iter_mut().for_each(|sample| {
        let owner = &sections[sample.section_index as usize];
        sample.section = owner.environment;
        sample.expected_speed = owner.expected_speed_mps;
    });
    let length_m = samples.last().map(|s| s.distance).unwrap_or(0.0);

    let compiled = plan
        .sections
        .iter()
        .enumerate()
        .map(|(i, span)| CompiledSection {
            id: sections[i].id.clone(),
            index: i as u16,
            start_m: span.first as f32 * limits.spacing,
            end_m: (span.first + span.count) as f32 * limits.spacing,
            environment: sections[i].environment,
            expected_speed_mps: sections[i].expected_speed_mps,
            lanes: sections[i].lanes,
            primitive: sections[i].primitive.token(),
        })
        .collect();

    Ok(CompiledGeometry {
        samples,
        sections: compiled,
        length_m,
        clamps,
    })
}

/// Where each section's samples sit in the global array.
#[derive(Debug, Clone, Copy)]
struct Span {
    first: usize,
    count: usize,
}

#[derive(Debug, Clone)]
struct Layout {
    sections: Vec<Span>,
    /// The section each sample belongs to.
    owner: Vec<u16>,
    /// The fraction along its own section each sample sits at.
    local_t: Vec<f32>,
    /// Total samples, including the closing one.
    total: usize,
}

/// Allot samples to sections. Each section gets at least two, so even a very
/// short one has a start and an end.
fn lay_out(sections: &[ExpandedSection], limits: &Limits) -> Layout {
    let mut spans = Vec::with_capacity(sections.len());
    let mut owner = Vec::new();
    let mut local_t = Vec::new();
    let mut cursor = 0usize;
    for (index, section) in sections.iter().enumerate() {
        let count = (section.primitive.length_m() / limits.spacing)
            .round()
            .max(2.0) as usize;
        spans.push(Span {
            first: cursor,
            count,
        });
        (0..count).for_each(|i| {
            owner.push(index as u16);
            local_t.push(i as f32 / count as f32);
        });
        cursor += count;
    }
    // One closing sample so the last section has an end point to draw to.
    owner.push((sections.len() - 1) as u16);
    local_t.push(1.0);
    Layout {
        sections: spans,
        owner,
        local_t,
        total: cursor + 1,
    }
}

/// The per-sample signals, before correction.
#[derive(Debug, Clone)]
struct Signals {
    curvature: Vec<f32>,
    grade: Vec<f32>,
    /// The bank a section explicitly asked for, or `None` to follow curvature.
    bank_override: Vec<Option<f32>>,
    bank_scale: Vec<f32>,
    bank_ceiling: Vec<f32>,
    half_width: Vec<f32>,
}

/// Read every section's primitive and modifiers into the global signal arrays.
fn author_signals(sections: &[ExpandedSection], plan: &Layout, limits: &Limits) -> Signals {
    let mut signals = Signals {
        curvature: Vec::with_capacity(plan.total),
        grade: Vec::with_capacity(plan.total),
        bank_override: Vec::with_capacity(plan.total),
        bank_scale: Vec::with_capacity(plan.total),
        bank_ceiling: Vec::with_capacity(plan.total),
        half_width: Vec::with_capacity(plan.total),
    };
    for i in 0..plan.total {
        let owner = plan.owner[i] as usize;
        let section = &sections[owner];
        let t = plan.local_t[i];
        let local_s = t * section.primitive.length_m();

        let mut curvature = section.primitive.heading_rate(t);
        let mut grade = section.primitive.grade(t);
        let mut bank_override = section.primitive.bank_rad(t);
        let mut bank_scale = 1.0f32;
        let mut bank_ceiling = limits.max_bank;
        let mut lanes = section
            .primitive
            .lanes(t)
            .unwrap_or(section.lanes as f32);
        let mut half_width = section.primitive.half_width_m(t);

        for modifier in &section.modifiers {
            match *modifier {
                RoadModifierSpec::LateralWave {
                    amplitude_m,
                    wavelength_m,
                    phase_rad,
                } => {
                    // A sideways weave is realised as **curvature**, not as a
                    // displacement added to the finished positions: displacing a
                    // centreline after the fact leaves every tangent — and
                    // therefore every lane, barrier and prop — pointing where it
                    // was before the wave existed.
                    //
                    // The cosine is load-bearing. Heading is the integral of
                    // curvature, so `κ = −Ak²·cos` integrates to
                    // `θ = −Ak·sin`, which is bounded and averages zero. Using
                    // a sine here integrates to `θ = Ak(cos − 1)`, which never
                    // goes positive — the road would drift steadily to one side
                    // and the "wave" would be a very long turn.
                    let k = std::f32::consts::TAU / wavelength_m.max(1.0e-3);
                    curvature += -amplitude_m * k * k * (k * local_s + phase_rad).cos();
                }
                RoadModifierSpec::ElevationWave {
                    amplitude_m,
                    wavelength_m,
                    phase_rad,
                } => {
                    let k = std::f32::consts::TAU / wavelength_m.max(1.0e-3);
                    grade += amplitude_m * k * (k * local_s + phase_rad).cos();
                }
                RoadModifierSpec::Banking {
                    mode,
                    strength,
                    maximum_rad,
                } => match mode {
                    BankingMode::FollowCurvature => {
                        bank_scale = strength;
                        bank_ceiling = maximum_rad.min(limits.max_bank);
                    }
                    // A literal constant bank across the section. `strength` is
                    // signed, so `strength = -1` banks the other way.
                    BankingMode::Fixed => {
                        bank_override = Some(maximum_rad * strength);
                        bank_ceiling = maximum_rad.min(limits.max_bank);
                    }
                    BankingMode::Flat => {
                        bank_override = Some(0.0);
                        bank_ceiling = 0.0;
                    }
                },
                RoadModifierSpec::GradeProfile { .. } => {
                    // Constant across the section, so a figure cut into several
                    // sections descends through the joins rather than levelling
                    // off at each one.
                    grade += modifier
                        .sustained_grade(section.primitive.length_m())
                        .unwrap_or(0.0);
                }
                RoadModifierSpec::WidthProfile {
                    start_half_width_m,
                    end_half_width_m,
                } => {
                    half_width = Some(
                        start_half_width_m
                            + (end_half_width_m - start_half_width_m)
                                * crate::course::specification::road::smoothstep(t),
                    );
                }
                RoadModifierSpec::LaneProfile {
                    start_lanes,
                    end_lanes,
                } => {
                    lanes = start_lanes as f32
                        + (end_lanes as f32 - start_lanes as f32)
                            * crate::course::specification::road::smoothstep(t);
                }
            }
        }

        signals.curvature.push(curvature);
        signals.grade.push(grade);
        signals.bank_override.push(bank_override);
        signals.bank_scale.push(bank_scale);
        signals.bank_ceiling.push(bank_ceiling);
        signals
            .half_width
            .push(half_width.unwrap_or_else(|| limits.half_width_for_lanes(lanes)));
    }
    signals
}

/// Clamp and rate-limit every signal, counting where each clamp bit.
fn correct(signals: &mut Signals, plan: &Layout, limits: &Limits) -> Vec<GeometryClamps> {
    let mut clamps = vec![GeometryClamps::default(); plan.sections.len()];

    let bitten = relax(
        &mut signals.curvature,
        limits.max_curvature,
        limits.max_curvature_step,
    );
    charge(&bitten, plan, &mut clamps, |c| &mut c.curvature);

    let bitten = relax(&mut signals.grade, limits.max_grade, limits.max_grade_step);
    charge(&bitten, plan, &mut clamps, |c| &mut c.grade);

    // Width is a band rather than a symmetric magnitude, so it gets its own
    // clamp before the shared rate limiter.
    let bitten: Vec<bool> = signals
        .half_width
        .iter_mut()
        .map(|w| {
            let clamped = w.clamp(limits.min_half_width, limits.max_half_width);
            let bit = (clamped - *w).abs() > 1.0e-4;
            *w = clamped;
            bit
        })
        .collect();
    charge(&bitten, plan, &mut clamps, |c| &mut c.width);
    rate_limit(&mut signals.half_width, limits.max_width_step);

    // Banking is resolved last, because "follow the curvature" means the
    // *corrected* curvature.
    let mut bank: Vec<f32> = (0..plan.total)
        .map(|i| {
            signals.bank_override[i].unwrap_or(
                -limits.bank_per_curvature * signals.bank_scale[i] * signals.curvature[i],
            )
        })
        .collect();
    smooth(&mut bank, BANK_SMOOTHING_PASSES);
    let bitten: Vec<bool> = bank
        .iter_mut()
        .enumerate()
        .map(|(i, b)| {
            let ceiling = signals.bank_ceiling[i].min(limits.max_bank);
            let clamped = b.clamp(-ceiling, ceiling);
            let bit = (clamped - *b).abs() > 1.0e-4;
            *b = clamped;
            bit
        })
        .collect();
    charge(&bitten, plan, &mut clamps, |c| &mut c.bank);
    rate_limit(&mut bank, limits.max_bank_step);
    signals.bank_override = bank.into_iter().map(Some).collect();

    clamps
}

/// Attribute a per-sample clamp flag to the section that owns the sample.
fn charge(
    bitten: &[bool],
    plan: &Layout,
    clamps: &mut [GeometryClamps],
    field: impl Fn(&mut GeometryClamps) -> &mut u32,
) {
    bitten.iter().enumerate().for_each(|(i, bit)| {
        bit.then(|| {
            let owner = plan.owner[i] as usize;
            *field(&mut clamps[owner]) += 1;
        });
    });
}

/// Clamp a signal to `±limit`, then relax it so no two adjacent values differ by
/// more than `max_delta`, in exactly [`CORRECTION_PASSES`] bounded sweeps.
///
/// Returns, per sample, whether the *magnitude* clamp bit — the rate limit is
/// ordinary smoothing and is not reported, but a clamped magnitude means the
/// author asked for road the course does not allow.
fn relax(signal: &mut [f32], limit: f32, max_delta: f32) -> Vec<bool> {
    let bitten: Vec<bool> = signal
        .iter_mut()
        .map(|value| {
            let clamped = value.clamp(-limit, limit);
            let bit = (clamped - *value).abs() > 1.0e-5;
            *value = clamped;
            bit
        })
        .collect();
    rate_limit(signal, max_delta);
    signal
        .iter_mut()
        .for_each(|value| *value = value.clamp(-limit, limit));
    bitten
}

/// Limit how fast a signal may change between adjacent samples, forward and
/// backward. Both directions are needed, or the constraint is only satisfied on
/// one side of a violation.
fn rate_limit(signal: &mut [f32], max_delta: f32) {
    for _ in 0..CORRECTION_PASSES {
        for i in 1..signal.len() {
            let previous = signal[i - 1];
            signal[i] = signal[i].clamp(previous - max_delta, previous + max_delta);
        }
        for i in (0..signal.len().saturating_sub(1)).rev() {
            let next = signal[i + 1];
            signal[i] = signal[i].clamp(next - max_delta, next + max_delta);
        }
    }
}

/// A fixed number of three-tap box smoothing passes. Bounded by construction.
fn smooth(signal: &mut [f32], passes: u32) {
    for _ in 0..passes {
        let previous = signal.to_vec();
        for i in 1..previous.len().saturating_sub(1) {
            signal[i] = (previous[i - 1] + previous[i] * 2.0 + previous[i + 1]) * 0.25;
        }
    }
}

/// Walk the corrected signals into the sample table. **The one integration.**
fn integrate(signals: &Signals, plan: &Layout, limits: &Limits) -> Vec<TrackSample> {
    let mut position = Vec3::ZERO;
    let mut heading = 0.0f32;
    let mut samples = Vec::with_capacity(plan.total);
    for i in 0..plan.total {
        let grade = signals.grade[i];
        let pitch = grade.atan();
        // The tangent is analytic rather than a difference of neighbours, so it
        // is exactly unit and exactly continuous — a central difference across a
        // clamped signal is neither.
        let tangent = Vec3::new(
            heading.sin() * pitch.cos(),
            pitch.sin(),
            heading.cos() * pitch.cos(),
        );
        let flat_right = unit_or(Vec3::UNIT_Y.cross(tangent), Vec3::UNIT_X);
        let flat_up = unit_or(tangent.cross(flat_right), Vec3::UNIT_Y);
        let bank = signals.bank_override[i].unwrap_or(0.0);
        let (sin_b, cos_b) = bank.sin_cos();
        samples.push(TrackSample {
            position,
            tangent,
            right: flat_right.mul_scalar(cos_b).add(flat_up.mul_scalar(sin_b)),
            up: flat_up.mul_scalar(cos_b).subtract(flat_right.mul_scalar(sin_b)),
            distance: i as f32 * limits.spacing,
            heading,
            curvature: signals.curvature[i],
            grade,
            bank,
            half_width: signals.half_width[i],
            section: SectionKind::StartStraight,
            section_index: plan.owner[i],
            expected_speed: 0.0,
        });
        // The step is a unit direction scaled by the sample spacing, so the
        // spacing is a true 3D arc length and a steep grade does not silently
        // stretch the course.
        position = position.add(tangent.mul_scalar(limits.spacing));
        heading += signals.curvature[i] * limits.spacing;
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::compiler::ExpandedSection;
    use crate::course::specification::{RoadPrimitiveSpec, TurnDirection};

    fn section(id: &str, primitive: RoadPrimitiveSpec) -> ExpandedSection {
        ExpandedSection {
            id: SectionId::new(id),
            primitive,
            modifiers: Vec::new(),
            lanes: 5,
            expected_speed_mps: 80.0,
            environment: SectionKind::StartStraight,
        }
    }

    fn build(sections: &[ExpandedSection]) -> CompiledGeometry {
        compile(sections, &CourseTuning::DEFAULT, &ValidationThresholds::DEFAULT)
            .expect("compiles")
    }

    #[test]
    fn a_straight_is_its_authored_length_sampled_at_the_course_spacing() {
        let g = build(&[section(
            "s",
            RoadPrimitiveSpec::Straight { length_m: 400.0 },
        )]);
        assert_eq!(g.samples.len(), 201, "200 spans of 2 m plus the end point");
        assert!((g.length_m - 400.0).abs() < 1.0e-3);
        assert_eq!(g.sections.len(), 1);
        assert_eq!(g.sections[0].start_m, 0.0);
        assert!((g.sections[0].end_m - 400.0).abs() < 1.0e-3);
        assert!((g.sections[0].length_m() - 400.0).abs() < 1.0e-3);
        assert_eq!(g.sections[0].primitive, "straight");
        for (i, s) in g.samples.iter().enumerate() {
            assert!((s.distance - i as f32 * 2.0).abs() < 1.0e-2);
            assert!(s.curvature.abs() < 1.0e-6, "a straight does not bend");
            assert!(s.grade.abs() < 1.0e-6, "and does not climb");
            assert_eq!(s.section_index, 0);
        }
        // Straight really means straight: the whole thing lies on +Z.
        let last = g.samples.last().unwrap();
        assert!(last.position.x.abs() < 1.0e-3, "x drift {}", last.position.x);
        assert!((last.position.z - 400.0).abs() < 1.0e-2);
    }

    #[test]
    fn a_turn_bends_the_authored_way_and_holds_the_authored_radius() {
        for (direction, sign) in [(TurnDirection::Right, 1.0f32), (TurnDirection::Left, -1.0)] {
            let g = build(&[section(
                "t",
                RoadPrimitiveSpec::Turn {
                    length_m: 600.0,
                    radius_m: 200.0,
                    direction,
                },
            )]);
            let middle = g.samples[g.samples.len() / 2];
            assert!(
                (middle.curvature - sign / 200.0).abs() < 5.0e-4,
                "{direction:?}: curvature {} wanted {}",
                middle.curvature,
                sign / 200.0
            );
            // The heading really turned, by roughly (arc / radius) allowing for
            // the eased ends.
            let turned = g.samples.last().unwrap().heading - g.samples[0].heading;
            assert!(
                turned * sign > 1.5,
                "{direction:?}: only turned {turned} rad"
            );
        }
    }

    #[test]
    fn an_s_bend_is_continuous_through_its_reversal() {
        let g = build(&[section(
            "s",
            RoadPrimitiveSpec::SBend {
                length_m: 600.0,
                radius_m: 220.0,
                first: TurnDirection::Right,
            },
        )]);
        let quarter = g.samples[g.samples.len() / 4].curvature;
        let three_quarter = g.samples[g.samples.len() * 3 / 4].curvature;
        assert!(quarter > 0.0, "first half turns right: {quarter}");
        assert!(three_quarter < 0.0, "second half turns left: {three_quarter}");
        // No step anywhere, including through the reversal.
        for pair in g.samples.windows(2) {
            assert!(
                (pair[1].curvature - pair[0].curvature).abs()
                    <= ValidationThresholds::DEFAULT.max_curvature_step + 1.0e-6,
                "curvature stepped by {} at {} m",
                pair[1].curvature - pair[0].curvature,
                pair[0].distance
            );
        }
        // And the road comes back to roughly the heading it started on.
        let net = g.samples.last().unwrap().heading - g.samples[0].heading;
        assert!(net.abs() < 0.05, "an S-bend nets out: {net} rad");
    }

    #[test]
    fn a_crest_climbs_and_a_dip_falls_and_both_return_to_level() {
        let crest = build(&[section(
            "c",
            RoadPrimitiveSpec::Crest {
                length_m: 400.0,
                height_m: 10.0,
            },
        )]);
        let peak = crest
            .samples
            .iter()
            .map(|s| s.position.y)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(peak > 8.0, "the crest reached only {peak} m");
        let end = crest.samples.last().unwrap().position.y;
        assert!(end.abs() < 0.6, "and came back to level, ended at {end} m");

        let dip = build(&[section(
            "d",
            RoadPrimitiveSpec::Dip {
                length_m: 400.0,
                depth_m: 10.0,
            },
        )]);
        let bottom = dip
            .samples
            .iter()
            .map(|s| s.position.y)
            .fold(f32::INFINITY, f32::min);
        assert!(bottom < -8.0, "the dip only reached {bottom} m");
    }

    #[test]
    fn a_bank_transition_rolls_smoothly_from_one_angle_to_the_other() {
        let g = build(&[section(
            "b",
            RoadPrimitiveSpec::BankTransition {
                length_m: 400.0,
                from_rad: 0.0,
                to_rad: 0.12,
            },
        )]);
        assert!(g.samples[0].bank.abs() < 0.02);
        assert!(
            (g.samples.last().unwrap().bank - 0.12).abs() < 0.02,
            "ended at {}",
            g.samples.last().unwrap().bank
        );
        for pair in g.samples.windows(2) {
            assert!(
                (pair[1].bank - pair[0].bank).abs()
                    <= ValidationThresholds::DEFAULT.max_bank_step + 1.0e-6,
                "bank stepped by {}",
                pair[1].bank - pair[0].bank
            );
        }
    }

    #[test]
    fn a_lane_transition_narrows_the_tarmac_without_a_ledge() {
        let g = build(&[
            ExpandedSection {
                lanes: 5,
                ..section("wide", RoadPrimitiveSpec::Straight { length_m: 200.0 })
            },
            ExpandedSection {
                lanes: 3,
                ..section(
                    "narrow",
                    RoadPrimitiveSpec::LaneTransition {
                        length_m: 200.0,
                        from_lanes: 5,
                        to_lanes: 3,
                    },
                )
            },
            ExpandedSection {
                lanes: 3,
                ..section("held", RoadPrimitiveSpec::Straight { length_m: 200.0 })
            },
        ]);
        let first = g.samples[0].half_width;
        let last = g.samples.last().unwrap().half_width;
        assert!((first - 9.5).abs() < 0.1, "five lanes is 9.5 m: {first}");
        assert!((last - 6.0).abs() < 0.1, "three lanes is 6.0 m: {last}");
        for pair in g.samples.windows(2) {
            assert!(
                (pair[1].half_width - pair[0].half_width).abs() <= WIDTH_RAMP_PER_METRE * 2.0 + 1.0e-4,
                "the tarmac stepped by {} m at {} m",
                pair[1].half_width - pair[0].half_width,
                pair[0].distance
            );
        }
    }

    #[test]
    fn several_sections_connect_without_a_discontinuity() {
        let g = build(&[
            section("a", RoadPrimitiveSpec::Straight { length_m: 300.0 }),
            section(
                "b",
                RoadPrimitiveSpec::Turn {
                    length_m: 400.0,
                    radius_m: 180.0,
                    direction: TurnDirection::Right,
                },
            ),
            section(
                "c",
                RoadPrimitiveSpec::Crest {
                    length_m: 300.0,
                    height_m: 8.0,
                },
            ),
            section(
                "d",
                RoadPrimitiveSpec::SBend {
                    length_m: 400.0,
                    radius_m: 200.0,
                    first: TurnDirection::Left,
                },
            ),
        ]);
        assert_eq!(g.sections.len(), 4);
        // Sections tile the course with no gap and no overlap.
        assert_eq!(g.sections[0].start_m, 0.0);
        for pair in g.sections.windows(2) {
            assert!((pair[1].start_m - pair[0].end_m).abs() < 1.0e-3);
        }
        assert!((g.sections.last().unwrap().end_m - g.length_m).abs() < 1.0e-3);

        for pair in g.samples.windows(2) {
            // Position: exactly one spacing apart, everywhere.
            let step = pair[1].position.distance(pair[0].position);
            assert!(
                (step - 2.0).abs() < 1.0e-2,
                "position stepped {step} m at {} m",
                pair[0].distance
            );
            // Tangent: no kink.
            assert!(
                pair[0].tangent.dot(pair[1].tangent) > 0.999,
                "tangent kinked at {} m",
                pair[0].distance
            );
            assert!(
                (pair[1].curvature - pair[0].curvature).abs()
                    <= ValidationThresholds::DEFAULT.max_curvature_step + 1.0e-6
            );
            assert!(
                (pair[1].grade - pair[0].grade).abs()
                    <= ValidationThresholds::DEFAULT.max_grade_step + 1.0e-6
            );
        }
        // Every frame is orthonormal and the road is never inverted.
        for s in &g.samples {
            assert!((s.tangent.length() - 1.0).abs() < 1.0e-4);
            assert!((s.right.length() - 1.0).abs() < 1.0e-4);
            assert!((s.up.length() - 1.0).abs() < 1.0e-4);
            assert!(s.tangent.dot(s.right).abs() < 1.0e-4);
            assert!(s.right.dot(s.up).abs() < 1.0e-4);
            assert!(s.up.y > 0.5);
        }
    }

    #[test]
    fn compilation_is_a_pure_function_of_its_input() {
        let sections = [
            section("a", RoadPrimitiveSpec::Straight { length_m: 300.0 }),
            section(
                "b",
                RoadPrimitiveSpec::Turn {
                    length_m: 400.0,
                    radius_m: 180.0,
                    direction: TurnDirection::Right,
                },
            ),
        ];
        assert_eq!(build(&sections).samples, build(&sections).samples);
    }

    #[test]
    fn a_lateral_wave_genuinely_bends_the_road_rather_than_sliding_it() {
        let mut waved = section("w", RoadPrimitiveSpec::Straight { length_m: 800.0 });
        waved.modifiers.push(RoadModifierSpec::LateralWave {
            amplitude_m: 6.0,
            wavelength_m: 300.0,
            phase_rad: 0.0,
        });
        let g = build(&[waved]);
        // It weaves: the heading really changes sign.
        let headings: Vec<f32> = g.samples.iter().map(|s| s.heading).collect();
        let max = headings.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let min = headings.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(max > 0.02 && min < -0.02, "no weave: {min}..{max}");
        // And the tangent follows the road, which is the whole point of doing
        // it as curvature: a displaced centreline would leave these at zero.
        assert!(g
            .samples
            .iter()
            .any(|s| s.tangent.x.abs() > 0.02), "the tangents never left +Z");
    }

    /// The modifier the whole elevation story rests on: a section that ends
    /// lower than it began, and several of them descending *through* the joins.
    #[test]
    fn a_grade_profile_drops_the_road_and_keeps_dropping_across_a_join() {
        let mut falling = section("d", RoadPrimitiveSpec::Straight { length_m: 400.0 });
        falling
            .modifiers
            .push(RoadModifierSpec::GradeProfile { drop_m: 30.0 });
        let g = build(&[falling.clone()]);
        let end = g.samples.last().unwrap().position.y;
        assert!(
            (end + 30.0).abs() < 2.5,
            "asked for a 30 m drop, got {:.1} m",
            -end
        );
        // Monotone: it descends the whole way, it does not dip and recover.
        assert!(g
            .samples
            .windows(2)
            .all(|w| w[1].position.y <= w[0].position.y + 1.0e-3));

        // Two of them in a row keep descending through the join — the grade does
        // not return to zero between them.
        let mut second = falling.clone();
        second.id = SectionId::new("d2");
        let joined = build(&[falling, second]);
        let middle = joined.samples[joined.samples.len() / 2];
        assert!(
            middle.grade < -0.05,
            "the road levelled off at the join: grade {}",
            middle.grade
        );
        assert!(
            (joined.samples.last().unwrap().position.y + 60.0).abs() < 4.0,
            "two 30 m drops should be ~60 m, got {:.1}",
            -joined.samples.last().unwrap().position.y
        );

        // And a negative drop climbs.
        let mut rising = section("u", RoadPrimitiveSpec::Straight { length_m: 400.0 });
        rising
            .modifiers
            .push(RoadModifierSpec::GradeProfile { drop_m: -20.0 });
        assert!(build(&[rising]).samples.last().unwrap().position.y > 15.0);
    }

    #[test]
    fn an_elevation_wave_rolls_the_road() {
        let mut waved = section("w", RoadPrimitiveSpec::Straight { length_m: 800.0 });
        waved.modifiers.push(RoadModifierSpec::ElevationWave {
            amplitude_m: 5.0,
            wavelength_m: 400.0,
            phase_rad: 0.0,
        });
        let g = build(&[waved]);
        let high = g.samples.iter().map(|s| s.position.y).fold(f32::NEG_INFINITY, f32::max);
        let low = g.samples.iter().map(|s| s.position.y).fold(f32::INFINITY, f32::min);
        assert!(high - low > 4.0, "the road only rolled {} m", high - low);
    }

    #[test]
    fn the_banking_modes_all_do_what_they_say() {
        let turn = RoadPrimitiveSpec::Turn {
            length_m: 500.0,
            radius_m: 160.0,
            direction: TurnDirection::Right,
        };
        let mut flat = section("f", turn);
        flat.modifiers.push(RoadModifierSpec::Banking {
            mode: BankingMode::Flat,
            strength: 1.0,
            maximum_rad: 0.2,
        });
        let g = build(&[flat]);
        assert!(g.samples.iter().all(|s| s.bank.abs() < 1.0e-3), "flat is flat");

        let mut fixed = section("x", turn);
        fixed.modifiers.push(RoadModifierSpec::Banking {
            mode: BankingMode::Fixed,
            strength: 1.0,
            maximum_rad: 0.09,
        });
        let g = build(&[fixed]);
        let middle = g.samples[g.samples.len() / 2].bank;
        assert!((middle - 0.09).abs() < 0.01, "fixed bank was {middle}");

        // Follow-curvature leans *into* the corner: on a right-hander the left
        // (outside) edge is the raised one.
        let mut follow = section("c", turn);
        follow.modifiers.push(RoadModifierSpec::Banking {
            mode: BankingMode::FollowCurvature,
            strength: 1.0,
            maximum_rad: 0.14,
        });
        let g = build(&[follow]);
        let bend = g.samples[g.samples.len() / 2];
        assert!(bend.curvature > 0.0);
        assert!(bend.bank < -0.01, "no lean: {}", bend.bank);
        let outside = bend.at_lateral(-bend.half_width);
        let inside = bend.at_lateral(bend.half_width);
        assert!(outside.y > inside.y, "the outside edge is raised");
    }

    #[test]
    fn a_road_the_course_forbids_is_clamped_and_reported() {
        // A wave far too violent for the minimum turn radius.
        let mut violent = section("v", RoadPrimitiveSpec::Straight { length_m: 600.0 });
        violent.modifiers.push(RoadModifierSpec::LateralWave {
            amplitude_m: 40.0,
            wavelength_m: 120.0,
            phase_rad: 0.0,
        });
        let g = build(&[violent]);
        assert!(g.clamps[0].curvature > 0, "the clamp did not report itself");
        assert!(g.clamps[0].any());
        let limit = 1.0 / ValidationThresholds::DEFAULT.min_turn_radius_m;
        assert!(
            g.samples.iter().all(|s| s.curvature.abs() <= limit + 1.0e-5),
            "and it did not actually hold the limit"
        );

        // A crest far too steep for the maximum grade.
        let steep = section(
            "g",
            RoadPrimitiveSpec::Crest {
                length_m: 120.0,
                height_m: 40.0,
            },
        );
        let g = build(&[steep]);
        assert!(g.clamps[0].grade > 0);
        assert!(g
            .samples
            .iter()
            .all(|s| s.grade.abs() <= ValidationThresholds::DEFAULT.max_grade + 1.0e-4));

        // And a course inside its limits reports nothing.
        let g = build(&[section("q", RoadPrimitiveSpec::Straight { length_m: 200.0 })]);
        assert!(!g.clamps[0].any(), "a legal road reported a clamp");
    }

    #[test]
    fn a_lane_count_the_tarmac_cannot_carry_is_rejected() {
        let seven = ExpandedSection {
            lanes: 7,
            ..section("wide", RoadPrimitiveSpec::Straight { length_m: 200.0 })
        };
        let err = compile(&[seven], &CourseTuning::DEFAULT, &ValidationThresholds::DEFAULT)
            .unwrap_err();
        assert_eq!(err.code, CourseErrorCode::InvalidLaneCount);
        assert_eq!(err.section.as_deref(), Some("wide"));
    }

    #[test]
    fn an_empty_expansion_is_rejected() {
        let err = compile(&[], &CourseTuning::DEFAULT, &ValidationThresholds::DEFAULT)
            .unwrap_err();
        assert_eq!(err.code, CourseErrorCode::EmptyCourse);
    }

    #[test]
    fn a_section_shorter_than_two_samples_still_gets_two() {
        let g = build(&[
            section("tiny", RoadPrimitiveSpec::Straight { length_m: 1.0 }),
            section("rest", RoadPrimitiveSpec::Straight { length_m: 100.0 }),
        ]);
        assert!(g.sections[0].length_m() >= 4.0 - 1.0e-3);
        assert!(g.samples.len() > 50);
    }

    #[test]
    fn a_compiled_section_answers_whether_a_distance_is_inside_it() {
        let g = build(&[
            section("a", RoadPrimitiveSpec::Straight { length_m: 200.0 }),
            section("b", RoadPrimitiveSpec::Straight { length_m: 200.0 }),
        ]);
        assert!(g.sections[0].contains(0.0));
        assert!(g.sections[0].contains(199.0));
        assert!(!g.sections[0].contains(200.0));
        assert!(g.sections[1].contains(200.0));
        assert!(!g.sections[1].contains(400.0));
    }

    #[test]
    fn the_degenerate_signal_helpers_are_harmless() {
        let mut empty: Vec<f32> = Vec::new();
        smooth(&mut empty, 3);
        rate_limit(&mut empty, 0.1);
        assert!(empty.is_empty());
        assert!(relax(&mut empty, 1.0, 0.1).is_empty());
        let mut pair = vec![1.0, 2.0];
        smooth(&mut pair, 3);
        assert_eq!(pair, vec![1.0, 2.0], "the endpoints are never smoothed");
        let mut single = vec![5.0];
        let bitten = relax(&mut single, 1.0, 0.1);
        assert_eq!(single, vec![1.0]);
        assert_eq!(bitten, vec![true]);
    }
}
