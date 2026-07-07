//! The soccer **virtual-muscle policy** — the per-phase configuration the kick
//! feeds the engine's generic `VirtualMuscleController` (in
//! `axiom-physical-animation`).
//!
//! The engine owns the *mechanism* (muscle groups, PD balance, centre-of-mass,
//! foot-plant/recovery math); this module owns the soccer *policy*: which feet
//! carry the body and which muscle groups are emphasized in each of the nine kick
//! phases. It is where the task's `StrikePreparation` / `StrikeDrive` /
//! `FollowThrough` / `Recovery` controllers live — as deterministic data, one row
//! per phase — plus the mapping from [`SoccerPenaltyKickStyle`] onto the engine's
//! muscle profile + style scalars.

use axiom_kernel::Ratio;

use crate::soccer_penalty::penalty_kick_motion::SoccerPenaltyKickStyle;

/// Muscle-group codes (engine group-code order). Readable names for the policy.
pub const GROUP_CORE: usize = 0;
pub const GROUP_PELVIS: usize = 1;
pub const GROUP_SPINE: usize = 2;
pub const GROUP_NECK_HEAD: usize = 3;
pub const GROUP_LEFT_LEG: usize = 4;
pub const GROUP_RIGHT_LEG: usize = 5;
pub const GROUP_LEFT_ANKLE: usize = 6;
pub const GROUP_RIGHT_ANKLE: usize = 7;
pub const GROUP_LEFT_ARM: usize = 8;
pub const GROUP_RIGHT_ARM: usize = 9;
/// The number of muscle groups (matches the engine).
pub const GROUP_COUNT: usize = 10;

/// Support-mode codes (engine order).
pub const SUPPORT_BOTH_FEET: u8 = 0;
pub const SUPPORT_LEFT_FOOT: u8 = 1;
pub const SUPPORT_RIGHT_FOOT: u8 = 2;
pub const SUPPORT_AIRBORNE: u8 = 3;

/// One phase's muscle policy: which feet support the body and the per-group
/// actuation emphasis (group-code order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyPhaseMuscle {
    pub support_mode: u8,
    pub group_weights: [f32; GROUP_COUNT],
}

/// The per-phase muscle policy for the nine kick phases. This is the soccer
/// realization of the strike-preparation / strike-drive / follow-through /
/// recovery controllers: each row emphasizes the groups that phase actuates while
/// keeping the support foot stabilized.
pub fn phase_profile_for(phase: &str) -> PenaltyPhaseMuscle {
    // Order: core, pelvis, spine, neck_head, left_leg, right_leg, left_ankle,
    //        right_ankle, left_arm, right_arm.
    match phase {
        // Stand ready over both feet — gentle postural stabilization.
        "setup" => row(SUPPORT_BOTH_FEET, [0.5, 0.5, 0.4, 0.3, 0.5, 0.5, 0.4, 0.4, 0.3, 0.3]),
        // Run-up: legs drive, arms pump, core braces — dynamic (both-feet) support.
        "sprint_approach" => row(SUPPORT_BOTH_FEET, [0.6, 0.7, 0.5, 0.3, 0.7, 0.7, 0.5, 0.5, 0.6, 0.6]),
        // Final step: lower + prepare the left support side.
        "pre_plant" => row(SUPPORT_BOTH_FEET, [0.6, 0.6, 0.5, 0.3, 0.7, 0.5, 0.6, 0.4, 0.5, 0.5]),
        // Plant: LEFT leg/ankle/core strongly stabilize the support; arms balance.
        "plant" => row(SUPPORT_LEFT_FOOT, [0.8, 0.7, 0.6, 0.4, 0.9, 0.4, 0.9, 0.3, 0.6, 0.6]),
        // Backswing (StrikePreparation): draw the RIGHT leg back while the LEFT
        // foot stays firmly planted; torso/arms counterbalance.
        "backswing" => row(SUPPORT_LEFT_FOOT, [0.7, 0.6, 0.6, 0.4, 0.85, 0.7, 0.85, 0.4, 0.6, 0.6]),
        // Hip drive (StrikeDrive): pelvis + right leg drive HARDER than backswing.
        "hip_drive" => row(SUPPORT_LEFT_FOOT, [0.85, 0.95, 0.7, 0.4, 0.85, 0.9, 0.85, 0.5, 0.6, 0.6]),
        // Strike (StrikeDrive peak): pelvis + right leg/ankle max; left foot stable.
        "strike" => row(SUPPORT_LEFT_FOOT, [0.9, 1.0, 0.75, 0.4, 0.85, 1.0, 0.85, 0.9, 0.6, 0.6]),
        // Follow-through: right leg continues; weight transitions off the plant
        // foot (both-feet support); arms counterbalance.
        "follow_through" => row(SUPPORT_BOTH_FEET, [0.7, 0.7, 0.6, 0.4, 0.5, 0.8, 0.5, 0.6, 0.6, 0.6]),
        // Recover: settle over both feet — low actuation, rest posture restores.
        "recover" => row(SUPPORT_BOTH_FEET, [0.4, 0.4, 0.4, 0.3, 0.4, 0.4, 0.4, 0.4, 0.3, 0.3]),
        // Anything else (no active phase): neutral both-feet stabilization.
        _ => row(SUPPORT_BOTH_FEET, [0.4; GROUP_COUNT]),
    }
}

fn row(support_mode: u8, group_weights: [f32; GROUP_COUNT]) -> PenaltyPhaseMuscle {
    PenaltyPhaseMuscle { support_mode, group_weights }
}

/// The phase's per-group weights as engine `Ratio`s.
pub fn phase_weights_ratio(phase: &str) -> [Ratio; GROUP_COUNT] {
    phase_profile_for(phase).group_weights.map(ratio)
}

/// The engine muscle-style scalars `(muscle_strength, muscle_damping,
/// balance_strength)` for this kick style. These are gains, not weights, so they
/// are left unclamped (may exceed 1).
pub fn muscle_style(style: SoccerPenaltyKickStyle) -> (Ratio, Ratio, Ratio) {
    (ratio_unclamped(style.muscle_strength), ratio_unclamped(style.muscle_damping), ratio_unclamped(style.balance_strength))
}

/// The engine per-group base profile params `(stiffness, damping, max_torque,
/// rest_weight)` in group-code order. The plant-side (left leg + ankle) and the
/// core/pelvis are stiffened by `plant_stability` so the support holds firm.
pub fn muscle_profile_params(style: SoccerPenaltyKickStyle) -> [(Ratio, Ratio, Ratio, Ratio); GROUP_COUNT] {
    let plant = style.plant_stability.clamp(0.0, 1.0);
    // (stiffness, damping, max_torque, rest_weight) base per group.
    let base: [(f32, f32, f32, f32); GROUP_COUNT] = [
        (1.0, 0.5, 1.0, 0.7),                       // core
        (1.0, 0.5, 1.2, 0.7),                       // pelvis
        (0.9, 0.5, 0.9, 0.6),                       // spine
        (0.6, 0.4, 0.4, 0.5),                       // neck_head
        (1.0 + 0.3 * plant, 0.5, 1.2 + 0.4 * plant, 0.6), // left_leg (support)
        (1.0, 0.5, 1.2, 0.5),                       // right_leg (kicking)
        (0.9 + 0.3 * plant, 0.6, 0.7 + 0.3 * plant, 0.5), // left_ankle (support)
        (0.9, 0.6, 0.7, 0.4),                       // right_ankle
        (0.6, 0.4, 0.5, 0.4),                       // left_arm
        (0.6, 0.4, 0.5, 0.4),                       // right_arm
    ];
    base.map(|(s, d, t, w)| (ratio_unclamped(s), ratio(d), ratio_unclamped(t), ratio(w)))
}

fn ratio(v: f32) -> Ratio {
    Ratio::finite_or_zero(v.clamp(0.0, 1.0))
}

/// A finite `Ratio` that may exceed 1 (stiffness / torque can be super-unit).
fn ratio_unclamped(v: f32) -> Ratio {
    Ratio::finite_or_zero(v.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plant_selects_left_foot_and_strengthens_the_support_side() {
        let plant = phase_profile_for("plant");
        assert_eq!(plant.support_mode, SUPPORT_LEFT_FOOT);
        // The left leg / ankle / core are the strongly-actuated groups.
        assert!(plant.group_weights[GROUP_LEFT_LEG] >= 0.85);
        assert!(plant.group_weights[GROUP_LEFT_ANKLE] >= 0.85);
        assert!(plant.group_weights[GROUP_CORE] >= 0.7);
        // The kicking leg is not yet emphasized.
        assert!(plant.group_weights[GROUP_RIGHT_LEG] < plant.group_weights[GROUP_LEFT_LEG]);
    }

    #[test]
    fn backswing_preserves_left_support_while_loading_the_right_leg() {
        let back = phase_profile_for("backswing");
        assert_eq!(back.support_mode, SUPPORT_LEFT_FOOT);
        assert!(back.group_weights[GROUP_LEFT_LEG] >= 0.8, "left foot stays planted");
        assert!(back.group_weights[GROUP_RIGHT_LEG] > phase_profile_for("plant").group_weights[GROUP_RIGHT_LEG]);
    }

    #[test]
    fn hip_drive_drives_pelvis_and_right_leg_harder_than_backswing() {
        let back = phase_profile_for("backswing");
        let hip = phase_profile_for("hip_drive");
        assert_eq!(hip.support_mode, SUPPORT_LEFT_FOOT);
        assert!(hip.group_weights[GROUP_PELVIS] > back.group_weights[GROUP_PELVIS]);
        assert!(hip.group_weights[GROUP_RIGHT_LEG] > back.group_weights[GROUP_RIGHT_LEG]);
    }

    #[test]
    fn follow_through_transitions_off_the_plant_and_recover_settles_over_both_feet() {
        let follow = phase_profile_for("follow_through");
        let recover = phase_profile_for("recover");
        assert_eq!(follow.support_mode, SUPPORT_BOTH_FEET);
        assert_eq!(recover.support_mode, SUPPORT_BOTH_FEET);
        // Recover actuates the least (settling), less than the strike.
        let strike = phase_profile_for("strike");
        assert!(recover.group_weights[GROUP_PELVIS] < strike.group_weights[GROUP_PELVIS]);
        // An unknown phase falls back to neutral both-feet.
        assert_eq!(phase_profile_for("nope").support_mode, SUPPORT_BOTH_FEET);
    }

    #[test]
    fn style_maps_to_engine_muscle_config_deterministically() {
        let mut style = SoccerPenaltyKickStyle::default_style();
        style.muscle_strength = 2.0;
        style.balance_strength = 0.5;
        style.plant_stability = 1.0;
        let (strength, _damp, balance) = muscle_style(style);
        // muscle_strength is a gain, left unclamped (super-unit passes through).
        assert_eq!(strength.get(), 2.0);
        assert_eq!(balance.get(), 0.5);
        // The plant side is stiffened by plant_stability (super-unit stiffness).
        let params = muscle_profile_params(style);
        assert!(params[GROUP_LEFT_LEG].0.get() > params[GROUP_RIGHT_LEG].0.get());
        // Deterministic.
        assert_eq!(muscle_profile_params(style), muscle_profile_params(style));
        assert_eq!(phase_weights_ratio("strike"), phase_weights_ratio("strike"));
    }
}
