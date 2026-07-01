//! The deterministic per-frame report, and the app-owned projection value types
//! the harness reads physics state into.
//!
//! Because `axiom-physics` exposes a single facade, its snapshot / record /
//! contact types cannot be named outside the module. The crucible therefore
//! projects them, at the `CrucibleWorld` boundary, into these plain app-owned
//! value types — which it *can* name, compare, hash into a digest, and print.
//! [`CrucibleReport`] is the structured diagnostic the spec asks for: every field
//! it can fill from `PhysicsApi`, it fills; every field physics does not surface,
//! it marks `unavailable` honestly rather than fabricating.

use axiom::prelude::Vec3;
use axiom_physics::PhysicsBodyHandle;

/// One body's state, projected from the physics snapshot.
///
/// `rotation` is the orientation quaternion projected as `[x, y, z, w]` — kept as
/// a plain array so the projection never has to name the math `Quat` type, while
/// still letting the two-world replay compare angular state exactly. `angular` is
/// the angular velocity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyState {
    pub handle: PhysicsBodyHandle,
    pub translation: Vec3,
    pub linear_velocity: Vec3,
    pub rotation: [f32; 4],
    pub angular: Vec3,
    pub enabled: bool,
}

/// One resolved contact, projected from a physics `ContactReport`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactInfo {
    pub body_a: PhysicsBodyHandle,
    pub body_b: PhysicsBodyHandle,
    pub normal: Vec3,
    pub depth: f32,
    pub point: Vec3,
}

/// The per-step diagnostic counts, projected from the physics step record. Every
/// one of these is a real count `PhysicsApi` exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepCounts {
    pub step_index: u64,
    pub body_count: u32,
    pub collider_count: u32,
    pub dynamic_body_count: u32,
    pub command_count: u32,
    pub event_count: u32,
    pub integration_count: u32,
    pub broad_phase_pair_count: u32,
    pub contact_pair_count: u32,
    pub solved_contact_count: u32,
    pub frictioned_contact_count: u32,
    pub solver_iteration_count: u32,
    pub substep_count: u32,
}

/// The deterministic crucible report for one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CrucibleReport {
    steps_run: u64,
    tracked_body_count: u32,
    counts: StepCounts,
    live_contact_count: u32,
    total_events: u64,
    replay_match: bool,
    query_hit_count: Option<u32>,
}

impl CrucibleReport {
    /// Build the report from the visible world's projected state.
    pub fn build(
        steps_run: u64,
        body_states: &[BodyState],
        counts: &StepCounts,
        contacts: &[ContactInfo],
        total_events: u64,
        replay_match: bool,
    ) -> Self {
        CrucibleReport {
            steps_run,
            tracked_body_count: body_states.len() as u32,
            counts: *counts,
            live_contact_count: contacts.len() as u32,
            total_events,
            replay_match,
            query_hit_count: None,
        }
    }

    /// Attach a deterministic query-hit count (the crucible runs one canonical
    /// overlap probe; physics exposes the query, the count is app-aggregated).
    pub fn with_query_hits(mut self, hits: u32) -> Self {
        self.query_hit_count = Some(hits);
        self
    }

    pub fn steps_run(&self) -> u64 {
        self.steps_run
    }
    pub fn counts(&self) -> StepCounts {
        self.counts
    }
    pub fn replay_match(&self) -> bool {
        self.replay_match
    }
    pub fn live_contact_count(&self) -> u32 {
        self.live_contact_count
    }
    pub fn total_events(&self) -> u64 {
        self.total_events
    }
    pub fn query_hit_count(&self) -> Option<u32> {
        self.query_hit_count
    }

    /// A small stable digest over the report's integer fields, for replay tests.
    pub fn digest(&self) -> u64 {
        let c = &self.counts;
        let mut acc: u64 = 1469598103934665603;
        let mut mix = |v: u64| {
            acc ^= v;
            acc = acc.wrapping_mul(1099511628211);
        };
        mix(self.steps_run);
        mix(self.tracked_body_count as u64);
        mix(c.step_index);
        mix(c.body_count as u64);
        mix(c.collider_count as u64);
        mix(c.dynamic_body_count as u64);
        mix(c.broad_phase_pair_count as u64);
        mix(c.contact_pair_count as u64);
        mix(c.solved_contact_count as u64);
        mix(c.frictioned_contact_count as u64);
        mix(c.solver_iteration_count as u64);
        mix(c.substep_count as u64);
        mix(self.live_contact_count as u64);
        mix(self.total_events);
        mix(self.replay_match as u64);
        mix(self.query_hit_count.map(|h| h as u64 + 1).unwrap_or(0));
        acc
    }

    /// The report rendered as ordered, human-readable lines. Fields physics does
    /// not surface are printed as `unavailable` with the reason.
    pub fn lines(&self) -> Vec<String> {
        let c = &self.counts;
        vec![
            format!("steps_run:              {}", self.steps_run),
            format!("step_index:             {}", c.step_index),
            format!("tracked_bodies:         {}", self.tracked_body_count),
            format!("body_count:             {}", c.body_count),
            format!("collider_count:         {}", c.collider_count),
            format!("dynamic_body_count:     {}", c.dynamic_body_count),
            format!("command_count:          {}", c.command_count),
            format!("broad_phase_pair_count: {}", c.broad_phase_pair_count),
            format!("contact_pair_count:     {}", c.contact_pair_count),
            format!("solved_contact_count:   {}", c.solved_contact_count),
            format!("frictioned_contacts:    {}", c.frictioned_contact_count),
            format!(
                "solver_iteration_count: {} (configured budget, not work)",
                c.solver_iteration_count
            ),
            format!("substep_count:          {}", c.substep_count),
            format!("event_count (step):     {}", c.event_count),
            format!("event_count (total):    {}", self.total_events),
            format!("live_contacts:          {}", self.live_contact_count),
            match self.query_hit_count {
                Some(h) => format!("query_hit_count:        {h}"),
                None => "query_hit_count:        unavailable (no probe this frame)".to_string(),
            },
            format!("replay_match:           {}", self.replay_match),
            // Remaining gap: physics does not surface lifecycle events yet.
            "collision_events:       unavailable (lifecycle events deferred)".to_string(),
        ]
    }

    /// The full report as a printable block.
    pub fn render(&self) -> String {
        let mut out = String::from("== Axiom Physics Crucible report ==\n");
        for line in self.lines() {
            out.push_str(&line);
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts() -> StepCounts {
        StepCounts {
            step_index: 10,
            body_count: 4,
            collider_count: 4,
            dynamic_body_count: 2,
            command_count: 1,
            event_count: 3,
            integration_count: 2,
            broad_phase_pair_count: 5,
            contact_pair_count: 2,
            solved_contact_count: 1,
            frictioned_contact_count: 1,
            solver_iteration_count: 8,
            substep_count: 1,
        }
    }

    fn state(raw: u64) -> BodyState {
        BodyState {
            handle: PhysicsBodyHandle::from_raw(raw),
            translation: Vec3::new(0.0, raw as f32, 0.0),
            linear_velocity: Vec3::ZERO,
            rotation: [0.0, 0.0, 0.0, 1.0],
            angular: Vec3::ZERO,
            enabled: true,
        }
    }

    #[test]
    fn build_captures_counts_and_tracked_bodies() {
        let report = CrucibleReport::build(
            12,
            &[state(1), state(2)],
            &counts(),
            &[],
            7,
            true,
        );
        assert_eq!(report.steps_run(), 12);
        assert_eq!(report.counts().broad_phase_pair_count, 5);
        assert_eq!(report.total_events(), 7);
        assert!(report.replay_match());
        assert_eq!(report.query_hit_count(), None);
    }

    #[test]
    fn with_query_hits_sets_the_field_and_changes_the_digest() {
        let base = CrucibleReport::build(1, &[state(1)], &counts(), &[], 0, true);
        let probed = base.clone().with_query_hits(3);
        assert_eq!(probed.query_hit_count(), Some(3));
        assert_ne!(base.digest(), probed.digest());
    }

    #[test]
    fn lines_report_friction_work_and_mark_remaining_gaps_unavailable() {
        let report = CrucibleReport::build(1, &[state(1)], &counts(), &[], 0, false);
        let text = report.render();
        // Friction now resolves live, so its work is reported, not "unavailable".
        assert!(text.contains("frictioned_contacts:    1"));
        // Lifecycle events remain a documented gap.
        assert!(text.contains("collision_events:       unavailable"));
        assert!(text.contains("query_hit_count:        unavailable"));
        assert!(text.contains("replay_match:           false"));
    }

    #[test]
    fn digest_is_stable_for_equal_reports() {
        let a = CrucibleReport::build(3, &[state(1)], &counts(), &[], 2, true);
        let b = CrucibleReport::build(3, &[state(1)], &counts(), &[], 2, true);
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a, b);
    }
}
