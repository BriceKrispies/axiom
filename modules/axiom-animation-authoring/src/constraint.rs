//! [`Constraint`] — the authored constraint vocabulary, and [`ResolvedConstraint`],
//! its name-resolved form.
//!
//! A constraint declares an invariant the pose should honor during a phase (a
//! foot pinned to a spot, gaze held on a target, weight kept over a support
//! foot). Each references at most one effector and one target *by name*; the
//! compiler resolves those uniformly. In this first version a *pinning*
//! constraint (`PinEffectorToTarget` / `PreserveFootContact`) overrides its
//! effector's world position to the target; the others are recorded as active in
//! the pose frame for a consumer/debugger to honor.

use axiom_math::Vec3;

use crate::ids::EffectorId;

/// The constraint vocabulary. The discriminant is a stable dispatch index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    /// Hold an effector exactly on a target (a hard pin).
    PinEffectorToTarget,
    /// Keep the gaze effector pointed at a target.
    KeepGazeOnTarget,
    /// Keep the center of mass over a named support effector.
    KeepCenterOfMassOverSupport,
    /// Orient an effector's surface toward a target.
    OrientSurfaceTowardTarget,
    /// Keep a foot effector on its contact target (a softer pin).
    PreserveFootContact,
}

impl ConstraintKind {
    /// Whether this kind pins its effector's world position to its target.
    /// Table-indexed over the fixed discriminant order.
    pub(crate) fn is_pin(self) -> bool {
        [true, false, false, false, true][self as usize]
    }
}

/// An authored constraint: a kind plus optional effector/target names.
#[derive(Debug, Clone, PartialEq)]
pub struct Constraint {
    kind: ConstraintKind,
    effector_name: Option<String>,
    target_name: Option<String>,
}

impl Constraint {
    /// Pin `effector` to `target`.
    pub(crate) fn pin_effector_to_target(effector: &str, target: &str) -> Self {
        Constraint {
            kind: ConstraintKind::PinEffectorToTarget,
            effector_name: Some(effector.to_string()),
            target_name: Some(target.to_string()),
        }
    }

    /// Keep the gaze on `target`.
    pub(crate) fn keep_gaze_on_target(target: &str) -> Self {
        Constraint {
            kind: ConstraintKind::KeepGazeOnTarget,
            effector_name: None,
            target_name: Some(target.to_string()),
        }
    }

    /// Keep the center of mass over the `support` effector.
    pub(crate) fn keep_center_of_mass_over_support(support: &str) -> Self {
        Constraint {
            kind: ConstraintKind::KeepCenterOfMassOverSupport,
            effector_name: Some(support.to_string()),
            target_name: None,
        }
    }

    /// Orient `effector`'s surface toward `target`.
    pub(crate) fn orient_surface_toward_target(effector: &str, target: &str) -> Self {
        Constraint {
            kind: ConstraintKind::OrientSurfaceTowardTarget,
            effector_name: Some(effector.to_string()),
            target_name: Some(target.to_string()),
        }
    }

    /// Preserve `effector`'s contact with `target`.
    pub(crate) fn preserve_foot_contact(effector: &str, target: &str) -> Self {
        Constraint {
            kind: ConstraintKind::PreserveFootContact,
            effector_name: Some(effector.to_string()),
            target_name: Some(target.to_string()),
        }
    }

    /// The constraint kind.
    pub(crate) fn kind(&self) -> ConstraintKind {
        self.kind
    }

    /// The referenced effector name, if any.
    pub(crate) fn effector_name(&self) -> Option<&str> {
        self.effector_name.as_deref()
    }

    /// The referenced target name, if any.
    pub(crate) fn target_name(&self) -> Option<&str> {
        self.target_name.as_deref()
    }
}

/// A resolved constraint: names replaced by an optional effector id and an
/// optional resolved target position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedConstraint {
    kind: ConstraintKind,
    effector: Option<EffectorId>,
    target: Option<Vec3>,
}

impl ResolvedConstraint {
    /// Construct a resolved constraint.
    pub(crate) fn new(kind: ConstraintKind, effector: Option<EffectorId>, target: Option<Vec3>) -> Self {
        ResolvedConstraint {
            kind,
            effector,
            target,
        }
    }

    /// The `(effector, target)` pin this constraint contributes, if it is a
    /// pinning kind with both endpoints resolved.
    pub(crate) fn pin(&self) -> Option<(EffectorId, Vec3)> {
        self.kind
            .is_pin()
            .then_some(())
            .and_then(|()| self.effector.zip(self.target))
    }

    /// The gaze target this constraint carries, if it is a `KeepGazeOnTarget`.
    pub(crate) fn gaze_target(&self) -> Option<Vec3> {
        (self.kind == ConstraintKind::KeepGazeOnTarget)
            .then_some(self.target)
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_pin_classifies_the_two_pinning_kinds() {
        assert!(ConstraintKind::PinEffectorToTarget.is_pin());
        assert!(ConstraintKind::PreserveFootContact.is_pin());
        assert!(!ConstraintKind::KeepGazeOnTarget.is_pin());
        assert!(!ConstraintKind::KeepCenterOfMassOverSupport.is_pin());
        assert!(!ConstraintKind::OrientSurfaceTowardTarget.is_pin());
    }

    #[test]
    fn constructors_set_kind_and_names() {
        let p = Constraint::pin_effector_to_target("left_foot_sole", "left_plant_spot");
        assert_eq!(p.kind(), ConstraintKind::PinEffectorToTarget);
        assert_eq!(p.effector_name(), Some("left_foot_sole"));
        assert_eq!(p.target_name(), Some("left_plant_spot"));

        let g = Constraint::keep_gaze_on_target("ball");
        assert_eq!(g.kind(), ConstraintKind::KeepGazeOnTarget);
        assert_eq!(g.effector_name(), None);
        assert_eq!(g.target_name(), Some("ball"));

        let c = Constraint::keep_center_of_mass_over_support("left_foot_sole");
        assert_eq!(c.kind(), ConstraintKind::KeepCenterOfMassOverSupport);
        assert_eq!(c.effector_name(), Some("left_foot_sole"));
        assert_eq!(c.target_name(), None);

        let o = Constraint::orient_surface_toward_target("right_foot_instep", "net_center");
        assert_eq!(o.kind(), ConstraintKind::OrientSurfaceTowardTarget);

        let f = Constraint::preserve_foot_contact("left_foot_sole", "left_plant_spot");
        assert_eq!(f.kind(), ConstraintKind::PreserveFootContact);
    }

    #[test]
    fn resolved_pin_only_yields_when_pinning_and_both_endpoints_present() {
        let pinned = ResolvedConstraint::new(
            ConstraintKind::PinEffectorToTarget,
            Some(EffectorId::from_raw(0)),
            Some(Vec3::new(1.0, 0.0, 0.0)),
        );
        assert_eq!(pinned.pin(), Some((EffectorId::from_raw(0), Vec3::new(1.0, 0.0, 0.0))));

        // A non-pinning kind never contributes a pin.
        let gaze = ResolvedConstraint::new(
            ConstraintKind::KeepGazeOnTarget,
            None,
            Some(Vec3::ZERO),
        );
        assert_eq!(gaze.pin(), None);

        // A pinning kind missing an endpoint yields no pin.
        let half = ResolvedConstraint::new(ConstraintKind::PinEffectorToTarget, None, Some(Vec3::ZERO));
        assert_eq!(half.pin(), None);
        assert_eq!(half.gaze_target(), None);

        // A gaze constraint yields its target; a non-gaze one does not.
        let gaze_c = ResolvedConstraint::new(ConstraintKind::KeepGazeOnTarget, None, Some(Vec3::new(0.0, 1.0, 2.0)));
        assert_eq!(gaze_c.gaze_target(), Some(Vec3::new(0.0, 1.0, 2.0)));
        assert_eq!(pinned.gaze_target(), None);
    }
}
