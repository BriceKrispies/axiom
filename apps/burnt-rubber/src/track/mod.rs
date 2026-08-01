//! The course: a deterministic, arc-length-indexed table of road samples, and
//! the lookups every other subsystem addresses it through.
//!
//! [`Track`] is the app's single source of spatial truth. The road mesh, the
//! roadside props, the traffic lanes, the collision boundary, the reset points,
//! the camera's anticipation and the HUD's progress bar all read *this* table —
//! there is no second, slightly-different idea of where the road is anywhere in
//! the app. That is deliberate: two representations of a racing line drift, and
//! when they drift the car drives through the scenery.
//!
//! The table is generated once at construction and is then immutable. It is
//! bounded (a ~9 km course at 2 m spacing is a few thousand entries, a few
//! hundred kilobytes), which is why the *logical* course can stay resident while
//! only the *rendered* geometry is chunked and streamed.

pub mod generate;
pub mod section;
pub mod spline;

use axiom_math::Vec3;

use crate::tuning::CourseTuning;

pub use generate::ControlPoint;
pub use section::{SectionKind, SectionProfile, Zone};
pub use spline::unit_or;

/// One sampled point of road, with the complete local frame the geometry and
/// the simulation are both built from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackSample {
    /// The road centre, on the driving surface.
    pub position: Vec3,
    /// Unit direction of travel, including grade.
    pub tangent: Vec3,
    /// Unit lateral, banked. `+right` is the driver's right.
    pub right: Vec3,
    /// Unit road normal, banked.
    pub up: Vec3,
    /// Arc length from the start line (m).
    pub distance: f32,
    /// Heading (radians) of the flattened tangent.
    pub heading: f32,
    /// Signed curvature (rad/m). Positive turns toward `+right`.
    pub curvature: f32,
    /// Grade (rise over run).
    pub grade: f32,
    /// Banking (radians), applied to `right`/`up`.
    pub bank: f32,
    /// Half-width of the driving surface (m).
    pub half_width: f32,
    /// The section this sample belongs to.
    pub section: SectionKind,
}

impl TrackSample {
    /// The world point `lateral` metres to the right of the centre, on the road
    /// surface.
    pub fn at_lateral(&self, lateral: f32) -> Vec3 {
        self.position.add(self.right.mul_scalar(lateral))
    }

    /// The flattened (horizontal) forward direction — what the car's heading and
    /// the traffic's travel direction are measured against.
    pub fn flat_forward(&self) -> Vec3 {
        unit_or(
            Vec3::new(self.tangent.x, 0.0, self.tangent.z),
            Vec3::UNIT_Z,
        )
    }
}

/// The generated course.
#[derive(Debug, Clone)]
pub struct Track {
    samples: Vec<TrackSample>,
    spacing: f32,
    shoulder: f32,
    verge: f32,
    lane_width: f32,
    length: f32,
    seed: u64,
}

impl Track {
    /// Generate the course for `seed` under `tuning`. A pure function of both.
    pub fn generate(seed: u64, tuning: &CourseTuning) -> Track {
        let controls = generate::control_points(seed, tuning);
        let positions: Vec<Vec3> = controls.iter().map(|c| c.position).collect();
        let dense = spline::densify(&positions);
        let resampled = spline::resample(&dense, tuning.sample_spacing);
        let samples = build_samples(&resampled, &controls, tuning);
        let length = samples.last().map(|s| s.distance).unwrap_or(0.0);
        Track {
            samples,
            spacing: tuning.sample_spacing,
            shoulder: tuning.shoulder,
            verge: tuning.verge,
            lane_width: tuning.lane_width,
            length,
            seed,
        }
    }

    /// The seed this course was generated from.
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// Every sample, in ascending distance order.
    pub fn samples(&self) -> &[TrackSample] {
        &self.samples
    }

    /// The total course length (m).
    pub const fn length(&self) -> f32 {
        self.length
    }

    /// The arc-length spacing between samples (m).
    pub const fn spacing(&self) -> f32 {
        self.spacing
    }

    /// The paved shoulder beyond the driving surface (m).
    pub const fn shoulder(&self) -> f32 {
        self.shoulder
    }

    /// The dirt verge beyond the shoulder on open sections (m).
    pub const fn verge(&self) -> f32 {
        self.verge
    }

    /// The nominal lane width the road is divided into (m).
    pub const fn lane_width(&self) -> f32 {
        self.lane_width
    }

    /// How many lanes the road has at `sample`.
    ///
    /// This lives on the track, not on the traffic and not on the road mesh,
    /// because **both** of them need it and they must agree: the painted
    /// dividers and the lanes the traffic holds are the same lanes. Computing
    /// it in two places is how you get cars driving down the middle of a
    /// painted line, and it is exactly the kind of second-idea-of-the-road this
    /// table exists to prevent.
    pub fn lane_count(&self, sample: &TrackSample) -> usize {
        let raw = (sample.half_width * 2.0 / self.lane_width.max(0.1)).floor() as usize;
        raw.clamp(MIN_LANES, MAX_LANES)
    }

    /// The centre of `lane` at `sample` (m from the road centre). An
    /// out-of-range lane clamps to the outermost one.
    pub fn lane_lateral(&self, sample: &TrackSample, lane: usize) -> f32 {
        let lanes = self.lane_count(sample);
        let width = sample.half_width * 2.0 / lanes as f32;
        let index = lane.min(lanes - 1);
        -sample.half_width + width * (index as f32 + 0.5)
    }

    /// How far from the centreline the barrier stands at `sample` (m).
    ///
    /// This is the *one* definition of "the edge of the world" — the collision
    /// resolver, the guardrail mesh, the reflector posts and the scenery
    /// exclusion zone all call it, so a barrier can never be drawn somewhere the
    /// car does not actually stop.
    pub fn barrier_offset(&self, sample: &TrackSample) -> f32 {
        sample.half_width
            + self.shoulder
            + if sample.section.walled() { 0.0 } else { self.verge }
    }

    /// The sample index nearest `distance`, clamped to the course.
    pub fn index_at(&self, distance: f32) -> usize {
        let raw = (distance / self.spacing).round();
        (raw.max(0.0) as usize).min(self.samples.len().saturating_sub(1))
    }

    /// The sample nearest `distance` metres along, clamped to the course.
    pub fn sample_at(&self, distance: f32) -> TrackSample {
        self.samples[self.index_at(distance)]
    }

    /// The linearly interpolated sample at `distance` — used where a stepped
    /// 2 m resolution would be visible, e.g. the car's road height.
    pub fn interpolated_at(&self, distance: f32) -> TrackSample {
        let clamped = distance.clamp(0.0, self.length);
        let raw = clamped / self.spacing;
        let i = (raw.floor().max(0.0) as usize).min(self.samples.len().saturating_sub(1));
        let j = (i + 1).min(self.samples.len().saturating_sub(1));
        let t = (raw - i as f32).clamp(0.0, 1.0);
        let a = self.samples[i];
        let b = self.samples[j];
        TrackSample {
            position: a.position.add(b.position.subtract(a.position).mul_scalar(t)),
            tangent: unit_or(
                a.tangent.add(b.tangent.subtract(a.tangent).mul_scalar(t)),
                a.tangent,
            ),
            right: unit_or(a.right.add(b.right.subtract(a.right).mul_scalar(t)), a.right),
            up: unit_or(a.up.add(b.up.subtract(a.up).mul_scalar(t)), a.up),
            distance: clamped,
            heading: a.heading + shortest_angle(b.heading - a.heading) * t,
            curvature: a.curvature + (b.curvature - a.curvature) * t,
            grade: a.grade + (b.grade - a.grade) * t,
            bank: a.bank + (b.bank - a.bank) * t,
            half_width: a.half_width + (b.half_width - a.half_width) * t,
            section: a.section,
        }
    }

    /// Re-localise a world position onto the course, searching only a bounded
    /// window either side of `hint_distance`.
    ///
    /// The window is what keeps this `O(1)` per step instead of `O(course)`, and
    /// what keeps it *stable*: a car cannot teleport 400 m in one 60 Hz step, so
    /// the nearest sample is always inside the window, and a hairpin that
    /// doubles back near itself cannot snap the car onto the wrong lap of the
    /// road. Returns `(distance_along, lateral_offset)`.
    pub fn localise(&self, position: Vec3, hint_distance: f32, window: f32) -> (f32, f32) {
        let centre = self.index_at(hint_distance) as isize;
        let span = (window / self.spacing).ceil() as isize;
        let last = self.samples.len().saturating_sub(1) as isize;
        let lo = (centre - span).max(0);
        let hi = (centre + span).min(last);
        let mut best = lo as usize;
        let mut best_d2 = f32::INFINITY;
        for i in lo..=hi {
            let d2 = self.samples[i as usize]
                .position
                .subtract(position)
                .length_squared();
            if d2 < best_d2 {
                best_d2 = d2;
                best = i as usize;
            }
        }
        let s = self.samples[best];
        let offset = position.subtract(s.position);
        // Project onto the local frame: along the tangent refines the distance
        // to sub-sample precision, along `right` is the lateral offset.
        let along = offset.dot(s.tangent);
        let lateral = offset.dot(s.right);
        ((s.distance + along).clamp(0.0, self.length), lateral)
    }

    /// The most recent safe respawn point at or before `distance`: a road-centre
    /// pose on the nearest sample, backed off slightly so a reset never drops
    /// the car on top of whatever it just hit, and never further back than
    /// [`GRID_DISTANCE`] so the chase camera always has road beneath it.
    pub fn safe_reset(&self, distance: f32) -> TrackSample {
        self.sample_at((distance - RESET_BACKOFF).max(GRID_DISTANCE))
    }

    /// Progress along the course as a `0..1` fraction.
    pub fn progress(&self, distance: f32) -> f32 {
        (distance / self.length.max(1.0)).clamp(0.0, 1.0)
    }
}

/// The fewest lanes a road is ever divided into.
pub const MIN_LANES: usize = 2;

/// The most lanes a road is ever divided into. A very wide road is a wide road,
/// not a road with twenty lanes: past this the dividers stop reading as lanes
/// and start reading as stripes.
pub const MAX_LANES: usize = 8;

/// How far back up the road a reset places the car (m). Far enough to clear the
/// obstacle, close enough that a mistake costs a moment rather than a section.
pub const RESET_BACKOFF: f32 = 24.0;

/// How far into the course the starting grid sits (m).
///
/// It is not zero, and the reason is framing rather than gameplay. The course
/// ribbon simply *stops* at distance zero — there is no tarmac before the first
/// sample — while the chase camera sits ~6.5 m behind the car, 2.2 m up, and at
/// a 65-degree field of view the bottom of the frame lands roughly 3.8 m
/// **behind** the car. Park the car on the first metre of road and the opening
/// shot is a car floating over a hole: a hard horizontal seam two thirds of the
/// way down the frame, with the rear of the car silhouetted against the
/// background instead of standing on the asphalt.
///
/// Thirty metres puts the whole of the camera's foreground — at every chase
/// distance it can reach, including the pull-back under acceleration — on real
/// road, so the car keeps its ground contact and the road reads as a ribbon
/// running past the viewer rather than a plank starting under the bumper. Real
/// grids sit some way down the pit straight for the same reason. It costs 0.3%
/// of a nine-kilometre course.
pub const GRID_DISTANCE: f32 = 30.0;

/// Wrap an angle difference into `[-π, π]`.
pub fn shortest_angle(delta: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let wrapped = (delta + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    wrapped
}

/// Turn the resampled polyline plus the control attributes into the final table.
fn build_samples(
    resampled: &[spline::DensePoint],
    controls: &[ControlPoint],
    tuning: &CourseTuning,
) -> Vec<TrackSample> {
    if resampled.len() < 3 || controls.is_empty() {
        return Vec::new();
    }
    let n = resampled.len();
    // Tangents from a central difference, so a sample's frame reflects the road
    // through it rather than the road leaving it.
    let tangents: Vec<Vec3> = (0..n)
        .map(|i| {
            let a = resampled[i.saturating_sub(1)].position;
            let b = resampled[(i + 1).min(n - 1)].position;
            unit_or(b.subtract(a), Vec3::UNIT_Z)
        })
        .collect();
    let headings: Vec<f32> = tangents.iter().map(|t| t.x.atan2(t.z)).collect();
    let curvature: Vec<f32> = (0..n)
        .map(|i| {
            let a = headings[i.saturating_sub(1)];
            let b = headings[(i + 1).min(n - 1)];
            let span = ((i + 1).min(n - 1) - i.saturating_sub(1)) as f32 * tuning.sample_spacing;
            shortest_angle(b - a) / span.max(1.0e-3)
        })
        .collect();
    // Banking follows curvature, clamped, then smoothed in a fixed number of
    // passes so the road rolls into a corner instead of stepping into it.
    let mut bank: Vec<f32> = curvature
        .iter()
        .map(|k| (-tuning.bank_per_curvature * k).clamp(-tuning.max_bank, tuning.max_bank))
        .collect();
    smooth(&mut bank, BANK_SMOOTHING_PASSES);

    (0..n)
        .map(|i| {
            let point = resampled[i];
            let control = control_attributes(point.control_t, controls);
            let tangent = tangents[i];
            let flat_right = unit_or(Vec3::UNIT_Y.cross(tangent), Vec3::UNIT_X);
            let flat_up = unit_or(tangent.cross(flat_right), Vec3::UNIT_Y);
            let (sin_b, cos_b) = bank[i].sin_cos();
            TrackSample {
                position: point.position,
                tangent,
                right: flat_right.mul_scalar(cos_b).add(flat_up.mul_scalar(sin_b)),
                up: flat_up.mul_scalar(cos_b).subtract(flat_right.mul_scalar(sin_b)),
                distance: point.distance,
                heading: headings[i],
                curvature: curvature[i],
                grade: tangent.y,
                bank: bank[i],
                half_width: control.0,
                section: control.1,
            }
        })
        .collect()
}

/// Smoothing passes applied to the banking signal.
const BANK_SMOOTHING_PASSES: u32 = 8;

/// A fixed number of three-tap box smoothing passes. Bounded by construction.
fn smooth(signal: &mut [f32], passes: u32) {
    for _ in 0..passes {
        let previous = signal.to_vec();
        for i in 1..previous.len().saturating_sub(1) {
            signal[i] = (previous[i - 1] + previous[i] * 2.0 + previous[i + 1]) * 0.25;
        }
    }
}

/// The half-width and section at a fractional control-point index.
fn control_attributes(control_t: f32, controls: &[ControlPoint]) -> (f32, SectionKind) {
    let last = controls.len() - 1;
    let i = (control_t.floor().max(0.0) as usize).min(last);
    let j = (i + 1).min(last);
    let t = (control_t - i as f32).clamp(0.0, 1.0);
    let half_width = controls[i].half_width + (controls[j].half_width - controls[i].half_width) * t;
    // The section is a discrete label, so it takes the nearer control point
    // rather than a meaningless blend.
    let section = if t < 0.5 {
        controls[i].section
    } else {
        controls[j].section
    };
    (half_width, section)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT)
    }

    #[test]
    fn the_same_seed_samples_an_identical_centreline() {
        let a = track();
        let b = track();
        assert_eq!(a.samples(), b.samples());
        assert_eq!(a.length(), b.length());
    }

    #[test]
    fn the_default_course_is_the_advertised_length() {
        let t = track();
        assert!(
            (8_000.0..=10_500.0).contains(&t.length()),
            "course length {} m",
            t.length()
        );
        assert!(t.samples().len() > 4_000, "sampled at 2 m spacing");
    }

    #[test]
    fn different_seeds_sample_different_centrelines() {
        let a = Track::generate(11, &CourseTuning::DEFAULT);
        let b = Track::generate(12, &CourseTuning::DEFAULT);
        assert_ne!(a.samples(), b.samples());
    }

    #[test]
    fn every_sample_is_finite_and_its_frame_is_orthonormal() {
        for s in track().samples() {
            for v in [s.position, s.tangent, s.right, s.up] {
                assert!(v.x.is_finite() && v.y.is_finite() && v.z.is_finite(), "{v:?}");
            }
            for f in [s.distance, s.heading, s.curvature, s.grade, s.bank, s.half_width] {
                assert!(f.is_finite());
            }
            assert!((s.tangent.length() - 1.0).abs() < 1.0e-3, "unit tangent");
            assert!((s.right.length() - 1.0).abs() < 1.0e-3, "unit right");
            assert!((s.up.length() - 1.0).abs() < 1.0e-3, "unit up");
            assert!(s.tangent.dot(s.right).abs() < 1.0e-3, "tangent ⟂ right");
            assert!(s.right.dot(s.up).abs() < 1.0e-3, "right ⟂ up");
            assert!(s.up.y > 0.5, "the road is never inverted");
        }
    }

    #[test]
    fn every_sampled_constraint_holds() {
        let c = CourseTuning::DEFAULT;
        for seed in [0u64, 5, 77, 9001, u64::MAX] {
            let t = Track::generate(seed, &c);
            for s in t.samples() {
                assert!(
                    s.half_width >= c.min_half_width - 1.0e-3,
                    "seed {seed}: width {}",
                    s.half_width
                );
                assert!(s.half_width <= c.max_half_width + 1.0e-3);
                assert!(s.bank.abs() <= c.max_bank + 1.0e-3, "seed {seed}: bank {}", s.bank);
                assert!(
                    s.grade.abs() <= c.max_grade * 1.35 + 1.0e-2,
                    "seed {seed}: grade {} (spline overshoot allowance)",
                    s.grade
                );
                // Minimum turn radius: 1/|curvature|.
                let radius = 1.0 / s.curvature.abs().max(1.0e-6);
                assert!(radius > 90.0, "seed {seed}: turn radius {radius} m is a hairpin");
            }
        }
    }

    #[test]
    fn distance_is_monotone_and_evenly_spaced() {
        let t = track();
        for (i, s) in t.samples().iter().enumerate() {
            assert!((s.distance - i as f32 * t.spacing()).abs() < 1.0e-2);
        }
    }

    #[test]
    fn localisation_recovers_a_point_placed_on_the_road() {
        let t = track();
        for d in [0.0f32, 137.0, 900.0, 3_050.0, 6_400.0] {
            for lateral in [-4.0f32, 0.0, 3.5] {
                let s = t.interpolated_at(d);
                let world = s.at_lateral(lateral);
                let (found_d, found_lat) = t.localise(world, d, 60.0);
                assert!((found_d - d).abs() < 1.5, "distance {d} -> {found_d}");
                assert!((found_lat - lateral).abs() < 0.35, "lateral {lateral} -> {found_lat}");
            }
        }
    }

    #[test]
    fn localisation_clamps_at_both_ends_of_the_course() {
        let t = track();
        let before = t.samples()[0].position.add(Vec3::new(0.0, 0.0, -500.0));
        let (d, _) = t.localise(before, 0.0, 40.0);
        assert!(d >= 0.0);
        let last = *t.samples().last().unwrap();
        let after = last.position.add(last.tangent.mul_scalar(500.0));
        let (d, _) = t.localise(after, t.length(), 40.0);
        assert!(d <= t.length() + 1.0e-3);
    }

    #[test]
    fn sample_lookups_clamp_rather_than_panic() {
        let t = track();
        assert_eq!(t.index_at(-1_000.0), 0);
        assert_eq!(t.index_at(1.0e9), t.samples().len() - 1);
        assert_eq!(t.sample_at(-5.0).distance, 0.0);
        assert!((t.interpolated_at(-5.0).distance - 0.0).abs() < 1.0e-6);
        assert!((t.interpolated_at(1.0e9).distance - t.length()).abs() < 1.0e-3);
        assert_eq!(t.progress(-1.0), 0.0);
        assert_eq!(t.progress(1.0e9), 1.0);
    }

    #[test]
    fn interpolation_agrees_with_the_table_at_the_sample_points() {
        let t = track();
        for i in [0usize, 1, 500, 2_000] {
            let s = t.samples()[i];
            let interpolated = t.interpolated_at(s.distance);
            assert!(interpolated.position.distance(s.position) < 1.0e-2);
            assert!((interpolated.half_width - s.half_width).abs() < 1.0e-2);
        }
    }

    #[test]
    fn a_reset_point_is_behind_the_car_and_on_the_road() {
        let t = track();
        let reset = t.safe_reset(1_000.0);
        assert!(reset.distance <= 1_000.0 - RESET_BACKOFF + t.spacing());
        assert!(reset.distance >= GRID_DISTANCE);
        // Resetting at the start line stays on the grid: never behind it, where
        // the course has no tarmac for the chase camera to look at.
        assert_eq!(t.safe_reset(0.0).distance, t.sample_at(GRID_DISTANCE).distance);
        assert!(t.safe_reset(0.0).distance >= GRID_DISTANCE - t.spacing());
    }

    /// The banking must roll *into* the turn: on a right-hand bend the left edge
    /// of the road is the higher one.
    #[test]
    fn banking_leans_into_the_corner() {
        let t = track();
        let bend = t
            .samples()
            .iter()
            .max_by(|a, b| a.curvature.abs().total_cmp(&b.curvature.abs()))
            .expect("the course has a corner");
        assert!(bend.curvature.abs() > 1.0e-3, "and it is a real corner");
        let outside = bend.at_lateral(-bend.half_width * bend.curvature.signum());
        let inside = bend.at_lateral(bend.half_width * bend.curvature.signum());
        assert!(
            outside.y > inside.y,
            "the outside edge is raised: outside {} vs inside {}",
            outside.y,
            inside.y
        );
    }

    #[test]
    fn shortest_angle_wraps_both_ways() {
        let pi = std::f32::consts::PI;
        assert!((shortest_angle(0.0)).abs() < 1.0e-6);
        // A half turn is equally +pi and -pi; only the magnitude is defined.
        assert!((shortest_angle(3.0 * pi).abs() - pi).abs() < 1.0e-4);
        assert!((shortest_angle(-3.0 * pi).abs() - pi).abs() < 1.0e-4);
        assert!((shortest_angle(0.25) - 0.25).abs() < 1.0e-6);
        assert!(shortest_angle(2.0 * pi - 0.25) < 0.0);
    }

    #[test]
    fn smoothing_a_degenerate_signal_is_harmless() {
        let mut empty: Vec<f32> = Vec::new();
        smooth(&mut empty, 3);
        assert!(empty.is_empty());
        let mut pair = vec![1.0, 2.0];
        smooth(&mut pair, 3);
        assert_eq!(pair, vec![1.0, 2.0], "the endpoints are never smoothed");
    }

    #[test]
    fn building_from_a_degenerate_polyline_yields_no_samples() {
        let t = CourseTuning::DEFAULT;
        assert!(build_samples(&[], &[], &t).is_empty());
        let controls = generate::control_points(1, &t);
        assert!(build_samples(&[], &controls, &t).is_empty());
    }

    #[test]
    fn the_sections_are_laid_out_along_the_course_in_order() {
        let t = track();
        let mut seen: Vec<SectionKind> = Vec::new();
        for s in t.samples() {
            if seen.last() != Some(&s.section) {
                seen.push(s.section);
            }
        }
        assert_eq!(seen, SectionKind::ALL.to_vec());
        assert_eq!(t.samples()[0].section, SectionKind::StartStraight);
        assert_eq!(t.samples().last().unwrap().section, SectionKind::Finish);
    }

    /// The lanes the traffic holds and the lanes the road is painted with are
    /// the same lanes, because there is only one definition of them.
    #[test]
    fn lanes_divide_the_road_evenly_and_stay_on_it() {
        let t = track();
        for distance in [0.0f32, 900.0, 4_400.0, 8_000.0] {
            let sample = t.sample_at(distance);
            let lanes = t.lane_count(&sample);
            assert!((MIN_LANES..=MAX_LANES).contains(&lanes));

            let mut previous = f32::NEG_INFINITY;
            for lane in 0..lanes {
                let lateral = t.lane_lateral(&sample, lane);
                assert!(lateral > previous, "lanes run left to right");
                previous = lateral;
                assert!(
                    lateral.abs() < sample.half_width,
                    "lane {lane} at {lateral} is off a road {} m wide",
                    sample.half_width
                );
            }
            // Evenly spaced, and symmetric about the centreline.
            let first = t.lane_lateral(&sample, 0);
            let last = t.lane_lateral(&sample, lanes - 1);
            assert!((first + last).abs() < 1.0e-3, "the lanes are centred");
            // Out of range clamps rather than panicking.
            assert_eq!(t.lane_lateral(&sample, 999), last);
        }
    }

    #[test]
    fn a_wider_road_gets_more_lanes_until_the_ceiling() {
        let t = track();
        let narrow = t
            .samples()
            .iter()
            .min_by(|a, b| a.half_width.total_cmp(&b.half_width))
            .copied()
            .expect("the course has road");
        let wide = t
            .samples()
            .iter()
            .max_by(|a, b| a.half_width.total_cmp(&b.half_width))
            .copied()
            .expect("the course has road");
        assert!(t.lane_count(&wide) >= t.lane_count(&narrow));
        assert!(t.lane_width() > 0.0);
    }

    #[test]
    fn lateral_and_forward_helpers_agree_with_the_frame() {
        let t = track();
        let s = t.sample_at(600.0);
        let right_of_centre = s.at_lateral(5.0);
        assert!((right_of_centre.subtract(s.position).dot(s.right) - 5.0).abs() < 1.0e-3);
        let flat = s.flat_forward();
        assert!((flat.length() - 1.0).abs() < 1.0e-4);
        assert!(flat.y.abs() < 1.0e-6, "the flattened forward is horizontal");
    }
}
