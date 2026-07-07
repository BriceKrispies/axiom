//! [`MotionPlan`] — a validated, name-resolved motion ready to sample. Produced
//! by [`crate::motion_compiler`] from a [`crate::motion_spec::MotionSpec`] and its
//! rig; consumed by [`crate::motion_sampler`].
//!
//! A plan owns a *copy* of its rig so sampling needs nothing else: forward
//! kinematics, effector resolution, pins, and events all read from the plan
//! alone.

use axiom_math::Vec3;

use crate::humanoid_rig::HumanoidRigSpec;
use crate::ids::TargetId;
use crate::motion_event::ResolvedEvent;
use crate::motion_phase::ResolvedPhase;

/// A compiled, sample-ready motion.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionPlan {
    rig: HumanoidRigSpec,
    duration: u64,
    phases: Vec<ResolvedPhase>,
    events: Vec<ResolvedEvent>,
    targets: Vec<(String, Vec3)>,
}

impl MotionPlan {
    /// Assemble a plan from a rig, a duration, resolved phases, resolved events,
    /// and the resolved targets (name + position, indexed by [`TargetId`]).
    pub(crate) fn new(
        rig: HumanoidRigSpec,
        duration: u64,
        phases: Vec<ResolvedPhase>,
        events: Vec<ResolvedEvent>,
        targets: Vec<(String, Vec3)>,
    ) -> Self {
        MotionPlan {
            rig,
            duration,
            phases,
            events,
            targets,
        }
    }

    /// The actor rig.
    pub(crate) fn rig(&self) -> &HumanoidRigSpec {
        &self.rig
    }

    /// The world position of the target at `id`, if in range.
    pub(crate) fn target_position(&self, id: TargetId) -> Option<Vec3> {
        self.targets.get(id.raw() as usize).map(|(_, p)| *p)
    }

    /// The world position of the target named `name`, if declared.
    pub(crate) fn target_position_by_name(&self, name: &str) -> Option<Vec3> {
        self.targets.iter().find(|(n, _)| n == name).map(|(_, p)| *p)
    }

    /// The name of the phase covering `tick`, if any.
    pub(crate) fn active_phase_name(&self, tick: u64) -> Option<&str> {
        self.active_phase(tick).map(ResolvedPhase::name)
    }

    /// The total duration in ticks.
    pub(crate) fn duration(&self) -> u64 {
        self.duration
    }

    /// The resolved phases in order.
    pub(crate) fn phases(&self) -> &[ResolvedPhase] {
        &self.phases
    }

    /// The resolved events in order.
    pub(crate) fn events(&self) -> &[ResolvedEvent] {
        &self.events
    }

    /// The phase covering `tick`, if any.
    pub(crate) fn active_phase(&self, tick: u64) -> Option<&ResolvedPhase> {
        self.phases.iter().find(|p| p.covers(tick))
    }

    /// The events firing exactly at `tick`.
    pub(crate) fn events_at(&self, tick: u64) -> Vec<ResolvedEvent> {
        self.events
            .iter()
            .filter(|e| e.tick().raw() == tick)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ease::EaseCurve;
    use crate::ids::{EffectorId, TargetId};
    use crate::motion_event::{EventKind, UNUSED_EFFECTOR, UNUSED_TARGET};
    use crate::root_motion::{ResolvedRootMotion, RootMotionKind};
    use axiom_kernel::Tick;
    use axiom_math::Vec3;

    fn phase(name: &str, start: u64, end: u64) -> ResolvedPhase {
        ResolvedPhase::new(
            (name.to_string(), start, end),
            ResolvedRootMotion::new(RootMotionKind::Hold, Vec3::ZERO, Vec3::ZERO),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            EaseCurve::Linear,
            1.0,
        )
    }

    fn plan() -> MotionPlan {
        MotionPlan::new(
            HumanoidRigSpec::standard_humanoid(),
            30,
            vec![phase("a", 0, 10), phase("b", 10, 30)],
            vec![
                ResolvedEvent::new(
                    EventKind::Named,
                    Tick::new(5),
                    "cue".to_string(),
                    UNUSED_EFFECTOR,
                    UNUSED_TARGET,
                    UNUSED_TARGET,
                    0.0,
                ),
                ResolvedEvent::new(
                    EventKind::BallContact,
                    Tick::new(20),
                    "ball_contact".to_string(),
                    EffectorId::from_raw(2),
                    TargetId::from_raw(0),
                    TargetId::from_raw(1),
                    0.5,
                ),
            ],
            vec![
                ("ball".to_string(), Vec3::new(0.0, 0.0, 0.0)),
                ("net_center".to_string(), Vec3::new(0.0, 0.8, 8.0)),
            ],
        )
    }

    #[test]
    fn accessors_expose_the_plan() {
        let p = plan();
        assert_eq!(p.rig().joints().len(), 25);
        assert_eq!(p.duration(), 30);
        assert_eq!(p.phases().len(), 2);
        assert_eq!(p.events().len(), 2);
        assert_eq!(p.target_position(TargetId::from_raw(1)), Some(Vec3::new(0.0, 0.8, 8.0)));
        assert_eq!(p.target_position(TargetId::from_raw(9)), None);
        assert_eq!(p.target_position_by_name("ball"), Some(Vec3::new(0.0, 0.0, 0.0)));
        assert_eq!(p.target_position_by_name("nope"), None);
        assert_eq!(p.active_phase_name(5), Some("a"));
        assert_eq!(p.active_phase_name(20), Some("b"));
        assert_eq!(p.active_phase_name(99), None);
    }

    #[test]
    fn active_phase_finds_the_covering_span_or_none() {
        let p = plan();
        assert_eq!(p.active_phase(0).unwrap().end(), 10);
        assert_eq!(p.active_phase(9).unwrap().end(), 10);
        assert_eq!(p.active_phase(10).unwrap().end(), 30);
        assert_eq!(p.active_phase(29).unwrap().end(), 30);
        assert!(p.active_phase(30).is_none());
    }

    #[test]
    fn events_at_selects_by_exact_tick() {
        let p = plan();
        assert_eq!(p.events_at(5).len(), 1);
        assert_eq!(p.events_at(5)[0].name(), "cue");
        assert_eq!(p.events_at(20).len(), 1);
        assert!(p.events_at(20)[0].is_ball_contact());
        assert_eq!(p.events_at(7).len(), 0);
    }
}
