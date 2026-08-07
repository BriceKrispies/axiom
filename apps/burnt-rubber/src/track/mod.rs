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
//! The table is **built by the course compiler** ([`crate::course`]) and is then
//! immutable. It used to be generated here, from a bespoke control-point walk;
//! that generator is gone, and what is left is the table and the questions
//! everything asks of it. The split matters: this file answers *where is the
//! road*, and the course system answers *what road should there be*.
//!
//! It is bounded (a ~9 km course at 2 m spacing is a few thousand entries, a few
//! hundred kilobytes), which is why the *logical* course can stay resident while
//! only the *rendered* geometry is chunked and streamed.

use axiom_math::Vec3;

use crate::tuning::CourseTuning;

pub use crate::course::specification::{SectionKind, Zone};

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
    /// The environment/scenery profile of the section this sample belongs to.
    pub section: SectionKind,
    /// The **identity** of the compiled section this sample belongs to — an
    /// index into [`CoursePlan::sections`](crate::course::runtime::CoursePlan::sections).
    ///
    /// Distinct from [`Self::section`] on purpose: several sections may share an
    /// environment (two stretches of coast look the same), but each is its own
    /// authored piece of road with its own id, seeds and traffic.
    pub section_index: u16,
    /// The speed a competent player is expected to be carrying here (m/s), from
    /// the section's authored value.
    pub expected_speed: f32,
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

/// The compiled course's road.
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
    /// Wrap a compiled sample table.
    ///
    /// The only constructor: a `Track` is a *result*, and the one thing allowed
    /// to produce the samples is [`crate::course::geometry`].
    pub fn from_samples(seed: u64, samples: Vec<TrackSample>, tuning: &CourseTuning) -> Track {
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

    /// How far out from the centreline lanes reach at `sample`: the road carries
    /// lanes `-reach ..= reach`.
    ///
    /// A lane pair is added only once the road is wide enough to hold it whole,
    /// so the count is always **odd** and never below three — and growing it
    /// appends lanes at the shoulders without disturbing any lane that already
    /// existed.
    pub fn lane_reach(&self, sample: &TrackSample) -> i32 {
        let fits = (sample.half_width / self.lane_width.max(0.1) - 0.5).floor() as i32;
        fits.clamp(1, MAX_LANE_REACH)
    }

    /// How many lanes the road has at `sample`. Always odd — one lane sits on
    /// the centreline and the rest are paired off either side of it.
    ///
    /// This lives on the track, not on the traffic and not on the road mesh,
    /// because **both** of them need it and they must agree: the painted
    /// dividers and the lanes the traffic holds are the same lanes. Computing
    /// it in two places is how you get cars driving down the middle of a
    /// painted line, and it is exactly the kind of second-idea-of-the-road this
    /// table exists to prevent.
    pub fn lane_count(&self, sample: &TrackSample) -> usize {
        (self.lane_reach(sample) * 2 + 1) as usize
    }

    /// The centre of `lane` at `sample` (m from the road centre). A lane beyond
    /// the road's reach clamps to the outermost one on that side.
    ///
    /// **Lanes are numbered out from the centreline and are a fixed width.**
    /// Lane 0 *is* the centreline, `-1`/`+1` flank it, `±2` sit outboard of
    /// those — so lane 0 is at 0.0 m and lane `n` is at `n * lane_width` for the
    /// entire nine kilometres, whatever the road is doing.
    ///
    /// Both halves of that matter. Anchoring the numbering at the centre instead
    /// of the edge is what makes a lane a durable identity rather than an
    /// ordinal into a list that keeps changing length — a car holding a lane
    /// index is not shunted sideways by a road that merely got wider.
    pub fn lane_lateral(&self, sample: &TrackSample, lane: i32) -> f32 {
        let reach = self.lane_reach(sample);
        lane.clamp(-reach, reach) as f32 * self.lane_width
    }

    /// Which lane a car at `lateral` (m from the road centre) is in at `sample` —
    /// the exact inverse of [`Track::lane_lateral`], and here rather than at the
    /// call site for the same reason that one is: **there is one idea of where
    /// the lanes are.**
    pub fn lane_at_lateral(&self, sample: &TrackSample, lateral: f32) -> i32 {
        let reach = self.lane_reach(sample);
        let nearest = (lateral / self.lane_width.max(0.1)).round() as i32;
        nearest.clamp(-reach, reach)
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
            // Discrete labels take the nearer sample rather than a meaningless
            // blend.
            section: a.section,
            section_index: a.section_index,
            expected_speed: a.expected_speed,
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

    /// The shipping course's road for `seed` — the fixture every test that
    /// wants *a road* asks for.
    ///
    /// Test-only, and deliberately routed through the real compiler rather than
    /// through a hand-built sample table: a fixture that is not the road the
    /// game builds proves nothing about the road the game builds.
    #[cfg(test)]
    pub fn fixture(seed: u64) -> Track {
        crate::course::procedural::shipping_plan(seed)
            .expect("the shipping course compiles")
            .track()
            .clone()
    }
}

/// The fewest lanes a road is ever divided into: the centre lane and one either
/// side. These three exist at the same three lateral offsets for the whole
/// course — that is the guarantee the whole lane lattice is built to give.
pub const MIN_LANES: usize = 3;

/// How far out from the centreline lanes may ever reach. A very wide road is a
/// wide road, not a road with twenty lanes: past this the dividers stop reading
/// as lanes and start reading as stripes. The shipping course tops out at 2
/// (five lanes); this is the ceiling, not the target.
pub const MAX_LANE_REACH: i32 = 3;

/// The most lanes a road is ever divided into.
pub const MAX_LANES: usize = (MAX_LANE_REACH * 2 + 1) as usize;

/// How far back up the road a reset places the car (m). Far enough to clear the
/// obstacle, close enough that a mistake costs a moment rather than a section.
pub const RESET_BACKOFF: f32 = 24.0;

/// How far into the course the starting grid sits (m).
///
/// It is not zero, and the reason is framing rather than gameplay. The course
/// ribbon simply *stops* at distance zero — there is no tarmac before the first
/// sample — while the chase camera sits ~5.5 m behind the car, so a grid on the
/// first metre of road frames the car against a hole. Thirty metres puts the
/// whole of the camera's foreground on real road. It costs 0.3% of a
/// nine-kilometre course.
pub const GRID_DISTANCE: f32 = 30.0;

/// Wrap an angle difference into `[-π, π]`.
pub fn shortest_angle(delta: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (delta + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

/// A unit vector, or `fallback` if the input has no direction.
///
/// Every normalisation in the course geometry goes through this, because a zero
/// vector normalised is a `NaN` and one `NaN` in a road frame poisons every
/// position downstream of it.
pub fn unit_or(v: Vec3, fallback: Vec3) -> Vec3 {
    let length = v.length();
    (length > 1.0e-6)
        .then(|| v.mul_scalar(1.0 / length))
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::procedural;

    fn track() -> Track {
        procedural::shipping_plan(crate::DEFAULT_SEED)
            .expect("the shipping course compiles")
            .track()
            .clone()
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
        let a = procedural::shipping_plan(11).unwrap().track().clone();
        let b = procedural::shipping_plan(12).unwrap().track().clone();
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
            assert!(s.expected_speed > 0.0, "every sample knows its target speed");
        }
    }

    #[test]
    fn every_sampled_constraint_holds() {
        let c = crate::tuning::CourseTuning::DEFAULT;
        let thresholds = crate::course::specification::ValidationThresholds::DEFAULT;
        for seed in [0u64, 5, 77, 9001, u64::MAX] {
            let t = procedural::shipping_plan(seed).unwrap().track().clone();
            for s in t.samples() {
                assert!(
                    s.half_width >= c.min_half_width - 1.0e-3,
                    "seed {seed}: width {}",
                    s.half_width
                );
                assert!(s.half_width <= c.max_half_width + 1.0e-3);
                assert!(s.bank.abs() <= c.max_bank + 1.0e-3, "seed {seed}: bank {}", s.bank);
                assert!(
                    s.grade.abs() <= c.max_grade + 1.0e-3,
                    "seed {seed}: grade {}",
                    s.grade
                );
                let radius = 1.0 / s.curvature.abs().max(1.0e-6);
                assert!(
                    radius >= thresholds.min_turn_radius_m - 1.0,
                    "seed {seed}: turn radius {radius} m is a hairpin"
                );
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
            assert_eq!(interpolated.section_index, s.section_index);
        }
    }

    #[test]
    fn a_reset_point_is_behind_the_car_and_on_the_road() {
        let t = track();
        let reset = t.safe_reset(1_000.0);
        assert!(reset.distance <= 1_000.0 - RESET_BACKOFF + t.spacing());
        assert!(reset.distance >= GRID_DISTANCE);
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
        assert!((shortest_angle(3.0 * pi).abs() - pi).abs() < 1.0e-4);
        assert!((shortest_angle(-3.0 * pi).abs() - pi).abs() < 1.0e-4);
        assert!((shortest_angle(0.25) - 0.25).abs() < 1.0e-6);
        assert!(shortest_angle(2.0 * pi - 0.25) < 0.0);
    }

    #[test]
    fn unit_or_falls_back_instead_of_producing_a_nan() {
        assert_eq!(unit_or(Vec3::ZERO, Vec3::UNIT_Z), Vec3::UNIT_Z);
        let u = unit_or(Vec3::new(0.0, 0.0, 4.0), Vec3::UNIT_X);
        assert!((u.length() - 1.0).abs() < 1.0e-6);
        assert_eq!(u, Vec3::UNIT_Z);
    }

    #[test]
    fn the_environments_are_laid_out_along_the_course_in_order() {
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

    /// Section *identity* is finer than section *environment*: the compiled
    /// course has many more sections than it has environments, and the index
    /// only ever moves forward.
    #[test]
    fn every_sample_names_the_compiled_section_it_belongs_to() {
        let plan = procedural::shipping_plan(crate::DEFAULT_SEED).unwrap();
        let t = plan.track();
        assert!(plan.sections().len() > SectionKind::ALL.len());
        for pair in t.samples().windows(2) {
            assert!(
                pair[1].section_index >= pair[0].section_index,
                "the section index went backwards at {} m",
                pair[0].distance
            );
        }
        for s in t.samples() {
            let section = &plan.sections()[s.section_index as usize];
            assert_eq!(section.environment, s.section);
            assert!(
                (s.distance >= section.start_m - 1.0e-2) & (s.distance <= section.end_m + 1.0e-2),
                "sample at {} m claims section {} which spans {}..{}",
                s.distance,
                section.id,
                section.start_m,
                section.end_m
            );
        }
    }

    /// The lanes the traffic holds and the lanes the road is painted with are
    /// the same lanes, because there is only one definition of them.
    #[test]
    fn lanes_divide_the_road_evenly_and_stay_on_it() {
        let t = track();
        for distance in [0.0f32, 900.0, 4_400.0, 8_000.0] {
            let sample = t.sample_at(distance);
            let lanes = t.lane_count(&sample);
            let reach = t.lane_reach(&sample);
            assert!((MIN_LANES..=MAX_LANES).contains(&lanes));
            assert_eq!(lanes % 2, 1, "a lane always sits on the centreline");
            assert_eq!(lanes, (reach * 2 + 1) as usize);

            let mut previous = f32::NEG_INFINITY;
            for lane in -reach..=reach {
                let lateral = t.lane_lateral(&sample, lane);
                assert!(lateral > previous, "lanes run left to right");
                previous = lateral;
                assert!(
                    lateral.abs() + t.lane_width() * 0.5 <= sample.half_width + 1.0e-4,
                    "lane {lane} at {lateral} hangs off a road {} m wide",
                    sample.half_width
                );
            }
            assert!(
                (t.lane_lateral(&sample, -reach) + t.lane_lateral(&sample, reach)).abs() < 1.0e-3,
                "the lanes are centred"
            );
            assert_eq!(t.lane_lateral(&sample, 999), t.lane_lateral(&sample, reach));
            assert_eq!(t.lane_lateral(&sample, -999), t.lane_lateral(&sample, -reach));
        }
    }

    /// **The invariant the whole lattice exists for.** The three centre lanes
    /// are the same three pieces of road for the entire course.
    #[test]
    fn the_three_centre_lanes_never_move_along_the_whole_course() {
        let t = track();
        let width = t.lane_width();
        for sample in t.samples() {
            assert_eq!(t.lane_lateral(sample, 0), 0.0, "lane 0 IS the centreline");
            assert_eq!(t.lane_lateral(sample, 1), width);
            assert_eq!(t.lane_lateral(sample, -1), -width);
            assert!(t.lane_count(sample) >= MIN_LANES, "and those three always exist");
        }
    }

    #[test]
    fn a_lane_survives_the_round_trip_through_its_own_lateral() {
        let t = track();
        for sample in t.samples() {
            let reach = t.lane_reach(sample);
            for lane in -reach..=reach {
                let lateral = t.lane_lateral(sample, lane);
                assert_eq!(t.lane_at_lateral(sample, lateral), lane);
            }
            let width = t.lane_width();
            assert_eq!(t.lane_at_lateral(sample, width * 0.49), 0);
            assert_eq!(t.lane_at_lateral(sample, width * 0.51), 1);
            assert_eq!(t.lane_at_lateral(sample, width * 99.0), reach);
            assert_eq!(t.lane_at_lateral(sample, width * -99.0), -reach);
        }
    }

    /// A road may only change lane count where the author asked it to, and the
    /// compiled ramp is smooth, so the count changes a bounded number of times
    /// over the whole course rather than flickering.
    #[test]
    fn the_lane_count_changes_only_where_the_course_authored_a_change() {
        for seed in [1u64, 7, 99, 4_242] {
            let t = procedural::shipping_plan(seed).unwrap().track().clone();
            let changes = t
                .samples()
                .windows(2)
                .filter(|w| t.lane_count(&w[0]) != t.lane_count(&w[1]))
                .count();
            assert!(
                changes <= SectionKind::ALL.len(),
                "seed {seed}: {changes} lane-count changes for {} authored widths",
                SectionKind::ALL.len()
            );
            assert!(changes > 0, "seed {seed}: the course should still open out");
        }
    }

    /// Extra lanes are appended at the shoulders, so a lane that exists on the
    /// narrow road is at the identical offset on the wide one.
    #[test]
    fn a_wider_road_appends_lanes_without_moving_the_existing_ones() {
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
        let (thin, thick) = (t.lane_reach(&narrow), t.lane_reach(&wide));
        assert!(thick > thin, "the course does actually change lane count");
        for lane in -thin..=thin {
            assert_eq!(
                t.lane_lateral(&narrow, lane),
                t.lane_lateral(&wide, lane),
                "lane {lane} moved when the road widened"
            );
        }
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

    #[test]
    fn an_empty_sample_table_is_a_zero_length_course_rather_than_a_panic() {
        let t = Track::from_samples(1, Vec::new(), &crate::tuning::CourseTuning::DEFAULT);
        assert_eq!(t.length(), 0.0);
        assert_eq!(t.samples().len(), 0);
        assert_eq!(t.index_at(500.0), 0);
        assert_eq!(t.progress(500.0), 1.0);
    }
}
