//! The deterministic configuration of the virtual-muscle controller: the
//! per-group base [`VirtualMuscleProfile`], the global [`MuscleStyle`] scalars,
//! the [`SupportMode`], and the per-phase [`MusclePhaseProfile`] a caller supplies
//! each tick.

use crate::muscle_group::{MuscleGroup, MuscleGroupParams, MUSCLE_GROUP_COUNT};

/// The per-group base control parameters for the whole body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VirtualMuscleProfile {
    groups: [MuscleGroupParams; MUSCLE_GROUP_COUNT],
}

impl VirtualMuscleProfile {
    /// Assemble a profile from per-group params in group-code order.
    pub(crate) fn new(groups: [MuscleGroupParams; MUSCLE_GROUP_COUNT]) -> Self {
        VirtualMuscleProfile { groups }
    }

    /// A balanced default profile — moderate stiffness/damping, a firm core and
    /// legs, softer arms, with rest-posture weight on the postural groups.
    pub(crate) fn default_profile() -> Self {
        // (stiffness, damping, max_torque, rest phase_weight) per group.
        VirtualMuscleProfile::new([
            MuscleGroupParams::new(1.0, 0.5, 1.0, 0.7), // core
            MuscleGroupParams::new(1.0, 0.5, 1.2, 0.7), // pelvis
            MuscleGroupParams::new(0.9, 0.5, 0.9, 0.6), // spine
            MuscleGroupParams::new(0.6, 0.4, 0.4, 0.5), // neck_head
            MuscleGroupParams::new(1.0, 0.5, 1.2, 0.6), // left_leg
            MuscleGroupParams::new(1.0, 0.5, 1.2, 0.5), // right_leg
            MuscleGroupParams::new(0.9, 0.6, 0.7, 0.5), // left_ankle
            MuscleGroupParams::new(0.9, 0.6, 0.7, 0.4), // right_ankle
            MuscleGroupParams::new(0.6, 0.4, 0.5, 0.4), // left_arm
            MuscleGroupParams::new(0.6, 0.4, 0.5, 0.4), // right_arm
        ])
    }

    /// The params for `group`.
    pub(crate) fn group(&self, group: MuscleGroup) -> MuscleGroupParams {
        self.groups[group.index()]
    }
}

/// The global muscle-style scalars — deterministic knobs the caller tunes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MuscleStyle {
    /// Scales every group's peak actuation.
    pub(crate) muscle_strength: f32,
    /// Scales recovery / settling damping.
    pub(crate) muscle_damping: f32,
    /// Scales the balance correction toward the support target.
    pub(crate) balance_strength: f32,
}

impl MuscleStyle {
    /// Assemble a style from its three scalars.
    pub(crate) fn new(muscle_strength: f32, muscle_damping: f32, balance_strength: f32) -> Self {
        MuscleStyle {
            muscle_strength,
            muscle_damping,
            balance_strength,
        }
    }

    /// A neutral, unit-strength style.
    pub(crate) fn default_style() -> Self {
        MuscleStyle::new(1.0, 1.0, 1.0)
    }
}

/// Which feet carry the body this phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupportMode {
    BothFeet = 0,
    LeftFoot = 1,
    RightFoot = 2,
    /// No stable support — the balance controller falls back to the CoM (no pull).
    Airborne = 3,
}

/// Support modes in code order.
const SUPPORT_MODES: [SupportMode; 4] = [
    SupportMode::BothFeet,
    SupportMode::LeftFoot,
    SupportMode::RightFoot,
    SupportMode::Airborne,
];

impl SupportMode {
    /// The stable facade code.
    pub(crate) const fn code(self) -> u8 {
        self as u8
    }

    /// The support mode for a facade code, clamped into range (branchless).
    pub(crate) fn from_code(code: u8) -> SupportMode {
        SUPPORT_MODES[(code as usize).min(SUPPORT_MODES.len() - 1)]
    }

    /// The mode's index for a `[_; 4]` candidate table.
    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

/// The per-phase policy the caller supplies each tick: the support mode plus a
/// per-group weight (the authored emphasis for the active phase).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MusclePhaseProfile {
    support: SupportMode,
    group_weights: [f32; MUSCLE_GROUP_COUNT],
}

impl MusclePhaseProfile {
    /// Assemble a phase profile from a support mode and per-group weights.
    pub(crate) fn new(support: SupportMode, group_weights: [f32; MUSCLE_GROUP_COUNT]) -> Self {
        MusclePhaseProfile {
            support,
            group_weights,
        }
    }

    /// The support mode for the phase.
    pub(crate) fn support(&self) -> SupportMode {
        self.support
    }

    /// The authored weight for `group`.
    pub(crate) fn weight(&self, group: MuscleGroup) -> f32 {
        self.group_weights[group.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_balanced_and_addressable() {
        let p = VirtualMuscleProfile::default_profile();
        // The pelvis and legs carry real actuation; every group has rest weight.
        assert!(p.group(MuscleGroup::Pelvis).max_torque > 0.0);
        assert!(p.group(MuscleGroup::LeftLeg).stiffness > 0.0);
        crate::muscle_group::MUSCLE_GROUPS
            .iter()
            .for_each(|&g| assert!(p.group(g).phase_weight > 0.0));
    }

    #[test]
    fn support_modes_round_trip_and_clamp() {
        SUPPORT_MODES.iter().enumerate().for_each(|(i, &m)| {
            assert_eq!(m.code() as usize, i);
            assert_eq!(m.index(), i);
            assert_eq!(SupportMode::from_code(i as u8), m);
        });
        assert_eq!(SupportMode::from_code(9), SupportMode::Airborne);
    }

    #[test]
    fn phase_profile_and_style_expose_their_fields() {
        let phase = MusclePhaseProfile::new(SupportMode::LeftFoot, [0.5; MUSCLE_GROUP_COUNT]);
        assert_eq!(phase.support(), SupportMode::LeftFoot);
        assert_eq!(phase.weight(MuscleGroup::Core), 0.5);
        let s = MuscleStyle::new(2.0, 0.5, 1.5);
        assert_eq!(
            (s.muscle_strength, s.muscle_damping, s.balance_strength),
            (2.0, 0.5, 1.5)
        );
        assert_eq!(
            MuscleStyle::default_style(),
            MuscleStyle::new(1.0, 1.0, 1.0)
        );
    }
}
