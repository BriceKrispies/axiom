//! [`PoseFrame`] — the deterministic result of sampling a [`crate::motion_plan`]
//! at one tick: the root transform, every joint's local transform, every
//! effector's world transform, and the constraints, contacts, and events active
//! at that tick.
//!
//! A `PoseFrame` is pure data derived by arithmetic from the plan and the tick,
//! so sampling the same plan at the same tick yields a byte-identical (and
//! [`PartialEq`]-equal, `Debug`-equal) frame — the replay/debug contract.

use axiom_math::Transform;

use crate::constraint::ResolvedConstraint;
use crate::contact::ResolvedContact;
use crate::ids::{EffectorId, JointId};
use crate::motion_event::ResolvedEvent;

/// A sampled pose at one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct PoseFrame {
    root: Transform,
    joint_locals: Vec<Transform>,
    joint_worlds: Vec<Transform>,
    effector_worlds: Vec<Transform>,
    active_constraints: Vec<ResolvedConstraint>,
    active_contacts: Vec<ResolvedContact>,
    events: Vec<ResolvedEvent>,
}

impl PoseFrame {
    /// Assemble a pose frame from its parts.
    pub(crate) fn new(
        root: Transform,
        joint_locals: Vec<Transform>,
        joint_worlds: Vec<Transform>,
        effector_worlds: Vec<Transform>,
        active_constraints: Vec<ResolvedConstraint>,
        active_contacts: Vec<ResolvedContact>,
        events: Vec<ResolvedEvent>,
    ) -> Self {
        PoseFrame {
            root,
            joint_locals,
            joint_worlds,
            effector_worlds,
            active_constraints,
            active_contacts,
            events,
        }
    }

    /// The world-space root transform.
    pub(crate) fn root(&self) -> Transform {
        self.root
    }

    /// The local transform of joint `id`, if in range.
    pub(crate) fn joint_local(&self, id: JointId) -> Option<Transform> {
        self.joint_locals.get(id.raw() as usize).copied()
    }

    /// The world transform of joint `id`, if in range (the composed FK result —
    /// used to drive a kinematic physics body at the joint).
    pub(crate) fn joint_world(&self, id: JointId) -> Option<Transform> {
        self.joint_worlds.get(id.raw() as usize).copied()
    }

    /// The world transform of effector `id`, if in range.
    pub(crate) fn effector_world(&self, id: EffectorId) -> Option<Transform> {
        self.effector_worlds.get(id.raw() as usize).copied()
    }

    /// The constraints active at this tick.
    pub(crate) fn active_constraints(&self) -> &[ResolvedConstraint] {
        &self.active_constraints
    }

    /// The contacts active at this tick.
    pub(crate) fn active_contacts(&self) -> &[ResolvedContact] {
        &self.active_contacts
    }

    /// The names/labels of the events emitted at this tick.
    pub(crate) fn event_names(&self) -> Vec<&str> {
        self.events.iter().map(ResolvedEvent::name).collect()
    }

    /// The first ball-contact event emitted at this tick, if any.
    pub(crate) fn ball_contact(&self) -> Option<&ResolvedEvent> {
        self.events.iter().find(|e| e.is_ball_contact())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::ConstraintKind;
    use crate::ids::TargetId;
    use crate::motion_event::{EventKind, UNUSED_EFFECTOR, UNUSED_TARGET};
    use axiom_kernel::Tick;
    use axiom_math::Vec3;

    fn frame() -> PoseFrame {
        PoseFrame::new(
            Transform::from_translation(Vec3::new(0.0, 0.0, 1.0)),
            vec![
                Transform::IDENTITY,
                Transform::from_translation(Vec3::new(0.0, 0.9, 0.0)),
            ],
            vec![
                Transform::IDENTITY,
                Transform::from_translation(Vec3::new(0.0, 0.9, 1.0)),
            ],
            vec![Transform::from_translation(Vec3::new(0.25, 0.0, -0.1))],
            vec![ResolvedConstraint::new(
                ConstraintKind::KeepGazeOnTarget,
                None,
                Some(Vec3::ZERO),
            )],
            vec![],
            vec![
                ResolvedEvent::new(
                    EventKind::Named,
                    Tick::new(2),
                    "cue".to_string(),
                    UNUSED_EFFECTOR,
                    UNUSED_TARGET,
                    UNUSED_TARGET,
                    0.0,
                ),
                ResolvedEvent::new(
                    EventKind::BallContact,
                    Tick::new(2),
                    "ball_contact".to_string(),
                    EffectorId::from_raw(2),
                    UNUSED_TARGET,
                    TargetId::from_raw(1),
                    0.6,
                ),
            ],
        )
    }

    #[test]
    fn accessors_read_every_field() {
        let f = frame();
        assert_eq!(f.root().translation, Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(
            f.joint_local(JointId::from_raw(1)).unwrap().translation,
            Vec3::new(0.0, 0.9, 0.0)
        );
        assert_eq!(f.joint_local(JointId::from_raw(9)), None);
        assert_eq!(
            f.joint_world(JointId::from_raw(1)).unwrap().translation,
            Vec3::new(0.0, 0.9, 1.0)
        );
        assert_eq!(f.joint_world(JointId::from_raw(9)), None);
        assert_eq!(
            f.effector_world(EffectorId::from_raw(0))
                .unwrap()
                .translation,
            Vec3::new(0.25, 0.0, -0.1)
        );
        assert_eq!(f.effector_world(EffectorId::from_raw(9)), None);
        assert_eq!(f.active_constraints().len(), 1);
        assert_eq!(f.active_contacts().len(), 0);
        assert_eq!(f.event_names(), vec!["cue", "ball_contact"]);
        assert!(f.ball_contact().is_some());
        assert_eq!(f.ball_contact().unwrap().power(), 0.6);
    }

    #[test]
    fn a_frame_without_a_ball_contact_reports_none() {
        let f = PoseFrame::new(
            Transform::IDENTITY,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![ResolvedEvent::new(
                EventKind::Named,
                Tick::new(0),
                "n".to_string(),
                UNUSED_EFFECTOR,
                UNUSED_TARGET,
                UNUSED_TARGET,
                0.0,
            )],
        );
        assert_eq!(f.ball_contact(), None);
        assert_eq!(f.event_names(), vec!["n"]);
    }

    #[test]
    fn equal_parts_produce_equal_frames() {
        assert_eq!(frame(), frame());
    }
}
