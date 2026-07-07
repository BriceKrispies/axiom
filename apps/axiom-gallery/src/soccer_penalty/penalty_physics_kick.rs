//! The **physics-backed penalty-kick controller**.
//!
//! This is the runtime bridge the game's kicker now flows through:
//!
//! ```text
//! SoccerPenaltyKickMotionSpec (authored, compiled)
//!   → PhysicalAnimationApi (axiom-physical-animation, over real axiom-physics)
//!   → advance() one fixed tick at a time
//!   → PhysicalAnimationFrame  (physics body transforms + ball state + objectives)
//!   → captured PhysicalKickFrame per tick
//!   → the visible kicker boxes (penalty_kicker) + phase/strike readouts
//! ```
//!
//! The whole 74-tick kick is deterministic and **pose-independent of the aim**, so
//! it is simulated **once** and cached; the game samples it by tick. The kicker's
//! limbs are driven kinematically from the authored pose and the pelvis is a real
//! dynamic axiom-physics body (force-driven through the run-up); the bridge's ball
//! receives a **real impulse** at the strike — proof the strike is physical. The
//! *game* ball (aimed at the player's chosen corner) is launched as its own
//! axiom-physics projectile in [`crate::soccer_penalty::penalty_ball`]; both are
//! driven through `axiom-physics`, never teleported.

use axiom_kernel::Tick;
use axiom_math::{Transform, Vec3};
use axiom_physical_animation::PhysicalAnimationApi;

use crate::soccer_penalty::penalty_kick_motion::{
    SoccerPenaltyKickMotionSpec, SoccerPenaltyKickStyle, DURATION, PHASE_NAMES, PHASE_SAMPLE_TICKS,
    STRIKE_CONTACT_TICK,
};

/// The 13 kicker joints whose physics body transforms drive the visible boxes, in
/// `penalty_kicker::KICKER_LABELS` order. Each maps 1-1 to a standard-humanoid
/// joint the bridge binds a body to.
pub const KICKER_JOINTS: [&str; 13] = [
    "pelvis",
    "chest",
    "head",
    "left_thigh",
    "left_shin",
    "left_foot",
    "right_thigh",
    "right_shin",
    "right_foot",
    "left_upper_arm",
    "left_forearm",
    "right_upper_arm",
    "right_forearm",
];

/// One captured tick of the physics-backed kick.
#[derive(Debug, Clone)]
pub struct PhysicalKickFrame {
    /// The authored tick.
    pub tick: u64,
    /// The active authored phase name.
    pub phase: Option<String>,
    /// Each bound joint's physics body world transform, in [`KICKER_JOINTS`] order.
    pub joints: [Transform; 13],
    /// The bridge ball's physics position after this step.
    pub ball_position: Vec3,
    /// The bridge ball's physics linear velocity after this step.
    pub ball_velocity: Vec3,
    /// The active root-velocity objective (the sprint drive), if any.
    pub root_velocity: Option<Vec3>,
    /// Whether a left-foot plant objective is active.
    pub foot_plant: bool,
    /// The active phase's motor drive (its layer weight).
    pub motor_drive: f32,
    /// The physics impulse applied to the bridge ball this tick, if any.
    pub ball_impulse: Option<Vec3>,
    /// Whether the `ball_contact` event fired this tick.
    pub strike: bool,
    /// The virtual-muscle support mode this tick (`0` both, `1` left, `2` right).
    pub support_mode: u8,
    /// The deterministic centre-of-mass estimate.
    pub center_of_mass: Vec3,
    /// The support target the balance controller pulled the CoM toward.
    pub support_target: Vec3,
    /// The balance-correction force applied to the pelvis.
    pub balance_correction: Vec3,
    /// The foot-plant hold strength (drops to `0` once the plant releases).
    pub plant_strength: f32,
    /// The recovery / settling damping factor.
    pub recovery_damping: f32,
    /// The final per-muscle-group actuation weight (group-code order).
    pub group_weights: [f32; 10],
    /// The per-muscle-group peak actuation (scaled by muscle strength).
    pub group_max_torque: [f32; 10],
}

impl PhysicalKickFrame {
    /// The right-foot body world position (joint index 8) — the kicking foot.
    pub fn right_foot(&self) -> Vec3 {
        self.joints[8].translation
    }

    /// The left-foot body world position (joint index 5) — the plant foot.
    pub fn left_foot(&self) -> Vec3 {
        self.joints[5].translation
    }

    /// The pelvis body world position (joint index 0) — the physics-driven root.
    pub fn pelvis(&self) -> Vec3 {
        self.joints[0].translation
    }

    /// The final actuation weight for muscle group `group` (code `0..=9`).
    pub fn group_weight(&self, group: usize) -> f32 {
        self.group_weights[group.min(9)]
    }

    /// The peak actuation for muscle group `group` (code `0..=9`).
    pub fn group_max_torque(&self, group: usize) -> f32 {
        self.group_max_torque[group.min(9)]
    }
}

/// A deterministic, cached physics-backed simulation of the whole kick.
#[derive(Debug)]
pub struct PenaltyPhysicsKick {
    frames: Vec<PhysicalKickFrame>,
    style: SoccerPenaltyKickStyle,
}

impl PenaltyPhysicsKick {
    /// Simulate the whole authored kick through the physics bridge — with the
    /// **virtual-muscle controller active** — and cache every tick. The muscle
    /// controller keeps the kicker balanced over its support each phase.
    /// Deterministic: identical `style` → identical frames.
    pub fn simulate(style: SoccerPenaltyKickStyle) -> Self {
        use crate::soccer_penalty::penalty_muscle;

        let spec = SoccerPenaltyKickMotionSpec::author(style);
        let mut sim = PhysicalAnimationApi::new();
        sim.bind_standard_humanoid(spec.authoring(), spec.plan()).expect("bind humanoid");
        sim.attach_ball(spec.authoring(), spec.plan()).expect("attach ball");
        // Configure the muscle profile + style from the kick style (soccer policy).
        let params = penalty_muscle::muscle_profile_params(style);
        sim.set_muscle_profile(params);
        let (strength, damping, balance) = penalty_muscle::muscle_style(style);
        sim.set_muscle_style(strength, damping, balance);

        let frames = (0..DURATION)
            .map(|t| {
                // Pick the phase's muscle policy up front, then advance under it.
                let phase = spec.authoring().active_phase_name(spec.plan(), Tick::new(t)).ok().flatten().unwrap_or_default();
                let policy = penalty_muscle::phase_profile_for(&phase);
                let frame = sim
                    .advance_muscled(
                        spec.authoring(),
                        spec.plan(),
                        Tick::new(t),
                        policy.support_mode,
                        penalty_muscle::phase_weights_ratio(&phase),
                    )
                    .expect("advance kick");
                let mut joints = [Transform::IDENTITY; 13];
                for (i, name) in KICKER_JOINTS.iter().enumerate() {
                    joints[i] = sim.frame_body_transform(&frame, name).unwrap_or(Transform::IDENTITY);
                }
                let events = sim.frame_event_names(&frame);
                let mut group_weights = [0.0; 10];
                let mut group_max_torque = [0.0; 10];
                for g in 0..10 {
                    group_weights[g] = sim.frame_muscle_group_weight(&frame, g as u8).map(|r| r.get()).unwrap_or(0.0);
                    group_max_torque[g] = sim.frame_muscle_group_max_torque(&frame, g as u8).map(|r| r.get()).unwrap_or(0.0);
                }
                PhysicalKickFrame {
                    tick: t,
                    phase: sim.frame_phase_name(&frame),
                    joints,
                    ball_position: sim.frame_ball_transform(&frame).map(|tf| tf.translation).unwrap_or(Vec3::ZERO),
                    ball_velocity: sim.frame_ball_velocity(&frame).unwrap_or(Vec3::ZERO),
                    root_velocity: sim.frame_root_velocity(&frame),
                    foot_plant: sim.frame_foot_plant(&frame).is_some(),
                    motor_drive: sim.frame_motor_drive(&frame).get(),
                    ball_impulse: sim.frame_ball_impulse(&frame).map(|(dir, mag)| dir.mul_scalar(mag.get())),
                    strike: events.iter().any(|e| e == "ball_contact"),
                    support_mode: sim.frame_support_mode(&frame).unwrap_or(0),
                    center_of_mass: sim.frame_center_of_mass(&frame).unwrap_or(Vec3::ZERO),
                    support_target: sim.frame_support_target(&frame).unwrap_or(Vec3::ZERO),
                    balance_correction: sim.frame_balance_correction(&frame).unwrap_or(Vec3::ZERO),
                    plant_strength: sim.frame_plant_strength(&frame).map(|r| r.get()).unwrap_or(0.0),
                    recovery_damping: sim.frame_recovery_damping(&frame).map(|r| r.get()).unwrap_or(0.0),
                    group_weights,
                    group_max_torque,
                }
            })
            .collect();
        Self { frames, style }
    }

    /// The default-style kick.
    pub fn default_kick() -> Self {
        Self::simulate(SoccerPenaltyKickStyle::default_style())
    }

    /// A kick simulated from a game power meter value (`0..=100`).
    pub fn from_power_kick(power: i32) -> Self {
        Self::simulate(SoccerPenaltyKickStyle::from_power(power))
    }

    /// The captured frame at `tick` (clamped into range).
    pub fn frame(&self, tick: u64) -> &PhysicalKickFrame {
        let i = (tick as usize).min(self.frames.len().saturating_sub(1));
        &self.frames[i]
    }

    /// The 13 joint world transforms at a **fractional** tick, interpolated between
    /// the bracketing captured frames (matching `SoccerPenaltyKickPose::joints_at`, so
    /// [`crate::soccer_penalty::penalty_kicker::KickerRig::boxes_at`] is source-agnostic).
    pub fn joints_at(&self, t: f32) -> [axiom_math::Transform; 13] {
        let max = self.frames.len().saturating_sub(1);
        let t = t.max(0.0);
        let i = (t.floor() as usize).min(max);
        let j = (i + 1).min(max);
        let f = t - (i as f32);
        let a = &self.frames[i].joints;
        let b = &self.frames[j].joints;
        let mut out = [axiom_math::Transform::IDENTITY; 13];
        for k in 0..13 {
            let pos = a[k].translation.add(b[k].translation.subtract(a[k].translation).mul_scalar(f));
            let rot = a[k].rotation.nlerp(b[k].rotation, f).unwrap_or(a[k].rotation);
            out[k] = axiom_math::Transform::new(pos, rot, a[k].scale);
        }
        out
    }

    /// The total authored tick count.
    pub fn duration(&self) -> u64 {
        DURATION
    }

    /// The tick the ball is struck.
    pub fn strike_tick(&self) -> u64 {
        STRIKE_CONTACT_TICK
    }

    /// The style this kick was simulated with.
    pub fn style(&self) -> SoccerPenaltyKickStyle {
        self.style
    }

    /// The world speed the physics strike imparts to the ball — the physics-derived
    /// "power" the game ball's launch is scaled by (magnitude of the post-strike
    /// bridge-ball velocity). Always positive for a struck ball.
    pub fn strike_launch_speed(&self) -> f32 {
        // The tick just after the strike carries the impulse-imparted velocity.
        self.frame((STRIKE_CONTACT_TICK + 1).min(DURATION - 1)).ball_velocity.length()
    }

    /// A deterministic debug snapshot: one entry per phase at its representative
    /// tick, showing the active phase, the muscle support mode + centre-of-mass +
    /// support target, the major muscle-group weights, the motor drive, the
    /// plant-foot strength, and any strike impulse. For inspecting the kick's
    /// active control without a renderer.
    pub fn debug_snapshot(&self) -> Vec<PhysicalKickPhaseSnapshot> {
        use crate::soccer_penalty::penalty_muscle::{GROUP_CORE, GROUP_LEFT_LEG, GROUP_PELVIS, GROUP_RIGHT_LEG};
        PHASE_SAMPLE_TICKS
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let f = self.frame(t);
                PhysicalKickPhaseSnapshot {
                    phase: PHASE_NAMES[i],
                    tick: t,
                    active_phase: f.phase.clone(),
                    support_mode: f.support_mode,
                    center_of_mass: f.center_of_mass,
                    support_target: f.support_target,
                    motor_drive: f.motor_drive,
                    foot_plant: f.foot_plant,
                    plant_strength: f.plant_strength,
                    recovery_damping: f.recovery_damping,
                    sprinting: f.root_velocity.is_some(),
                    striking: f.strike,
                    strike_impulse: f.ball_impulse,
                    pelvis_weight: f.group_weight(GROUP_PELVIS),
                    core_weight: f.group_weight(GROUP_CORE),
                    left_leg_weight: f.group_weight(GROUP_LEFT_LEG),
                    right_leg_weight: f.group_weight(GROUP_RIGHT_LEG),
                    pelvis: f.pelvis(),
                    right_foot: f.right_foot(),
                    ball_velocity: f.ball_velocity,
                }
            })
            .collect()
    }
}

/// One phase's deterministic debug read (see [`PenaltyPhysicsKick::debug_snapshot`]).
#[derive(Debug, Clone)]
pub struct PhysicalKickPhaseSnapshot {
    pub phase: &'static str,
    pub tick: u64,
    pub active_phase: Option<String>,
    pub support_mode: u8,
    pub center_of_mass: Vec3,
    pub support_target: Vec3,
    pub motor_drive: f32,
    pub foot_plant: bool,
    pub plant_strength: f32,
    pub recovery_damping: f32,
    pub sprinting: bool,
    pub striking: bool,
    pub strike_impulse: Option<Vec3>,
    pub pelvis_weight: f32,
    pub core_weight: f32,
    pub left_leg_weight: f32,
    pub right_leg_weight: f32,
    pub pelvis: Vec3,
    pub right_foot: Vec3,
    pub ball_velocity: Vec3,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kick_simulates_all_phases_in_order_through_physics() {
        let kick = PenaltyPhysicsKick::default_kick();
        let snap = kick.debug_snapshot();
        assert_eq!(snap.len(), 9);
        // Each snapshot's active phase matches the expected phase, in order.
        for s in &snap {
            assert_eq!(s.active_phase.as_deref(), Some(s.phase));
        }
    }

    #[test]
    fn the_sprint_drives_the_pelvis_toward_the_ball_through_physics() {
        let kick = PenaltyPhysicsKick::default_kick();
        // Early sprint vs late sprint: the dynamic pelvis body advances in +Z
        // (toward the ball at the origin) under the approach force.
        let early = kick.frame(6).pelvis().z;
        let late = kick.frame(18).pelvis().z;
        assert!(late > early, "pelvis moves toward the ball: early={early}, late={late}");
        // The sprint frames carry a root-velocity objective.
        assert!(kick.frame(12).root_velocity.is_some());
    }

    #[test]
    fn the_plant_phase_applies_a_left_foot_plant_objective() {
        let kick = PenaltyPhysicsKick::default_kick();
        assert!(kick.frame(32).foot_plant, "plant phase pins the left foot");
        assert!(!kick.frame(6).foot_plant, "the sprint does not");
    }

    #[test]
    fn hip_drive_drives_harder_than_backswing() {
        let kick = PenaltyPhysicsKick::default_kick();
        let backswing = kick.frame(41).motor_drive;
        let hip_drive = kick.frame(49).motor_drive;
        assert!(hip_drive > backswing, "hip_drive={hip_drive} backswing={backswing}");
    }

    #[test]
    fn the_backswing_draws_the_right_foot_behind_and_follow_through_sends_it_past() {
        let kick = PenaltyPhysicsKick::default_kick();
        let back = kick.frame(41).right_foot().z;
        let follow = kick.frame(64).right_foot().z;
        // The kicking foot goes behind the body (−Z) at backswing and past the ball
        // (toward the goal, +Z in the bridge frame) on the follow-through.
        assert!(follow > back, "foot sweeps forward: back={back}, follow={follow}");
    }

    #[test]
    fn exactly_one_strike_fires_and_it_impulses_the_ball_toward_the_net() {
        let kick = PenaltyPhysicsKick::default_kick();
        let strikes: Vec<u64> = (0..kick.duration()).filter(|&t| kick.frame(t).strike).collect();
        assert_eq!(strikes, vec![STRIKE_CONTACT_TICK], "exactly one ball_contact");
        let strike = kick.frame(STRIKE_CONTACT_TICK);
        assert!(strike.ball_impulse.is_some(), "a real impulse is applied");
        // Just after the strike the ball has real velocity toward the net (+Z).
        let vel = kick.frame(STRIKE_CONTACT_TICK + 1).ball_velocity;
        assert!(vel.z > 0.0 && vel.length() > 1.0, "ball flies toward the net: {vel:?}");
        assert!(kick.strike_launch_speed() > 1.0);
    }

    #[test]
    fn recover_settles_softer_than_the_strike() {
        let kick = PenaltyPhysicsKick::default_kick();
        let strike = kick.frame(STRIKE_CONTACT_TICK).motor_drive;
        let recover = kick.frame(71).motor_drive;
        assert!(recover < strike, "recover={recover} strike={strike}");
    }

    #[test]
    fn stronger_power_impulses_the_ball_faster_and_is_deterministic() {
        let soft = PenaltyPhysicsKick::simulate(SoccerPenaltyKickStyle::from_power(20));
        let hard = PenaltyPhysicsKick::simulate(SoccerPenaltyKickStyle::from_power(95));
        assert!(hard.strike_launch_speed() > soft.strike_launch_speed());
        // Determinism: identical style → identical captured ball velocity.
        let a = PenaltyPhysicsKick::from_power_kick(70);
        let b = PenaltyPhysicsKick::from_power_kick(70);
        assert_eq!(a.frame(STRIKE_CONTACT_TICK + 1).ball_velocity, b.frame(STRIKE_CONTACT_TICK + 1).ball_velocity);
    }
}
