//! [`MotionSpec`] — an authored motion: a name, a duration in ticks, the actor
//! rig it drives, named targets, style scalars, ordered phases, and ordered
//! events. This is the pure authored data the [`crate::motion_compiler`]
//! validates and resolves.

use axiom_kernel::Tick;
use axiom_math::Vec3;

use crate::ids::{RigId, TargetId};
use crate::motion_event::MotionEvent;
use crate::motion_phase::MotionPhase;

/// An authored motion specification.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionSpec {
    name: String,
    duration: Tick,
    rig: RigId,
    targets: Vec<(String, Vec3)>,
    style: Vec<(String, f32)>,
    phases: Vec<MotionPhase>,
    events: Vec<MotionEvent>,
}

impl MotionSpec {
    /// A new empty motion `name` of `duration` ticks driving `rig`.
    pub(crate) fn new(name: &str, duration: Tick, rig: RigId) -> Self {
        MotionSpec {
            name: name.to_string(),
            duration,
            rig,
            targets: Vec::new(),
            style: Vec::new(),
            phases: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Declare a target `name` at `position`, returning its id (its index).
    pub(crate) fn add_target(&mut self, name: &str, position: Vec3) -> TargetId {
        let id = TargetId::from_raw(self.targets.len() as u64);
        self.targets.push((name.to_string(), position));
        id
    }

    /// Set style scalar `name` to `value`, overwriting any prior value.
    pub(crate) fn set_style(&mut self, name: &str, value: f32) {
        let pos = self.style.iter().position(|(n, _)| n == name);
        // Overwrite in place if present; otherwise append. Two independent,
        // branch-free effects keyed on the same lookup.
        pos.into_iter().for_each(|i| self.style[i].1 = value);
        pos.is_none().then(|| self.style.push((name.to_string(), value)));
    }

    /// Append a phase, returning its index.
    pub(crate) fn add_phase(&mut self, phase: MotionPhase) -> u64 {
        let index = self.phases.len() as u64;
        self.phases.push(phase);
        index
    }

    /// A mutable handle to the phase at `index`, for attaching goals/constraints.
    pub(crate) fn phase_mut(&mut self, index: u64) -> Option<&mut MotionPhase> {
        self.phases.get_mut(index as usize)
    }

    /// Append an event.
    pub(crate) fn add_event(&mut self, event: MotionEvent) {
        self.events.push(event);
    }

    /// The motion name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The total duration in ticks.
    pub(crate) fn duration(&self) -> Tick {
        self.duration
    }

    /// The actor rig.
    pub(crate) fn rig(&self) -> RigId {
        self.rig
    }

    /// The declared targets in order.
    pub(crate) fn targets(&self) -> &[(String, Vec3)] {
        &self.targets
    }

    /// The id of the target named `name`, if declared.
    pub(crate) fn target_id(&self, name: &str) -> Option<TargetId> {
        self.targets
            .iter()
            .position(|(n, _)| n == name)
            .map(|i| TargetId::from_raw(i as u64))
    }

    /// The position of the target named `name`, if declared.
    pub(crate) fn target_position(&self, name: &str) -> Option<Vec3> {
        self.targets.iter().find(|(n, _)| n == name).map(|(_, p)| *p)
    }

    /// The value of style scalar `name`, if set.
    pub(crate) fn style_value(&self, name: &str) -> Option<f32> {
        self.style.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }

    /// The style scalars in insertion order.
    pub(crate) fn styles(&self) -> &[(String, f32)] {
        &self.style
    }

    /// The phases in order.
    pub(crate) fn phases(&self) -> &[MotionPhase] {
        &self.phases
    }

    /// The events in order.
    pub(crate) fn events(&self) -> &[MotionEvent] {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_get_sequential_ids_and_resolve_by_name_and_id() {
        let mut m = MotionSpec::new("kick", Tick::new(60), RigId::from_raw(0));
        assert_eq!(m.name(), "kick");
        assert_eq!(m.duration(), Tick::new(60));
        assert_eq!(m.rig(), RigId::from_raw(0));
        let ball = m.add_target("ball", Vec3::new(0.0, 0.0, 0.0));
        let net = m.add_target("net_center", Vec3::new(0.0, 0.8, 8.0));
        assert_eq!(ball, TargetId::from_raw(0));
        assert_eq!(net, TargetId::from_raw(1));
        assert_eq!(m.target_id("net_center"), Some(net));
        assert_eq!(m.target_id("nope"), None);
        assert_eq!(m.target_position("ball"), Some(Vec3::ZERO));
        assert_eq!(m.target_position("nope"), None);
        assert_eq!(net, TargetId::from_raw(1));
        assert_eq!(m.targets().len(), 2);
    }

    #[test]
    fn set_style_inserts_then_overwrites() {
        let mut m = MotionSpec::new("kick", Tick::new(60), RigId::from_raw(0));
        assert_eq!(m.style_value("power"), None);
        m.set_style("power", 0.5);
        assert_eq!(m.style_value("power"), Some(0.5));
        m.set_style("power", 0.9);
        assert_eq!(m.style_value("power"), Some(0.9));
        m.set_style("reach", 0.3);
        assert_eq!(m.style_value("reach"), Some(0.3));
        assert_eq!(m.style_value("power"), Some(0.9));
        assert_eq!(m.styles().len(), 2);
    }

    #[test]
    fn phases_and_events_append_and_expose() {
        let mut m = MotionSpec::new("kick", Tick::new(60), RigId::from_raw(0));
        let i = m.add_phase(MotionPhase::new("approach", Tick::new(0), Tick::new(10)));
        assert_eq!(i, 0);
        assert!(m.phase_mut(0).is_some());
        assert!(m.phase_mut(9).is_none());
        m.add_event(MotionEvent::named(Tick::new(5), "cue"));
        assert_eq!(m.phases().len(), 1);
        assert_eq!(m.events().len(), 1);
    }
}
