//! The **`VirtualMuscleController`** — the deterministic active-control layer that
//! turns authored physical objectives + style into muscle / balance / contact
//! commands. `axiom-physics` simulates; *this* is what keeps the character from
//! collapsing into a ragdoll.
//!
//! It is a pure function: `command(profile, style, phase, objectives, body)` →
//! [`VirtualMuscleCommand`]. The bridge applies the command's balance force +
//! upright torque to the dynamic pelvis and carries the rest into the frame. The
//! sub-behaviours are folded in as deterministic stages:
//!
//! - **rest posture** — a baseline per-group stabilization weight that fades as
//!   the authored motor drive rises, plus the pelvis upright torque;
//! - **balance** — a deterministic centre-of-mass estimate and a horizontal pull
//!   toward the support target;
//! - **foot plant** — a plant-hold strength that releases when the authored
//!   plant objective ends;
//! - **strike / follow-through / recovery shaping** — per-group actuation and
//!   recovery damping scaled by the phase profile and style.

use axiom_math::{Transform, Vec3};

use crate::muscle_group::{MuscleGroup, MUSCLE_GROUP_COUNT};
use crate::muscle_profile::{MusclePhaseProfile, MuscleStyle, SupportMode, VirtualMuscleProfile};

/// Force gain for the horizontal balance pull toward the support target.
const BALANCE_GAIN: f32 = 30.0;
/// Torque gain for the pelvis upright correction.
const UPRIGHT_GAIN: f32 = 8.0;

/// The neutral, authored control data the controller reads (fed by the bridge).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MuscleObjectives {
    /// The active foot-plant world target, if a plant objective is active.
    pub(crate) foot_plant: Option<Vec3>,
    /// The active phase's authored motor drive (0..=1).
    pub(crate) motor_drive: f32,
    /// The strike impulse to apply to the ball this tick, if any.
    pub(crate) ball_impulse: Option<Vec3>,
}

/// The current physics body state the balance stage reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MuscleBodyState<'a> {
    /// The bound body world positions used for the centre-of-mass estimate.
    pub(crate) com_samples: &'a [Vec3],
    /// The left foot body world position.
    pub(crate) left_foot: Vec3,
    /// The right foot body world position.
    pub(crate) right_foot: Vec3,
    /// The pelvis body world transform (for the upright torque).
    pub(crate) pelvis: Transform,
}

/// The deterministic per-tick control command the controller produces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VirtualMuscleCommand {
    support_mode: SupportMode,
    center_of_mass: Vec3,
    support_target: Vec3,
    group_weight: [f32; MUSCLE_GROUP_COUNT],
    group_max_torque: [f32; MUSCLE_GROUP_COUNT],
    plant_strength: f32,
    balance_correction: Vec3,
    upright_torque: Vec3,
    recovery_damping: f32,
    strike_impulse: Option<Vec3>,
}

impl VirtualMuscleCommand {
    /// The active support mode.
    pub(crate) fn support_mode(&self) -> SupportMode {
        self.support_mode
    }

    /// The deterministic centre-of-mass estimate.
    pub(crate) fn center_of_mass(&self) -> Vec3 {
        self.center_of_mass
    }

    /// The support target the balance stage pulls the CoM toward.
    pub(crate) fn support_target(&self) -> Vec3 {
        self.support_target
    }

    /// The final actuation weight for `group`.
    pub(crate) fn group_weight(&self, group: MuscleGroup) -> f32 {
        self.group_weight[group.index()]
    }

    /// The peak actuation for `group` (scaled by muscle strength).
    pub(crate) fn group_max_torque(&self, group: MuscleGroup) -> f32 {
        self.group_max_torque[group.index()]
    }

    /// The plant-hold strength (0 when no plant objective is active).
    pub(crate) fn plant_strength(&self) -> f32 {
        self.plant_strength
    }

    /// The horizontal balance-correction force applied to the pelvis.
    pub(crate) fn balance_correction(&self) -> Vec3 {
        self.balance_correction
    }

    /// The upright-correction torque applied to the pelvis.
    pub(crate) fn upright_torque(&self) -> Vec3 {
        self.upright_torque
    }

    /// The recovery / settling damping factor.
    pub(crate) fn recovery_damping(&self) -> f32 {
        self.recovery_damping
    }

    /// A deterministic, one-field-per-line debug report (the `strike=` field
    /// reports the applied ball-impulse magnitude, `0` when none).
    pub(crate) fn report(&self) -> String {
        let weights: Vec<String> = crate::muscle_group::MUSCLE_GROUPS
            .iter()
            .map(|g| format!("{}={:.3}", g.name(), self.group_weight(*g)))
            .collect();
        format!(
            "support={} com=({:.3},{:.3},{:.3}) target=({:.3},{:.3},{:.3}) plant={:.3} \
             balance=({:.3},{:.3},{:.3}) upright=({:.3},{:.3},{:.3}) recovery={:.3} strike={:.3} \
             weights[{}]",
            self.support_mode.code(),
            self.center_of_mass.x,
            self.center_of_mass.y,
            self.center_of_mass.z,
            self.support_target.x,
            self.support_target.y,
            self.support_target.z,
            self.plant_strength,
            self.balance_correction.x,
            self.balance_correction.y,
            self.balance_correction.z,
            self.upright_torque.x,
            self.upright_torque.y,
            self.upright_torque.z,
            self.recovery_damping,
            self.strike_impulse.map(|i| i.length()).unwrap_or(0.0),
            weights.join(","),
        )
    }
}

/// The deterministic virtual-muscle controller (a namespace for `command`).
pub(crate) struct VirtualMuscleController;

impl VirtualMuscleController {
    /// Compute the control command for one tick. Pure and deterministic.
    pub(crate) fn command(
        profile: &VirtualMuscleProfile,
        style: MuscleStyle,
        phase: MusclePhaseProfile,
        objectives: MuscleObjectives,
        body: MuscleBodyState<'_>,
    ) -> VirtualMuscleCommand {
        let motor = objectives.motor_drive.clamp(0.0, 1.0);

        // --- rest posture: baseline stabilization that fades as drive rises ---
        // and the strike/recovery weight shaping.
        let group_weight: [f32; MUSCLE_GROUP_COUNT] = core::array::from_fn(|i| {
            let g = MuscleGroup::from_code(i as u8);
            let rest = (1.0 - motor) * profile.group(g).phase_weight;
            phase.weight(g).max(rest).clamp(0.0, 1.0)
        });
        let group_max_torque: [f32; MUSCLE_GROUP_COUNT] = core::array::from_fn(|i| {
            let g = MuscleGroup::from_code(i as u8);
            profile.group(g).max_torque * style.muscle_strength.max(0.0) * group_weight[i]
        });

        // --- balance: deterministic CoM + a pull toward the support target ---
        let center_of_mass = mean(body.com_samples);
        let both_mid = body.left_foot.add(body.right_foot).mul_scalar(0.5);
        let support_target = [both_mid, body.left_foot, body.right_foot, center_of_mass][phase.support().index()];
        let pelvis_stiffness = profile.group(MuscleGroup::Pelvis).stiffness;
        let to_support = support_target.subtract(center_of_mass);
        let balance_correction = Vec3::new(to_support.x, 0.0, to_support.z)
            .mul_scalar(pelvis_stiffness * style.balance_strength.max(0.0) * BALANCE_GAIN);

        // --- upright torque: rotate the pelvis 'up' back toward world up ---
        let pelvis_up = body.pelvis.rotation.rotate(Vec3::new(0.0, 1.0, 0.0));
        let upright_torque = pelvis_up.cross(Vec3::new(0.0, 1.0, 0.0)).mul_scalar(pelvis_stiffness * UPRIGHT_GAIN);

        // --- foot plant: hold strength, released when the objective ends ---
        let plant_strength = objectives
            .foot_plant
            .map(|_| phase.weight(MuscleGroup::LeftLeg).max(phase.weight(MuscleGroup::LeftAnkle)))
            .unwrap_or(0.0);

        // --- recovery: damping rises as the authored drive falls ---
        let recovery_damping = style.muscle_damping.max(0.0) * (1.0 - motor);

        VirtualMuscleCommand {
            support_mode: phase.support(),
            center_of_mass,
            support_target,
            group_weight,
            group_max_torque,
            plant_strength,
            balance_correction,
            upright_torque,
            recovery_damping,
            strike_impulse: objectives.ball_impulse,
        }
    }
}

/// The mean of `points` (`ZERO` for an empty slice) — the deterministic CoM.
fn mean(points: &[Vec3]) -> Vec3 {
    points
        .iter()
        .fold(Vec3::ZERO, |acc, &p| acc.add(p))
        .mul_scalar(1.0 / (points.len().max(1) as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body<'a>(samples: &'a [Vec3], left: Vec3, right: Vec3) -> MuscleBodyState<'a> {
        MuscleBodyState { com_samples: samples, left_foot: left, right_foot: right, pelvis: Transform::IDENTITY }
    }

    fn phase(support: SupportMode, w: f32) -> MusclePhaseProfile {
        MusclePhaseProfile::new(support, [w; MUSCLE_GROUP_COUNT])
    }

    #[test]
    fn command_is_deterministic_and_reports() {
        let profile = VirtualMuscleProfile::default_profile();
        let objs = MuscleObjectives { foot_plant: Some(Vec3::new(0.25, 0.0, -0.1)), motor_drive: 0.5, ball_impulse: None };
        let samples = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 0.0)];
        let b = body(&samples, Vec3::new(0.2, 0.0, 0.0), Vec3::new(-0.2, 0.0, 0.0));
        let a = VirtualMuscleController::command(&profile, MuscleStyle::default_style(), phase(SupportMode::LeftFoot, 0.6), objs, b);
        let c = VirtualMuscleController::command(&profile, MuscleStyle::default_style(), phase(SupportMode::LeftFoot, 0.6), objs, b);
        assert_eq!(a, c);
        assert!(a.report().contains("support=1"));
        assert!(a.report().contains("core="));
    }

    #[test]
    fn center_of_mass_is_the_mean_and_stable() {
        let profile = VirtualMuscleProfile::default_profile();
        let objs = MuscleObjectives { foot_plant: None, motor_drive: 0.0, ball_impulse: None };
        let samples = [Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 0.0, 0.0)];
        let b = body(&samples, Vec3::ZERO, Vec3::ZERO);
        let cmd = VirtualMuscleController::command(&profile, MuscleStyle::default_style(), phase(SupportMode::BothFeet, 0.0), objs, b);
        assert_eq!(cmd.center_of_mass(), Vec3::new(0.0, 1.0, 0.0));
        // Empty samples fold to ZERO (no divide-by-zero).
        let empty: [Vec3; 0] = [];
        let cmd0 = VirtualMuscleController::command(&profile, MuscleStyle::default_style(), phase(SupportMode::BothFeet, 0.0), objs, body(&empty, Vec3::ZERO, Vec3::ZERO));
        assert_eq!(cmd0.center_of_mass(), Vec3::ZERO);
    }

    #[test]
    fn each_support_mode_selects_its_target() {
        let profile = VirtualMuscleProfile::default_profile();
        let objs = MuscleObjectives { foot_plant: None, motor_drive: 0.0, ball_impulse: None };
        let samples = [Vec3::new(1.0, 0.0, 1.0)];
        let (left, right) = (Vec3::new(0.2, 0.0, 0.0), Vec3::new(-0.4, 0.0, 0.0));
        let b = body(&samples, left, right);
        let target = |m| VirtualMuscleController::command(&profile, MuscleStyle::default_style(), phase(m, 0.0), objs, b).support_target();
        assert_eq!(target(SupportMode::BothFeet), left.add(right).mul_scalar(0.5));
        assert_eq!(target(SupportMode::LeftFoot), left);
        assert_eq!(target(SupportMode::RightFoot), right);
        // Airborne falls back to the CoM → zero balance correction.
        let air = VirtualMuscleController::command(&profile, MuscleStyle::default_style(), phase(SupportMode::Airborne, 0.0), objs, b);
        assert_eq!(air.support_target(), air.center_of_mass());
        assert_eq!(air.balance_correction(), Vec3::ZERO);
    }

    #[test]
    fn rest_posture_stabilizes_when_idle_and_fades_under_drive() {
        let profile = VirtualMuscleProfile::default_profile();
        let samples = [Vec3::ZERO];
        let b = body(&samples, Vec3::ZERO, Vec3::ZERO);
        let idle = VirtualMuscleController::command(
            &profile,
            MuscleStyle::default_style(),
            phase(SupportMode::BothFeet, 0.0),
            MuscleObjectives { foot_plant: None, motor_drive: 0.0, ball_impulse: None },
            b,
        );
        // Idle (zero authored weight, zero drive) still emits rest-posture weight.
        assert!(idle.group_weight(MuscleGroup::Pelvis) > 0.0);
        assert!(idle.group_weight(MuscleGroup::Core) > 0.0);
        // Full drive collapses the rest contribution (authored weight 0 → weight 0).
        let driven = VirtualMuscleController::command(
            &profile,
            MuscleStyle::default_style(),
            phase(SupportMode::BothFeet, 0.0),
            MuscleObjectives { foot_plant: None, motor_drive: 1.0, ball_impulse: None },
            b,
        );
        assert!(driven.group_weight(MuscleGroup::Core) < idle.group_weight(MuscleGroup::Core));
    }

    #[test]
    fn muscle_strength_scales_torque_and_balance_strength_scales_correction() {
        let profile = VirtualMuscleProfile::default_profile();
        let objs = MuscleObjectives { foot_plant: None, motor_drive: 0.5, ball_impulse: None };
        let samples = [Vec3::new(0.5, 0.0, 0.5)];
        let b = body(&samples, Vec3::new(0.2, 0.0, 0.0), Vec3::new(0.2, 0.0, 0.0));
        let p = phase(SupportMode::LeftFoot, 0.8);
        let torque = |m| VirtualMuscleController::command(&profile, MuscleStyle::new(m, 1.0, 1.0), p, objs, b).group_max_torque(MuscleGroup::RightLeg);
        assert!(torque(2.0) > torque(1.0), "muscle_strength scales max_torque");
        let corr = |bal| VirtualMuscleController::command(&profile, MuscleStyle::new(1.0, 1.0, bal), p, objs, b).balance_correction().length();
        assert!(corr(2.0) > corr(1.0), "balance_strength scales the correction");
    }

    #[test]
    fn plant_strength_present_only_with_a_plant_objective_and_recovery_tracks_damping() {
        let profile = VirtualMuscleProfile::default_profile();
        let samples = [Vec3::ZERO];
        let b = body(&samples, Vec3::ZERO, Vec3::ZERO);
        let p = phase(SupportMode::LeftFoot, 0.7);
        let planted = VirtualMuscleController::command(&profile, MuscleStyle::default_style(), p, MuscleObjectives { foot_plant: Some(Vec3::ZERO), motor_drive: 0.5, ball_impulse: None }, b);
        let released = VirtualMuscleController::command(&profile, MuscleStyle::default_style(), p, MuscleObjectives { foot_plant: None, motor_drive: 0.5, ball_impulse: None }, b);
        assert!(planted.plant_strength() > 0.0);
        assert_eq!(released.plant_strength(), 0.0);
        // Recovery damping rises as drive falls and scales with muscle_damping.
        let low_drive = VirtualMuscleController::command(&profile, MuscleStyle::new(1.0, 2.0, 1.0), phase(SupportMode::BothFeet, 0.0), MuscleObjectives { foot_plant: None, motor_drive: 0.1, ball_impulse: None }, b);
        let high_drive = VirtualMuscleController::command(&profile, MuscleStyle::new(1.0, 2.0, 1.0), phase(SupportMode::BothFeet, 0.0), MuscleObjectives { foot_plant: None, motor_drive: 0.9, ball_impulse: None }, b);
        assert!(low_drive.recovery_damping() > high_drive.recovery_damping());
    }

    #[test]
    fn strike_impulse_reports_its_magnitude_and_upright_torque_is_zero_when_upright() {
        let profile = VirtualMuscleProfile::default_profile();
        let samples = [Vec3::ZERO];
        let b = body(&samples, Vec3::ZERO, Vec3::ZERO);
        let cmd = VirtualMuscleController::command(
            &profile,
            MuscleStyle::default_style(),
            phase(SupportMode::LeftFoot, 1.0),
            MuscleObjectives { foot_plant: Some(Vec3::ZERO), motor_drive: 1.0, ball_impulse: Some(Vec3::new(0.0, 0.0, 5.0)) },
            b,
        );
        // The report carries the strike magnitude (|(0,0,5)| = 5).
        assert!(cmd.report().contains("strike=5.000"));
        // A no-strike command reports zero.
        let no_strike = VirtualMuscleController::command(
            &profile,
            MuscleStyle::default_style(),
            phase(SupportMode::LeftFoot, 1.0),
            MuscleObjectives { foot_plant: None, motor_drive: 1.0, ball_impulse: None },
            b,
        );
        assert!(no_strike.report().contains("strike=0.000"));
        // An already-upright pelvis needs no upright torque.
        assert_eq!(cmd.upright_torque(), Vec3::ZERO);
    }
}
