//! **The compiled course plan** — the immutable value the game actually runs
//! on, and the indexes that make reading it cheap.
//!
//! Everything authored is gone by this point. There is no spec, no motif, no
//! parser node and no expansion here: a [`CoursePlan`] is a sample table, a list
//! of sections, a list of vehicles sorted by spawn distance, a list of
//! encounters, a list of opportunity windows, and a validation report. It is
//! built once, shared by `Arc`, and never mutated.
//!
//! # Why the indexes exist
//!
//! The runtime asks two questions on **every** fixed step: *which section am I
//! in* and *which traffic plans have entered the spawn horizon*. Answered by
//! scanning, those are `O(sections)` and `O(vehicles)` per step — a hundred-odd
//! comparisons sixty times a second, forever, to learn something that changes
//! once every few seconds. Both are therefore answered by a bucket index built
//! at compile time: a flat `Vec<u32>` mapping a 100 m bucket of course distance
//! to the first entry at or past it, so a lookup is an array read and a short
//! walk.

pub mod activation;
pub mod inspect;

use crate::course::geometry::CompiledSection;
use crate::course::pickups::BoostPickup;
use crate::course::specification::{EncounterId, PickupId, VehicleId};
use crate::course::traffic::{CompiledEncounter, NearMissWindow, TrafficPlan};
use crate::course::validation::ValidationReport;
use crate::track::Track;

pub use activation::DistanceIndex;

/// One compiled, immutable course.
#[derive(Debug, Clone)]
pub struct CoursePlan {
    name: String,
    seed: u64,
    track: Track,
    sections: Vec<CompiledSection>,
    section_index: DistanceIndex,
    traffic: Vec<TrafficPlan>,
    traffic_index: DistanceIndex,
    encounters: Vec<CompiledEncounter>,
    windows: Vec<NearMissWindow>,
    window_index: DistanceIndex,
    pickups: Vec<BoostPickup>,
    pickup_index: DistanceIndex,
    report: ValidationReport,
}

impl CoursePlan {
    /// Assemble a plan from its compiled parts, building the runtime indexes.
    ///
    /// `traffic`, `windows` and `pickups` must already be in ascending distance
    /// order — the compiler sorts them, and the indexes assume it.
    #[allow(clippy::too_many_arguments)]
    pub fn assemble(
        name: String,
        seed: u64,
        track: Track,
        sections: Vec<CompiledSection>,
        traffic: Vec<TrafficPlan>,
        encounters: Vec<CompiledEncounter>,
        windows: Vec<NearMissWindow>,
        pickups: Vec<BoostPickup>,
        report: ValidationReport,
    ) -> CoursePlan {
        let length = track.length();
        let section_index =
            DistanceIndex::build(length, sections.iter().map(|s| s.start_m));
        let traffic_index = DistanceIndex::build(length, traffic.iter().map(|p| p.spawn_m));
        let window_index = DistanceIndex::build(length, windows.iter().map(|w| w.start_m));
        let pickup_index = DistanceIndex::build(length, pickups.iter().map(|p| p.at_m));
        CoursePlan {
            name,
            seed,
            track,
            sections,
            section_index,
            traffic,
            traffic_index,
            encounters,
            windows,
            window_index,
            pickups,
            pickup_index,
            report,
        }
    }

    /// The course's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The seed everything on this course was derived from.
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// The road.
    pub const fn track(&self) -> &Track {
        &self.track
    }

    /// Total course length (m).
    pub fn length(&self) -> f32 {
        self.track.length()
    }

    /// The compiled sections, in course order.
    pub fn sections(&self) -> &[CompiledSection] {
        &self.sections
    }

    /// The section covering `distance_m`, or the nearest one at either end.
    ///
    /// The index gives a hint; the two bounded walks either side of it are what
    /// make the answer exact however many short sections share one bucket.
    pub fn section_at(&self, distance_m: f32) -> &CompiledSection {
        let last = self.sections.len() - 1;
        let mut i = self.section_index.first_at(distance_m).min(last);
        // `&&`, not `&`: the bound check has to short-circuit, or the index
        // is evaluated one past the end of the list.
        while i > 0 && self.sections[i].start_m > distance_m {
            i -= 1;
        }
        while i < last && self.sections[i + 1].start_m <= distance_m {
            i += 1;
        }
        &self.sections[i]
    }

    /// Every compiled vehicle, in ascending spawn order.
    pub fn traffic(&self) -> &[TrafficPlan] {
        &self.traffic
    }

    /// The index of the first vehicle whose spawn distance is at or past
    /// `distance_m`. `O(1)` plus a bounded walk inside one bucket.
    pub fn first_vehicle_at(&self, distance_m: f32) -> usize {
        let from = self.traffic_index.first_at(distance_m);
        from + self.traffic[from..]
            .iter()
            .position(|p| p.spawn_m >= distance_m)
            .unwrap_or(self.traffic.len() - from)
    }

    /// A vehicle by its stable identity.
    pub fn vehicle(&self, id: VehicleId) -> Option<&TrafficPlan> {
        // Identities are minted densely in spawn order, so the id is very
        // nearly the index; confirm rather than search.
        self.traffic
            .get(id.0 as usize)
            .filter(|p| p.id == id)
            .or_else(|| self.traffic.iter().find(|p| p.id == id))
    }

    /// The compiled encounters, in course order.
    pub fn encounters(&self) -> &[CompiledEncounter] {
        &self.encounters
    }

    /// The encounter covering `distance_m`, if any.
    pub fn encounter_at(&self, distance_m: f32) -> Option<&CompiledEncounter> {
        self.encounters
            .iter()
            .find(|e| (distance_m >= e.start_m) & (distance_m < e.end_m))
    }

    /// An encounter by its stable identity.
    pub fn encounter(&self, id: EncounterId) -> Option<&CompiledEncounter> {
        self.encounters.iter().find(|e| e.id == id)
    }

    /// Every compiled near-miss opportunity window, in ascending order.
    pub fn near_miss_windows(&self) -> &[NearMissWindow] {
        &self.windows
    }

    /// The opportunity windows opening in `[from_m, from_m + span_m]`.
    pub fn windows_ahead(&self, from_m: f32, span_m: f32) -> impl Iterator<Item = &NearMissWindow> {
        let start = self.window_index.first_at(from_m - MAX_WINDOW_LENGTH_M);
        let limit = from_m + span_m;
        self.windows[start..]
            .iter()
            .take_while(move |w| w.start_m <= limit)
            .filter(move |w| w.end_m >= from_m)
    }

    /// Every compiled boost pickup, in ascending course order.
    pub fn pickups(&self) -> &[BoostPickup] {
        &self.pickups
    }

    /// The index of the first pickup at or past `distance_m`. `O(1)` plus a
    /// bounded walk inside one bucket — the same shape as
    /// [`Self::first_vehicle_at`], and the entry point the runtime's collector
    /// uses so a swept collect test never scans the whole course.
    pub fn first_pickup_at(&self, distance_m: f32) -> usize {
        let from = self.pickup_index.first_at(distance_m);
        from + self.pickups[from..]
            .iter()
            .position(|p| p.at_m >= distance_m)
            .unwrap_or(self.pickups.len() - from)
    }

    /// A pickup by its stable identity.
    pub fn pickup(&self, id: PickupId) -> Option<&BoostPickup> {
        // Identities are minted densely, so the id is very nearly the index;
        // confirm rather than search.
        self.pickups
            .get(id.0 as usize)
            .filter(|p| p.id == id)
            .or_else(|| self.pickups.iter().find(|p| p.id == id))
    }

    /// The pickups standing in `[from_m, from_m + span_m]`.
    pub fn pickups_ahead(&self, from_m: f32, span_m: f32) -> impl Iterator<Item = &BoostPickup> {
        let limit = from_m + span_m;
        self.pickups[self.first_pickup_at(from_m)..]
            .iter()
            .take_while(move |p| p.at_m <= limit)
    }

    /// The validation report this course compiled with.
    pub const fn report(&self) -> &ValidationReport {
        &self.report
    }

    /// The deterministic textual dump — see [`inspect`].
    pub fn dump(&self) -> String {
        inspect::dump(self)
    }
}

/// How far back the window index is rewound when answering "what is ahead".
///
/// A window that *starts* before the query point may still be open at it, so
/// the scan has to begin far enough back to see them. This is the longest a
/// compiled window can be, which bounds that rewind.
pub const MAX_WINDOW_LENGTH_M: f32 = 1_200.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::procedural;

    fn plan() -> CoursePlan {
        procedural::shipping_plan(crate::DEFAULT_SEED).expect("the shipping course compiles")
    }

    #[test]
    fn the_plan_reports_its_identity_and_its_road() {
        let plan = plan();
        assert!(!plan.name().is_empty());
        assert_eq!(plan.seed(), crate::DEFAULT_SEED);
        assert!(plan.length() > 8_000.0);
        assert_eq!(plan.length(), plan.track().length());
        assert!(plan.sections().len() >= 9);
    }

    #[test]
    fn a_section_lookup_finds_the_section_that_contains_the_distance() {
        let plan = plan();
        for section in plan.sections() {
            let middle = (section.start_m + section.end_m) * 0.5;
            assert_eq!(
                plan.section_at(middle).index,
                section.index,
                "{} m is inside {}",
                middle,
                section.id
            );
            assert_eq!(plan.section_at(section.start_m).index, section.index);
        }
        // Out of range clamps rather than panicking.
        assert_eq!(plan.section_at(-100.0).index, 0);
        assert_eq!(
            plan.section_at(plan.length() * 2.0).index,
            plan.sections().last().unwrap().index
        );
    }

    #[test]
    fn a_traffic_lookup_finds_the_first_plan_at_or_past_a_distance() {
        let plan = plan();
        assert!(!plan.traffic().is_empty());
        for probe in [0.0f32, 500.0, 2_500.0, 6_000.0, plan.length()] {
            let index = plan.first_vehicle_at(probe);
            // Everything before it spawns earlier...
            assert!(plan.traffic()[..index].iter().all(|p| p.spawn_m < probe));
            // ...and the one it points at does not.
            plan.traffic()
                .get(index)
                .map(|p| assert!(p.spawn_m >= probe));
        }
        assert_eq!(plan.first_vehicle_at(-1.0), 0);
        assert_eq!(plan.first_vehicle_at(1.0e9), plan.traffic().len());
    }

    #[test]
    fn a_vehicle_can_be_found_by_its_stable_identity() {
        let plan = plan();
        for expected in plan.traffic().iter().take(20) {
            let found = plan.vehicle(expected.id).expect("the vehicle exists");
            assert_eq!(found, expected);
        }
        assert!(plan.vehicle(VehicleId(u32::MAX)).is_none());
    }

    #[test]
    fn the_windows_ahead_of_a_point_are_the_ones_open_there_or_soon() {
        let plan = plan();
        assert!(!plan.near_miss_windows().is_empty());
        let ahead: Vec<&NearMissWindow> = plan.windows_ahead(2_000.0, 600.0).collect();
        assert!(!ahead.is_empty(), "no opportunities in a 600 m stretch");
        for window in &ahead {
            assert!(window.start_m <= 2_600.0);
            assert!(window.end_m >= 2_000.0);
        }
        // The scan is the same set the brute-force filter finds.
        let brute: Vec<&NearMissWindow> = plan
            .near_miss_windows()
            .iter()
            .filter(|w| (w.start_m <= 2_600.0) & (w.end_m >= 2_000.0))
            .collect();
        assert_eq!(ahead, brute);
        assert_eq!(plan.windows_ahead(1.0e9, 100.0).count(), 0);
    }

    #[test]
    fn every_compiled_window_and_encounter_is_addressable() {
        let plan = plan();
        for encounter in plan.encounters() {
            assert_eq!(plan.encounter(encounter.id), Some(encounter));
            let middle = (encounter.start_m + encounter.end_m) * 0.5;
            assert_eq!(plan.encounter_at(middle).map(|e| e.id), Some(encounter.id));
        }
        assert!(plan.encounter(EncounterId(9_999)).is_none());
    }
}
