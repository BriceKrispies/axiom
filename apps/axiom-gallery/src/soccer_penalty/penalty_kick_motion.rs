//! The authored **`SoccerPenaltyKickMotionSpec`** — the soccer penalty kick as a
//! nine-phase procedural motion, authored entirely through the engine's
//! `AnimationAuthoringApi` vocabulary (targets, phases, root motion, pose goals,
//! constraints, contacts, a ball-contact event, and named style scalars).
//!
//! This replaces the game's old ad-hoc frame-index kicker animation with a real
//! authored motion plan. The plan is compiled once here; the physics-backed
//! controller (`penalty_physics_kick`) then drives it through `axiom-physics`.
//! Authoring lives in the **app** (the composition root) — the generic authoring
//! module holds no soccer concepts.
//!
//! ## The nine phases (in order)
//! `setup → sprint_approach → pre_plant → plant → backswing → hip_drive → strike
//! → follow_through → recover`. Each phase owns a fixed tick range and a set of
//! authored objectives; the `hip_drive` phase deliberately drives harder than
//! `backswing`, and `recover` softest, so the physical drive reads as a real kick.

use axiom_animation_authoring::{AnimationAuthoringApi, MotionId, PlanId};
use axiom_kernel::{Ratio, Tick};
use axiom_math::Vec3;

/// Total authored motion length, in fixed ticks.
pub const DURATION: u64 = 74;
/// The tick the right instep meets the ball and the `ball_contact` event fires.
pub const STRIKE_CONTACT_TICK: u64 = 55;

/// The nine phase tick ranges `[start, end)`, in order.
pub const SETUP: (u64, u64) = (0, 4);
pub const SPRINT_APPROACH: (u64, u64) = (4, 20);
pub const PRE_PLANT: (u64, u64) = (20, 28);
pub const PLANT: (u64, u64) = (28, 36);
pub const BACKSWING: (u64, u64) = (36, 46);
pub const HIP_DRIVE: (u64, u64) = (46, 52);
pub const STRIKE: (u64, u64) = (52, 60);
pub const FOLLOW_THROUGH: (u64, u64) = (60, 68);
pub const RECOVER: (u64, u64) = (68, 74);

/// The nine ordered phase names (for debug/inspection + tests).
pub const PHASE_NAMES: [&str; 9] = [
    "setup",
    "sprint_approach",
    "pre_plant",
    "plant",
    "backswing",
    "hip_drive",
    "strike",
    "follow_through",
    "recover",
];

/// A representative interior tick of each phase, in phase order — the deterministic
/// inspection points for the debug snapshot path.
pub const PHASE_SAMPLE_TICKS: [u64; 9] = [2, 12, 24, 32, 41, 49, STRIKE_CONTACT_TICK, 64, 71];

/// Deterministic style parameters for the kick. Each is a `[0, 1]` scalar that is
/// threaded into the authored magnitudes / phase drive weights / ball impulse, so
/// changing any one changes the motion (and the physics) deterministically.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoccerPenaltyKickStyle {
    /// Strike power — the ball-contact power and (via the controller) the impulse.
    pub power: f32,
    /// How aggressively the hips drive and the run-up presses (drive weights).
    pub urgency: f32,
    /// Run-up distance / burst — a longer, faster approach when higher.
    pub runup_speed: f32,
    /// How firmly the plant foot is held (plant drive + COM emphasis).
    pub plant_stability: f32,
    /// Torso/pelvis counter-rotation and twist-through magnitude.
    pub torso_twist: f32,
    /// How wide the arms spread for balance.
    pub arm_balance: f32,
    /// How far the right leg draws back in the backswing.
    pub backswing_amount: f32,
    /// How far the right leg continues past the ball on the follow-through.
    pub follow_through_amount: f32,
    /// How much the body settles in recovery (higher → calmer settle).
    pub recovery_settle: f32,
    /// Virtual-muscle peak actuation scale — stronger muscles hold pose harder.
    pub muscle_strength: f32,
    /// Virtual-muscle recovery/settling damping scale.
    pub muscle_damping: f32,
    /// Balance-controller correction strength — how hard the pelvis is pulled over
    /// its support.
    pub balance_strength: f32,
}

impl SoccerPenaltyKickStyle {
    /// A balanced default kick.
    pub const fn default_style() -> Self {
        Self {
            power: 0.7,
            urgency: 0.6,
            runup_speed: 0.6,
            plant_stability: 0.7,
            torso_twist: 0.6,
            arm_balance: 0.6,
            backswing_amount: 0.7,
            follow_through_amount: 0.7,
            recovery_settle: 0.6,
            muscle_strength: 1.0,
            muscle_damping: 1.0,
            balance_strength: 1.0,
        }
    }

    /// Derive a style from a game power meter (`0..=100`): power maps directly;
    /// urgency, run-up and follow-through scale gently with it; the rest hold at
    /// their default so the motion stays readable across the power range.
    pub fn from_power(power: i32) -> Self {
        let p = (power.clamp(0, 100) as f32) / 100.0;
        Self {
            power: p,
            urgency: 0.45 + 0.45 * p,
            runup_speed: 0.4 + 0.5 * p,
            follow_through_amount: 0.5 + 0.4 * p,
            ..Self::default_style()
        }
    }

    fn ratio(v: f32) -> Ratio {
        Ratio::finite_or_zero(v.clamp(0.0, 1.0))
    }
}

/// The authored, compiled penalty kick: the authoring api holding the motion plus
/// the compiled `PlanId`. The physics controller borrows the authoring api and the
/// plan to sample objectives per tick.
#[derive(Debug)]
pub struct SoccerPenaltyKickMotionSpec {
    authoring: AnimationAuthoringApi,
    plan: PlanId,
    style: SoccerPenaltyKickStyle,
}

impl SoccerPenaltyKickMotionSpec {
    /// Author and compile the nine-phase kick for `style`.
    pub fn author(style: SoccerPenaltyKickStyle) -> Self {
        let mut authoring = AnimationAuthoringApi::new();
        let motion = build_kick(&mut authoring, style);
        let plan = authoring.compile(motion).expect("penalty kick motion compiles");
        Self { authoring, plan, style }
    }

    /// The authoring facade (for sampling objectives / poses).
    pub fn authoring(&self) -> &AnimationAuthoringApi {
        &self.authoring
    }

    /// The compiled plan id.
    pub fn plan(&self) -> PlanId {
        self.plan
    }

    /// The style this motion was authored with.
    pub fn style(&self) -> SoccerPenaltyKickStyle {
        self.style
    }
}

/// Author the nine-phase kick against a fresh standard humanoid, returning the
/// motion id (ready to compile). Every phase is expressed as authoring data.
fn build_kick(api: &mut AnimationAuthoringApi, style: SoccerPenaltyKickStyle) -> MotionId {
    let rig = api.standard_humanoid();
    let m = api.create_motion("soccer_penalty_kick", Tick::new(DURATION), rig).expect("create motion");

    // --- targets: the geometry the phases aim at --------------------------------
    // The run-up start recedes with run-up speed (a longer burst when quicker).
    let runup = 2.0 + 2.0 * style.runup_speed;
    api.add_target(m, "approach_start", Vec3::new(0.0, 0.0, -runup)).unwrap();
    api.add_target(m, "ball", Vec3::new(0.0, 0.0, 0.0)).unwrap();
    api.add_target(m, "net_center", Vec3::new(0.0, 0.8, 8.0)).unwrap();
    api.add_target(m, "left_plant_spot", Vec3::new(0.28, 0.0, -0.12)).unwrap();
    api.add_target(m, "backswing_target", Vec3::new(0.0, 0.4, -1.6)).unwrap();

    // --- style scalars (all threaded into the motion below) ---------------------
    let s = SoccerPenaltyKickStyle::ratio;
    api.set_style(m, "power", s(style.power)).unwrap();
    api.set_style(m, "urgency", s(style.urgency)).unwrap();
    api.set_style(m, "runup_speed", s(style.runup_speed)).unwrap();
    api.set_style(m, "plant_stability", s(style.plant_stability)).unwrap();
    api.set_style(m, "torso_twist", s(style.torso_twist)).unwrap();
    api.set_style(m, "arm_balance", s(style.arm_balance)).unwrap();
    api.set_style(m, "backswing_amount", s(style.backswing_amount)).unwrap();
    api.set_style(m, "follow_through_amount", s(style.follow_through_amount)).unwrap();
    api.set_style(m, "recovery_settle", s(style.recovery_settle)).unwrap();

    let twist = s(style.torso_twist);
    let counter = s((style.torso_twist * 0.8).clamp(0.0, 1.0));
    let backswing = s(style.backswing_amount);
    let follow = s(style.follow_through_amount);
    let reach = s((style.follow_through_amount * 0.7).clamp(0.0, 1.0));
    let arm = s(style.arm_balance);

    // 1. setup — behind the ball, facing it; ball still. A brief ready hold with
    //    the gaze already on the ball.
    let setup = api.add_phase(m, "setup", tick(SETUP.0), tick(SETUP.1)).unwrap();
    api.set_phase_root_motion_hold(setup).unwrap();
    api.set_phase_layer_weight(setup, s(0.4)).unwrap();
    api.add_keep_gaze_on_target(setup, "ball").unwrap();

    // 2. sprint_approach — a short burst run toward the ball: root drives forward,
    //    torso leans in, arms pump opposite, gaze locked on the ball.
    let sprint = api.add_phase(m, "sprint_approach", tick(SPRINT_APPROACH.0), tick(SPRINT_APPROACH.1)).unwrap();
    api.set_phase_root_motion_move_toward(sprint, "approach_start", "ball").unwrap();
    api.set_phase_ease_smoothstep(sprint).unwrap();
    api.set_phase_layer_weight(sprint, s(0.6 + 0.3 * style.urgency)).unwrap();
    api.add_torso_twist_toward_target(sprint, "ball", s(0.25 * style.torso_twist)).unwrap();
    api.add_move_effector_toward_target(sprint, "right_hand", "approach_start", arm).unwrap();
    api.add_move_effector_toward_target(sprint, "left_hand", "ball", arm).unwrap();
    api.add_keep_gaze_on_target(sprint, "ball").unwrap();

    // 3. pre_plant — the final step shortens, body lowers, pelvis begins turning
    //    into the kick, arms begin spreading, the right leg prepares.
    let pre_plant = api.add_phase(m, "pre_plant", tick(PRE_PLANT.0), tick(PRE_PLANT.1)).unwrap();
    api.set_phase_root_motion_settle(pre_plant).unwrap();
    api.set_phase_ease_out(pre_plant).unwrap();
    api.set_phase_layer_weight(pre_plant, s(0.6)).unwrap();
    api.add_set_joint_rotation(pre_plant, "pelvis", Vec3::new(0.0, -0.2 * style.torso_twist, 0.0)).unwrap();
    api.add_raise_arm_for_balance(pre_plant, true).unwrap();
    api.add_raise_arm_for_balance(pre_plant, false).unwrap();
    api.add_leg_backswing(pre_plant, true, s(0.3 * style.backswing_amount)).unwrap();
    api.add_keep_gaze_on_target(pre_plant, "ball").unwrap();

    // 4. plant — the left foot plants beside the ball and is pinned through the
    //    controller; COM shifts over the support foot; arms widen; gaze on ball.
    let plant = api.add_phase(m, "plant", tick(PLANT.0), tick(PLANT.1)).unwrap();
    api.set_phase_root_motion_hold(plant).unwrap();
    api.set_phase_layer_weight(plant, s(0.65 + 0.2 * style.plant_stability)).unwrap();
    api.add_contact(plant, "left_foot_sole", "left_plant_spot").unwrap();
    api.add_pin_effector_to_target(plant, "left_foot_sole", "left_plant_spot").unwrap();
    api.add_keep_center_of_mass_over_support(plant, "left_foot_sole").unwrap();
    api.add_raise_arm_for_balance(plant, true).unwrap();
    api.add_raise_arm_for_balance(plant, false).unwrap();
    api.add_keep_gaze_on_target(plant, "ball").unwrap();

    // 5. backswing — the right hip extends back, knee bends, foot swings behind on
    //    an arc; chest counter-rotates against the pelvis; arms counterbalance; the
    //    planted foot stays pinned. Drives *less* than hip_drive.
    let back = api.add_phase(m, "backswing", tick(BACKSWING.0), tick(BACKSWING.1)).unwrap();
    api.set_phase_root_motion_hold(back).unwrap();
    api.set_phase_ease_in(back).unwrap();
    api.set_phase_layer_weight(back, s(0.4 + 0.2 * style.backswing_amount)).unwrap();
    api.add_leg_backswing(back, true, backswing).unwrap();
    api.add_aim_effector_at_target(back, "right_foot_instep", "backswing_target").unwrap();
    api.add_set_joint_rotation(back, "pelvis", Vec3::new(0.0, 0.25 * style.torso_twist, 0.0)).unwrap();
    api.add_torso_twist_toward_target(back, "approach_start", counter).unwrap();
    api.add_raise_arm_for_balance(back, true).unwrap();
    api.add_raise_arm_for_balance(back, false).unwrap();
    api.add_preserve_foot_contact(back, "left_foot_sole", "left_plant_spot").unwrap();

    // 6. hip_drive — the pelvis initiates the kick before the lower leg follows:
    //    the right hip drives forward, torso rotates through, knee begins to
    //    extend, foot accelerates. Deliberately the strongest pre-strike drive.
    let hip = api.add_phase(m, "hip_drive", tick(HIP_DRIVE.0), tick(HIP_DRIVE.1)).unwrap();
    api.set_phase_root_motion_hold(hip).unwrap();
    api.set_phase_ease_in(hip).unwrap();
    api.set_phase_layer_weight(hip, s(0.85 + 0.15 * style.urgency)).unwrap();
    api.add_set_joint_rotation(hip, "right_hip", Vec3::new(-0.35 - 0.2 * style.urgency, 0.0, 0.0)).unwrap();
    api.add_set_joint_rotation(hip, "pelvis", Vec3::new(0.0, -0.2 * style.torso_twist, 0.0)).unwrap();
    api.add_torso_twist_toward_target(hip, "net_center", counter).unwrap();
    api.add_raise_arm_for_balance(hip, true).unwrap();
    api.add_raise_arm_for_balance(hip, false).unwrap();
    api.add_preserve_foot_contact(hip, "left_foot_sole", "left_plant_spot").unwrap();

    // 7. strike — the right instep meets the ball; hip + knee drive through; torso
    //    twists through; the planted foot stays stable; gaze on contact. The
    //    ball_contact event + physics impulse fire this phase.
    let strike = api.add_phase(m, "strike", tick(STRIKE.0), tick(STRIKE.1)).unwrap();
    api.set_phase_root_motion_hold(strike).unwrap();
    api.set_phase_ease_out(strike).unwrap();
    api.set_phase_layer_weight(strike, s(1.0)).unwrap();
    api.add_set_joint_rotation(strike, "right_hip", Vec3::new(-0.5, 0.0, 0.0)).unwrap();
    api.add_leg_strike(strike, true, "ball").unwrap();
    api.add_aim_effector_at_target(strike, "right_foot_instep", "ball").unwrap();
    api.add_orient_surface_toward_target(strike, "right_foot_instep", "net_center").unwrap();
    api.add_torso_twist_toward_target(strike, "net_center", twist).unwrap();
    api.add_keep_gaze_on_target(strike, "ball").unwrap();
    api.add_preserve_foot_contact(strike, "left_foot_sole", "left_plant_spot").unwrap();

    // 8. follow_through — the right leg continues past the ball toward the net;
    //    pelvis and chest keep rotating; arms counterbalance; weight starts leaving
    //    the plant foot (no more pin).
    let follow_ph = api.add_phase(m, "follow_through", tick(FOLLOW_THROUGH.0), tick(FOLLOW_THROUGH.1)).unwrap();
    api.set_phase_root_motion_hold(follow_ph).unwrap();
    api.set_phase_ease_out(follow_ph).unwrap();
    api.set_phase_layer_weight(follow_ph, s(0.6 + 0.2 * style.follow_through_amount)).unwrap();
    api.add_follow_through(follow_ph, true, "net_center").unwrap();
    api.add_move_effector_toward_target(follow_ph, "right_foot_instep", "net_center", follow).unwrap();
    api.add_torso_twist_toward_target(follow_ph, "net_center", counter).unwrap();
    api.add_move_effector_toward_target(follow_ph, "left_hand", "net_center", reach).unwrap();

    // 9. recover — the body settles into a post-kick stance; arms lower; the right
    //    foot returns; rotation stops; gaze can move toward the net. Softest drive.
    let recover = api.add_phase(m, "recover", tick(RECOVER.0), tick(RECOVER.1)).unwrap();
    api.set_phase_root_motion_settle(recover).unwrap();
    api.set_phase_ease_out(recover).unwrap();
    api.set_phase_layer_weight(recover, s(0.2 + 0.2 * (1.0 - style.recovery_settle))).unwrap();
    api.add_move_effector_toward_target(recover, "right_foot_instep", "ball", s(0.3)).unwrap();
    api.add_keep_gaze_on_target(recover, "net_center").unwrap();

    // The strike connects: right instep on the ball, aimed at the net, at power.
    api.add_ball_contact(
        m,
        tick(STRIKE_CONTACT_TICK),
        "right_foot_instep",
        "ball",
        "net_center",
        SoccerPenaltyKickStyle::ratio(style.power),
    )
    .unwrap();

    m
}

fn tick(t: u64) -> Tick {
    Tick::new(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nine_phase_kick_authors_and_compiles() {
        let spec = SoccerPenaltyKickMotionSpec::author(SoccerPenaltyKickStyle::default_style());
        // The phase tick ranges are contiguous, ordered, and cover [0, DURATION).
        let bounds = [SETUP, SPRINT_APPROACH, PRE_PLANT, PLANT, BACKSWING, HIP_DRIVE, STRIKE, FOLLOW_THROUGH, RECOVER];
        assert_eq!(bounds[0].0, 0);
        assert_eq!(bounds[8].1, DURATION);
        for w in bounds.windows(2) {
            assert_eq!(w[0].1, w[1].0, "phases are contiguous");
            assert!(w[0].0 < w[0].1, "phase range is non-empty");
        }
        // The strike tick sits inside the strike phase.
        assert!(STRIKE.0 <= STRIKE_CONTACT_TICK && STRIKE_CONTACT_TICK < STRIKE.1);
        // Sampling each phase's representative tick reports the phases in order.
        let auth = spec.authoring();
        for (i, &t) in PHASE_SAMPLE_TICKS.iter().enumerate() {
            assert_eq!(
                auth.active_phase_name(spec.plan(), Tick::new(t)).unwrap().as_deref(),
                Some(PHASE_NAMES[i]),
                "tick {t} should be in phase {}",
                PHASE_NAMES[i]
            );
        }
    }

    #[test]
    fn style_power_sets_the_ball_contact_power_deterministically() {
        let power_of = |p: f32| {
            let mut st = SoccerPenaltyKickStyle::default_style();
            st.power = p;
            let spec = SoccerPenaltyKickMotionSpec::author(st);
            let frame = spec.authoring().sample(spec.plan(), Tick::new(STRIKE_CONTACT_TICK)).unwrap();
            spec.authoring().frame_ball_contact(&frame).unwrap().3.get()
        };
        assert!((power_of(0.2) - 0.2).abs() < 1.0e-6);
        assert!((power_of(0.9) - 0.9).abs() < 1.0e-6);
        assert!(power_of(0.2) < power_of(0.9));
    }

    #[test]
    fn from_power_maps_the_game_power_meter_into_style() {
        assert!((SoccerPenaltyKickStyle::from_power(0).power - 0.0).abs() < 1.0e-6);
        assert!((SoccerPenaltyKickStyle::from_power(100).power - 1.0).abs() < 1.0e-6);
        assert!(SoccerPenaltyKickStyle::from_power(100).urgency > SoccerPenaltyKickStyle::from_power(0).urgency);
    }
}
