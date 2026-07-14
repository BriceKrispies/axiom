//! The ten named **virtual-muscle groups** and their deterministic control
//! parameters.
//!
//! A muscle group is a bundle of the character's actuation for one body region.
//! The groups and their order are fixed (the facade addresses them by a stable
//! `u8` code `0..=9`), so a caller can configure and read them without naming any
//! module-private type.

/// The number of muscle groups.
pub(crate) const MUSCLE_GROUP_COUNT: usize = 10;

/// A named virtual-muscle group. The discriminant is the stable facade code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MuscleGroup {
    Core = 0,
    Pelvis = 1,
    Spine = 2,
    NeckHead = 3,
    LeftLeg = 4,
    RightLeg = 5,
    LeftAnkle = 6,
    RightAnkle = 7,
    LeftArm = 8,
    RightArm = 9,
}

/// The groups in code order — the single source of truth for `u8` ↔ group.
pub(crate) const MUSCLE_GROUPS: [MuscleGroup; MUSCLE_GROUP_COUNT] = [
    MuscleGroup::Core,
    MuscleGroup::Pelvis,
    MuscleGroup::Spine,
    MuscleGroup::NeckHead,
    MuscleGroup::LeftLeg,
    MuscleGroup::RightLeg,
    MuscleGroup::LeftAnkle,
    MuscleGroup::RightAnkle,
    MuscleGroup::LeftArm,
    MuscleGroup::RightArm,
];

/// The greppable name of each group, in code order.
const MUSCLE_GROUP_NAMES: [&str; MUSCLE_GROUP_COUNT] = [
    "core",
    "pelvis",
    "spine",
    "neck_head",
    "left_leg",
    "right_leg",
    "left_ankle",
    "right_ankle",
    "left_arm",
    "right_arm",
];

impl MuscleGroup {
    /// The group for a facade code, clamped into range (branchless).
    pub(crate) fn from_code(code: u8) -> MuscleGroup {
        MUSCLE_GROUPS[(code as usize).min(MUSCLE_GROUP_COUNT - 1)]
    }

    /// The group's index into a per-group array.
    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    /// The group's greppable name.
    pub(crate) fn name(self) -> &'static str {
        MUSCLE_GROUP_NAMES[self.index()]
    }
}

/// Deterministic per-group control parameters. Private, so plain `f32` fields are
/// fine (the naked-float public-API lint keys on declared visibility).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MuscleGroupParams {
    /// How hard the group pulls toward its target.
    pub(crate) stiffness: f32,
    /// How much the group resists motion (velocity opposition).
    pub(crate) damping: f32,
    /// The group's peak actuation.
    pub(crate) max_torque: f32,
    /// The group's baseline rest-posture stabilization weight.
    pub(crate) phase_weight: f32,
}

impl MuscleGroupParams {
    /// Assemble params from `(stiffness, damping, max_torque, phase_weight)`.
    pub(crate) const fn new(
        stiffness: f32,
        damping: f32,
        max_torque: f32,
        phase_weight: f32,
    ) -> Self {
        MuscleGroupParams {
            stiffness,
            damping,
            max_torque,
            phase_weight,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_names_and_from_code_round_trip_and_clamp() {
        // Every group round-trips through its code and has a distinct name.
        MUSCLE_GROUPS.iter().enumerate().for_each(|(i, &g)| {
            assert_eq!(g.index(), i);
            assert_eq!(MuscleGroup::from_code(i as u8), g);
        });
        assert_eq!(MuscleGroup::Core.name(), "core");
        assert_eq!(MuscleGroup::RightArm.name(), "right_arm");
        // Out-of-range codes clamp to the last group.
        assert_eq!(MuscleGroup::from_code(200), MuscleGroup::RightArm);
        // Names are unique.
        let mut names: Vec<&str> = MUSCLE_GROUPS.iter().map(|g| g.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), MUSCLE_GROUP_COUNT);
    }

    #[test]
    fn params_store_their_fields() {
        let p = MuscleGroupParams::new(1.0, 2.0, 3.0, 0.4);
        assert_eq!(
            (p.stiffness, p.damping, p.max_torque, p.phase_weight),
            (1.0, 2.0, 3.0, 0.4)
        );
    }
}
