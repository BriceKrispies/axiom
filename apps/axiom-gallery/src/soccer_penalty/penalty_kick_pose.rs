//! The **default authored / kinematic penalty-kick pose source**.
//!
//! This is the stable pose pipeline the game's kicker flows through:
//!
//! ```text
//! SoccerPenaltyKickMotionSpec (authored, compiled)
//!   → AnimationAuthoringApi::sample(plan, tick)   (deterministic forward kinematics)
//!   → frame_joint_world(joint)  for each of the 13 kicker joints
//!   → captured KinematicKickFrame per tick
//!   → the visible kicker boxes (penalty_kicker)
//! ```
//!
//! Every joint transform is the authored pose evaluated by pure forward kinematics —
//! there is **no physics body** in the kicker's render path. That is the fix for the
//! broken kicker: the previous physics-backed path (`penalty_physics_kick`, now behind
//! the `experimental_physical_humanoid_kicker` feature) posed the figure's root box
//! from a *dynamic* pelvis body that free-integrated under force + an uncontrolled
//! `apply_torque`, so the pelvis drifted and tumbled away from the kinematically-driven
//! limbs — the inverted body and the orphan capsule. Reading all 13 joints straight
//! from the authored pose keeps the whole figure coherent, upright, and readable.
//!
//! The ball is still a real `axiom-physics` projectile (`penalty_ball`) launched by a
//! real impulse at the strike; the authored motion emits the `ball_contact` event at
//! [`STRIKE_CONTACT_TICK`] that bridges the strike to that ball launch.
//!
//! The whole kick is deterministic and pose-independent of the aim, so it is simulated
//! once and cached; the game samples it by tick.

use axiom_kernel::Tick;
use axiom_math::{Transform, Vec3};

use crate::soccer_penalty::penalty_ball::{
    flight_ticks, penalty_spot, world_target, PenaltyBallTrajectory,
};
use crate::soccer_penalty::penalty_kick_motion::{
    SoccerPenaltyKickMotionSpec, SoccerPenaltyKickStyle, DURATION, PHASE_NAMES, PHASE_SAMPLE_TICKS,
    STRIKE_CONTACT_TICK,
};

/// The 13 kicker joints whose authored world transforms drive the visible boxes, in
/// `penalty_kicker::KICKER_LABELS` order. Joint `i` poses figure part `i`. Every name
/// is a real joint in the standard humanoid rig.
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

/// Joint indices into [`KICKER_JOINTS`] / a frame's `joints`, named for readability.
const PELVIS: usize = 0;
const HEAD: usize = 2;
const LEFT_FOOT: usize = 5;
const RIGHT_FOOT: usize = 8;

/// One captured tick of the authored/kinematic kick.
#[derive(Debug, Clone)]
pub struct KinematicKickFrame {
    /// The authored tick.
    pub tick: u64,
    /// The active authored phase name.
    pub phase: Option<String>,
    /// Each kicker joint's authored world transform, in [`KICKER_JOINTS`] order.
    pub joints: [Transform; 13],
    /// The character root's authored world position (the FK origin at the feet).
    pub root: Vec3,
    /// The left-foot-sole effector world position (the plant contact point).
    pub left_foot_sole: Vec3,
    /// The right-foot-instep effector world position (the striking surface).
    pub right_foot_instep: Vec3,
    /// Whether the authored `ball_contact` event fires this tick.
    pub strike: bool,
    /// The strike direction (ball → net target), unit length, only at the strike.
    pub strike_dir: Option<Vec3>,
    /// The authored strike power `[0, 1]` (from the `ball_contact` event).
    pub strike_power: f32,
}

impl KinematicKickFrame {
    /// The pelvis world position (joint index 0) — the figure's root box.
    pub fn pelvis(&self) -> Vec3 {
        self.joints[PELVIS].translation
    }

    /// The head world position (joint index 2).
    pub fn head(&self) -> Vec3 {
        self.joints[HEAD].translation
    }

    /// The left-foot body world position (joint index 5) — the plant foot.
    pub fn left_foot(&self) -> Vec3 {
        self.joints[LEFT_FOOT].translation
    }

    /// The right-foot body world position (joint index 8) — the kicking foot.
    pub fn right_foot(&self) -> Vec3 {
        self.joints[RIGHT_FOOT].translation
    }

    /// Whether every joint / effector transform in the frame is finite (no NaN/inf).
    fn is_finite(&self) -> bool {
        let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        self.joints.iter().all(|t| finite(t.translation))
            && finite(self.root)
            && finite(self.left_foot_sole)
            && finite(self.right_foot_instep)
    }
}

/// A deterministic, cached authored/kinematic simulation of the whole kick.
#[derive(Debug)]
pub struct SoccerPenaltyKickPose {
    frames: Vec<KinematicKickFrame>,
    style: SoccerPenaltyKickStyle,
}

impl SoccerPenaltyKickPose {
    /// Author the kick for `style` and evaluate it kinematically per tick, caching
    /// every frame. Deterministic: identical `style` → identical frames. No physics.
    pub fn simulate(style: SoccerPenaltyKickStyle) -> Self {
        let spec = SoccerPenaltyKickMotionSpec::author(style);
        let authoring = spec.authoring();
        let plan = spec.plan();

        // Resolve the 13 kicker joint ids once (all exist in the standard rig).
        let joint_ids: Vec<_> = KICKER_JOINTS
            .iter()
            .map(|name| authoring.plan_joint_id(plan, name).ok().flatten())
            .collect();
        // The left-sole / right-instep effectors, for the plant + strike checks.
        let left_sole = authoring.plan_effector_id(plan, "left_foot_sole").ok().flatten();
        let right_instep = authoring.plan_effector_id(plan, "right_foot_instep").ok().flatten();
        // The strike direction is ball → net target (both authored, tick-independent).
        let ball_target = authoring.plan_target_position(plan, "ball").ok().flatten().unwrap_or(Vec3::ZERO);
        let net_target = authoring.plan_target_position(plan, "net_center").ok().flatten().unwrap_or(Vec3::ZERO);
        let to_net = net_target.subtract(ball_target);
        let strike_dir = (to_net.length() > 1.0e-6).then(|| to_net.mul_scalar(1.0 / to_net.length()));

        let frames = (0..DURATION)
            .map(|t| {
                let tick = Tick::new(t);
                // The pose frame type is un-nameable, so all pose reads stay inline.
                let pose = authoring.sample(plan, tick).expect("authored kick samples");
                let mut joints = [Transform::IDENTITY; 13];
                for (i, id) in joint_ids.iter().enumerate() {
                    joints[i] = id.and_then(|j| authoring.frame_joint_world(&pose, j)).unwrap_or(Transform::IDENTITY);
                }
                let root = authoring.frame_root(&pose).translation;
                let left_foot_sole = left_sole
                    .and_then(|e| authoring.frame_effector_world(&pose, e))
                    .map(|t| t.translation)
                    .unwrap_or(joints[LEFT_FOOT].translation);
                let right_foot_instep = right_instep
                    .and_then(|e| authoring.frame_effector_world(&pose, e))
                    .map(|t| t.translation)
                    .unwrap_or(joints[RIGHT_FOOT].translation);
                let events = authoring.frame_event_names(&pose);
                let strike = events.iter().any(|e| e == "ball_contact");
                let strike_power = authoring.frame_ball_contact(&pose).map(|(_, _, _, p)| p.get()).unwrap_or(0.0);
                KinematicKickFrame {
                    tick: t,
                    phase: authoring.active_phase_name(plan, tick).ok().flatten(),
                    joints,
                    root,
                    left_foot_sole,
                    right_foot_instep,
                    strike,
                    strike_dir: strike.then_some(strike_dir).flatten(),
                    strike_power,
                }
            })
            .collect();
        Self { frames, style }
    }

    /// The default-style kick.
    pub fn default_kick() -> Self {
        Self::simulate(SoccerPenaltyKickStyle::default_style())
    }

    /// A kick evaluated from a game power meter value (`0..=100`).
    pub fn from_power_kick(power: i32) -> Self {
        Self::simulate(SoccerPenaltyKickStyle::from_power(power))
    }

    /// The captured frame at `tick` (clamped into range).
    pub fn frame(&self, tick: u64) -> &KinematicKickFrame {
        let i = (tick as usize).min(self.frames.len().saturating_sub(1));
        &self.frames[i]
    }

    /// The total authored tick count.
    pub fn duration(&self) -> u64 {
        DURATION
    }

    /// The tick the ball is struck.
    pub fn strike_tick(&self) -> u64 {
        STRIKE_CONTACT_TICK
    }

    /// The style this kick was evaluated with.
    pub fn style(&self) -> SoccerPenaltyKickStyle {
        self.style
    }

    /// Validate the authored pose at every sampled phase tick. Returns the first
    /// violated invariant, or `Ok(())` when the whole kick is structurally sane.
    ///
    /// Checks: all transforms finite; root at/above the ground; head above the
    /// pelvis; both feet below the pelvis (the kicking foot is allowed to rise during
    /// `strike`/`follow_through`); the left/right limbs stay on their own side (except
    /// the intentional cross during `follow_through`); the left sole reaches the plant
    /// during `plant`; the right instep approaches the ball during `strike`.
    pub fn validate(&self) -> Result<(), String> {
        for &t in PHASE_SAMPLE_TICKS.iter() {
            let f = self.frame(t);
            let phase = f.phase.clone().unwrap_or_default();
            if !f.is_finite() {
                return Err(format!("non-finite transform at tick {t} ({phase})"));
            }
            if f.root.y < -1.0e-3 {
                return Err(format!("root below ground at tick {t} ({phase}): y={}", f.root.y));
            }
            if f.head().y <= f.pelvis().y {
                return Err(format!("head not above pelvis at tick {t} ({phase})"));
            }
            // The left (plant) foot is always below the pelvis; the right (kicking)
            // foot may swing up through the strike + follow-through.
            if f.left_foot().y >= f.pelvis().y {
                return Err(format!("left foot not below pelvis at tick {t} ({phase})"));
            }
            let kicking_foot_may_rise = phase == "strike" || phase == "follow_through";
            if !kicking_foot_may_rise && f.right_foot().y >= f.pelvis().y {
                return Err(format!("right foot not below pelvis at tick {t} ({phase})"));
            }
            // +X is the character's left. Legs keep their side except the follow-through
            // cross-body swing.
            if phase != "follow_through" && f.left_foot().x <= f.right_foot().x {
                return Err(format!("legs crossed off-plan at tick {t} ({phase})"));
            }
        }
        // The plant places the left sole beside the ball; the strike puts the right
        // instep on the ball. Both are authored effector goals, checked at their phase.
        let plant = self.frame(PHASE_SAMPLE_TICKS[3]); // "plant"
        // The authored `left_plant_spot` target (see penalty_kick_motion).
        if plant.left_foot_sole.distance(Vec3::new(0.28, 0.0, -0.12)) > 0.9 {
            return Err("left sole does not reach the plant spot during plant".to_string());
        }
        let strike = self.frame(STRIKE_CONTACT_TICK);
        if strike.right_foot_instep.distance(Vec3::ZERO) > 0.9 {
            return Err("right instep does not approach the ball during strike".to_string());
        }
        Ok(())
    }

    /// A deterministic fixed-tick debug snapshot: one entry per phase at its
    /// representative tick, pairing the authored kicker pose with a representative
    /// centred ball flight so each entry also reports the ball state, the strike
    /// event, and whether the ball impulse has been applied. For inspecting the whole
    /// kick without a renderer.
    pub fn debug_snapshot(&self) -> Vec<KinematicKickPhaseSnapshot> {
        // A representative centred shot: the ball rests on the spot until the strike,
        // then flies the physics trajectory to a mid-height net target.
        let power = (self.style.power.clamp(0.0, 1.0) * 100.0) as i32;
        let trajectory = PenaltyBallTrajectory::to_target(penalty_spot(), world_target(0, 45), flight_ticks(power));
        PHASE_SAMPLE_TICKS
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let f = self.frame(t);
                // Ball state: at rest before the strike, on the flown trajectory after.
                let struck = t >= STRIKE_CONTACT_TICK;
                let flight_elapsed = t.saturating_sub(STRIKE_CONTACT_TICK) as u32;
                let ball_position =
                    if struck { trajectory.position_at(flight_elapsed) } else { penalty_spot() };
                let prev = if struck && flight_elapsed > 0 {
                    trajectory.position_at(flight_elapsed - 1)
                } else {
                    ball_position
                };
                let ball_velocity = ball_position.subtract(prev);
                KinematicKickPhaseSnapshot {
                    phase: PHASE_NAMES[i],
                    tick: t,
                    active_phase: f.phase.clone(),
                    root: f.root,
                    pelvis: f.pelvis(),
                    head: f.head(),
                    left_foot: f.left_foot(),
                    right_foot: f.right_foot(),
                    right_instep: f.right_foot_instep,
                    ball_position,
                    ball_velocity,
                    ball_contact: f.strike,
                    ball_impulse_applied: struck,
                }
            })
            .collect()
    }
}

/// One phase's deterministic fixed-tick read (see [`SoccerPenaltyKickPose::debug_snapshot`]).
#[derive(Debug, Clone, PartialEq)]
pub struct KinematicKickPhaseSnapshot {
    pub phase: &'static str,
    pub tick: u64,
    /// The phase the sampler actually reports at this tick (should equal `phase`).
    pub active_phase: Option<String>,
    pub root: Vec3,
    pub pelvis: Vec3,
    pub head: Vec3,
    pub left_foot: Vec3,
    pub right_foot: Vec3,
    pub right_instep: Vec3,
    pub ball_position: Vec3,
    pub ball_velocity: Vec3,
    pub ball_contact: bool,
    pub ball_impulse_applied: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kick_evaluates_all_phases_in_order_kinematically() {
        let kick = SoccerPenaltyKickPose::default_kick();
        let snap = kick.debug_snapshot();
        assert_eq!(snap.len(), 9);
        for (i, s) in snap.iter().enumerate() {
            assert_eq!(s.active_phase.as_deref(), Some(PHASE_NAMES[i]));
        }
    }

    #[test]
    fn the_pose_is_structurally_valid_across_every_phase() {
        SoccerPenaltyKickPose::default_kick().validate().expect("kinematic kick pose is valid");
    }

    #[test]
    fn exactly_one_strike_fires_and_carries_a_net_ward_direction() {
        let kick = SoccerPenaltyKickPose::default_kick();
        let strikes: Vec<u64> = (0..kick.duration()).filter(|&t| kick.frame(t).strike).collect();
        assert_eq!(strikes, vec![STRIKE_CONTACT_TICK], "exactly one ball_contact");
        let strike = kick.frame(STRIKE_CONTACT_TICK);
        let dir = strike.strike_dir.expect("strike carries a direction");
        assert!(dir.z > 0.0, "strike drives toward the net (+Z): {dir:?}");
        assert!(strike.strike_power > 0.0);
    }

    #[test]
    fn identical_styles_produce_identical_snapshots() {
        let a = SoccerPenaltyKickPose::from_power_kick(70);
        let b = SoccerPenaltyKickPose::from_power_kick(70);
        assert_eq!(a.debug_snapshot(), b.debug_snapshot());
    }
}
