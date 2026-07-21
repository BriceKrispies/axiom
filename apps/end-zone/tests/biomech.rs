//! Deterministic tests for the whole-body sprint biomechanics: the gait-phase
//! contract, the stance/flight split, the pelvis weight-transfer curves, the
//! virtual-muscle springs, and the one-way gameplay-root → visual-body-root
//! boundary.
//!
//! These test the pure animation *math* and state transitions — no rendering,
//! no browser, no wall clock. The pose-composition side (IK, joint transforms)
//! is only touched where it proves an invariant the carriage owns.

use axiom::prelude::Vec3;
use axiom_end_zone::data::{BiomechTuning, LocomotionTuning};
use axiom_end_zone::player::animation::JointPose;
use axiom_end_zone::player::model::{PART_COUNT, PELVIS};
use axiom_end_zone::player::{rig, AnimState};
use axiom_end_zone::presentation::locomotion::carriage::{self, Carry};
use axiom_end_zone::presentation::locomotion::gait::{self, LocomotionInput};
use axiom_end_zone::presentation::locomotion::spring::{BodySprings, Spring};
use axiom_end_zone::presentation::locomotion::{pose, GaitState, OverrideReason, PlantedFoot};

const DT: f32 = 16_666_667.0 / 1_000_000_000.0;

fn planar(v: Vec3) -> f32 {
    Vec3::new(v.x, 0.0, v.z).length()
}

fn run_input(pos: Vec3, vel: Vec3) -> LocomotionInput {
    LocomotionInput {
        pos,
        vel,
        facing: 0.0,
        speed: planar(vel),
        grounded: true,
        allowed: true,
        reason: OverrideReason::None,
        teleported: false,
    }
}

/// Run a player straight down +Z at `speed` for `ticks`, returning the gait and
/// the springs so a test can inspect the resolved state.
fn sprint(ticks: usize, speed: f32) -> (GaitState, BodySprings, Vec3) {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();
    let mut springs = BodySprings::new();
    let mut pos = Vec3::ZERO;
    let vel = Vec3::new(0.0, 0.0, speed);
    for _ in 0..ticks {
        gait::advance(&mut gait, run_input(pos, vel), &loco);
        pose::locomotion_pose(
            &mut gait,
            &mut springs,
            0.0,
            pos,
            AnimState::Sprint,
            &loco,
            &bio,
        );
        pos = pos.add(vel.mul_scalar(DT));
    }
    (gait, springs, pos)
}

// ----- gait phase ---------------------------------------------------------

#[test]
fn gait_phase_advances_deterministically_with_distance() {
    let loco = LocomotionTuning::default();
    let mut gait = GaitState::new();
    let vel = Vec3::new(0.0, 0.0, 8.4);
    let mut pos = Vec3::ZERO;
    let mut phases = Vec::new();
    for _ in 0..90 {
        gait::advance(&mut gait, run_input(pos, vel), &loco);
        phases.push(gait.phase);
        pos = pos.add(vel.mul_scalar(DT));
    }
    assert!(
        phases.iter().all(|p| (0.0..1.0).contains(p)),
        "the phase stays normalized to [0, 1)"
    );
    // Replaying the identical input stream reproduces the identical phases.
    let mut again = GaitState::new();
    let mut pos = Vec3::ZERO;
    for expected in &phases {
        gait::advance(&mut again, run_input(pos, vel), &loco);
        assert_eq!(
            again.phase.to_bits(),
            expected.to_bits(),
            "the gait phase is bit-for-bit reproducible"
        );
        pos = pos.add(vel.mul_scalar(DT));
    }
}

#[test]
fn a_stationary_player_does_not_keep_running_a_sprint_cycle() {
    let loco = LocomotionTuning::default();
    let mut gait = GaitState::new();
    // Get the gait moving, then stop dead and hold still.
    let mut pos = Vec3::ZERO;
    let vel = Vec3::new(0.0, 0.0, 8.4);
    for _ in 0..40 {
        gait::advance(&mut gait, run_input(pos, vel), &loco);
        pos = pos.add(vel.mul_scalar(DT));
    }
    for _ in 0..60 {
        gait::advance(&mut gait, run_input(pos, Vec3::ZERO), &loco);
    }
    let settled = gait.phase;
    for _ in 0..120 {
        gait::advance(&mut gait, run_input(pos, Vec3::ZERO), &loco);
    }
    assert!(
        (gait.phase - settled).abs() < 1.0e-4,
        "a standing player's phase holds still ({settled} → {}) instead of \
         cycling", gait.phase
    );
    // And it settles ON a foot-down position, not mid-swing.
    let nearest = (settled * 2.0).round() / 2.0;
    assert!(
        (settled - nearest).abs() < 1.0e-3,
        "the stopped phase settles to a foot-down (0 or ½), got {settled}"
    );
}

#[test]
fn the_carriage_fades_out_at_a_standstill() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();
    gait.norm_speed = 0.0;
    let still = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
    assert_eq!(still.activity, 0.0, "no gait activity at a standstill");
    assert_eq!(still.root_lateral, 0.0, "no weight shift while standing");
    assert_eq!(still.root_lift, 0.0, "no stride bob while standing");

    gait.norm_speed = 1.0;
    let running = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
    assert_eq!(running.activity, 1.0, "full carriage at a sprint");
}

// ----- stance alternation and foot planting -------------------------------

#[test]
fn left_and_right_stance_phases_alternate_across_the_cycle() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();
    gait.norm_speed = 1.0;
    gait.stride_length = loco.sprint_stride;

    // The left foot bears weight through the first half of the cycle, the
    // right through the second — the two are exactly half a cycle apart.
    for step in 0..20 {
        let phase = step as f32 / 20.0;
        gait.phase = phase;
        let c = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
        let expected = if phase < 0.5 {
            PlantedFoot::Left
        } else {
            PlantedFoot::Right
        };
        assert_eq!(c.stance, expected, "stance side at phase {phase}");
    }
}

#[test]
fn stance_progress_runs_strike_to_toe_off_then_enters_flight() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();
    gait.norm_speed = 1.0;
    // A long sprint stride shrinks ground contact, so a flight phase exists.
    gait.stride_length = loco.sprint_stride;

    gait.phase = 0.0;
    let strike = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
    assert_eq!(strike.stance_progress, 0.0, "a stride begins at foot-strike");
    assert!(!strike.in_flight, "foot-strike is not flight");

    gait.phase = 0.49;
    let late = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
    assert!(
        late.in_flight,
        "the end of a sprint half-cycle is airborne/transition"
    );
    assert_eq!(late.stance_progress, 1.0, "stance has fully run out by then");
}

#[test]
fn the_planted_foot_target_holds_still_through_its_support_phase() {
    let loco = LocomotionTuning::default();
    let mut gait = GaitState::new();
    let vel = Vec3::new(0.0, 0.0, 8.4);
    let mut pos = Vec3::ZERO;
    // Settle the gait first so we sample a steady-state stride.
    for _ in 0..60 {
        gait::advance(&mut gait, run_input(pos, vel), &loco);
        pos = pos.add(vel.mul_scalar(DT));
    }
    // Follow whichever foot is planted and check its lock never moves while it
    // stays planted, even though the body travels metres over it.
    let mut locked: Option<Vec3> = None;
    let mut travelled = 0.0_f32;
    for _ in 0..40 {
        gait::advance(&mut gait, run_input(pos, vel), &loco);
        let (lock, phase) = match gait.planted {
            PlantedFoot::Left => (gait.left.lock, gait.left.phase),
            PlantedFoot::Right => (gait.right.lock, gait.right.phase),
        };
        let in_support = !matches!(
            phase,
            axiom_end_zone::presentation::locomotion::FootPhase::Swing
        );
        match (in_support, locked) {
            (true, Some(previous)) => {
                assert!(
                    lock.distance(previous) < 1.0e-5,
                    "the planted foot's world lock is fixed during support"
                );
            }
            (true, None) => locked = Some(lock),
            (false, _) => locked = None,
        }
        pos = pos.add(vel.mul_scalar(DT));
        travelled += planar(vel.mul_scalar(DT));
    }
    assert!(travelled > 1.0, "the body really did travel during the test");
}

// ----- pelvis weight transfer ---------------------------------------------

#[test]
fn the_pelvis_shifts_laterally_toward_the_stance_leg() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();
    gait.norm_speed = 1.0;
    gait.stride_length = loco.sprint_stride;

    // Mid-left-stance: the body's weight is over the LEFT foot, which sits on
    // the -X side of the centerline, so the pelvis shifts to -X.
    gait.phase = 0.12;
    let left = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
    assert_eq!(left.stance, PlantedFoot::Left);
    assert!(
        left.root_lateral < -1.0e-3,
        "weight shifts toward the left stance leg (-X), got {}",
        left.root_lateral
    );

    // Mid-right-stance: mirrored.
    gait.phase = 0.62;
    let right = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
    assert_eq!(right.stance, PlantedFoot::Right);
    assert!(
        right.root_lateral > 1.0e-3,
        "weight shifts toward the right stance leg (+X), got {}",
        right.root_lateral
    );
    assert!(
        (left.root_lateral + right.root_lateral).abs() < 1.0e-4,
        "the two stance phases are symmetric mirrors"
    );
}

#[test]
fn the_pelvis_sinks_accepting_weight_and_rises_through_push_off() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();
    gait.norm_speed = 1.0;
    gait.stride_length = loco.sprint_stride;

    // Walk the ground-contact part of one step at fine resolution. Sampling by
    // resolved stance progress (rather than a hardcoded phase) keeps this test
    // honest if the stride or planted fraction is ever retuned.
    let mut stance: Vec<(f32, f32)> = Vec::new();
    for step in 0..2000 {
        gait.phase = step as f32 / 2000.0 * 0.5;
        let c = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
        if !c.in_flight {
            stance.push((c.stance_progress, c.root_lift));
        }
    }
    assert!(stance.len() > 20, "the sweep found a real ground-contact phase");

    let strike = stance[0].1;
    let (dip_at, dip) = stance
        .iter()
        .fold((0.0, f32::MAX), |acc, &(s, l)| if l < acc.1 { (s, l) } else { acc });
    let toe_off = stance[stance.len() - 1].1;

    assert!(
        strike.abs() < 1.0e-3,
        "a stride starts level at foot-strike, got {strike}"
    );
    assert!(
        dip < -1.0e-3,
        "the pelvis LOWERS as the stance leg accepts weight, got {dip}"
    );
    assert!(
        (0.05..0.75).contains(&dip_at),
        "the sink bottoms out during weight acceptance, not at toe-off \
         (stance progress {dip_at})"
    );
    assert!(
        toe_off > dip,
        "the pelvis RISES again through push-off ({dip} → {toe_off})"
    );
}

#[test]
fn every_pelvis_offset_stays_inside_its_configured_bounds() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();

    // Sweep the whole cycle at several speeds, including a hard acceleration.
    for speed_step in 0..=10 {
        for accel_step in -4..=4 {
            gait.norm_speed = speed_step as f32 / 10.0;
            gait.stride_length = loco.sprint_stride;
            gait.accel = Vec3::new(0.0, 0.0, accel_step as f32 * 12.0);
            for phase_step in 0..64 {
                gait.phase = phase_step as f32 / 64.0;
                let c = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
                assert!(c.is_finite(), "carriage stays finite");
                assert!(
                    c.root_lift.abs() <= bio.vertical_bound + 1.0e-6,
                    "vertical pelvis travel {} within ±{}",
                    c.root_lift,
                    bio.vertical_bound
                );
                assert!(
                    c.root_lateral.abs() <= bio.lateral_bound + 1.0e-6,
                    "lateral pelvis travel {} within ±{}",
                    c.root_lateral,
                    bio.lateral_bound
                );
                assert!(
                    c.pelvis_yaw.abs() <= bio.pelvis_yaw_max + 1.0e-6,
                    "pelvis yaw {} within ±{}",
                    c.pelvis_yaw,
                    bio.pelvis_yaw_max
                );
                assert!(
                    c.pelvis_pitch.abs() <= bio.pelvis_tilt_max + 1.0e-6,
                    "pelvis pitch within bound"
                );
                assert!(
                    c.pelvis_roll.abs() <= bio.pelvis_drop + 1.0e-6,
                    "pelvis roll within bound"
                );
            }
        }
    }
}

#[test]
fn the_ribcage_counter_rotates_against_the_pelvis() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();
    gait.norm_speed = 1.0;
    gait.stride_length = loco.sprint_stride;
    // Drive the feet apart so there is a real pelvis yaw to counter.
    gait.left.target = Vec3::new(-0.14, 0.09, 1.0);
    gait.right.target = Vec3::new(0.14, 0.09, -0.4);
    gait.phase = 0.25;

    let c = carriage::solve(&gait, 0.0, Carry::Running, &loco, &bio);
    assert!(c.pelvis_yaw.abs() > 1.0e-3, "the pelvis yaws at all");
    assert!(
        c.spine_yaw * c.pelvis_yaw < 0.0,
        "the lower spine counter-rotates against the pelvis"
    );
    assert!(
        c.ribcage_yaw * c.pelvis_yaw < 0.0,
        "the ribcage counter-rotates against the pelvis"
    );
    // The head cancels most of what it inherits, so it is steadier than the hips.
    let inherited = c.pelvis_yaw + c.spine_yaw + c.ribcage_yaw;
    assert!(
        (inherited + c.head_yaw).abs() < inherited.abs() + 1.0e-6,
        "head stabilization reduces, never amplifies, inherited yaw"
    );
}

// ----- the gameplay-root / visual-body-root boundary ----------------------

#[test]
fn visual_offsets_never_move_the_authoritative_gameplay_transform() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();
    let mut springs = BodySprings::new();
    let vel = Vec3::new(0.0, 0.0, 8.4);
    let mut pos = Vec3::ZERO;
    let mut moved_visually = false;

    for _ in 0..120 {
        // The authoritative root is whatever the sim says — captured before.
        let gameplay_root = pos;
        let facing = 0.0_f32;
        gait::advance(&mut gait, run_input(pos, vel), &loco);
        let (jp, _) = pose::locomotion_pose(
            &mut gait,
            &mut springs,
            facing,
            pos,
            AnimState::Sprint,
            &loco,
            &bio,
        );
        // The animator may not write back into the position/facing it was given.
        assert_eq!(
            pos.x.to_bits(),
            gameplay_root.x.to_bits(),
            "the gameplay root is untouched by the animator"
        );
        assert_eq!(pos.z.to_bits(), gameplay_root.z.to_bits());
        assert_eq!(facing.to_bits(), 0.0_f32.to_bits());

        // The VISUAL root, meanwhile, genuinely departs from it.
        let body = rig::body_transform(gameplay_root, facing, &jp, 0.0);
        let lateral = body.translation.x - gameplay_root.x;
        moved_visually |= lateral.abs() > 1.0e-3;

        pos = pos.add(vel.mul_scalar(DT));
    }
    assert!(
        moved_visually,
        "the visual body root really does shift off the gameplay root — \
         otherwise the hips are still anchored"
    );
}

#[test]
fn the_visual_body_root_is_bounded_relative_to_the_gameplay_root() {
    let bio = BiomechTuning::default();
    let (_, _, _) = sprint(200, 8.4);
    let loco = LocomotionTuning::default();
    let mut gait = GaitState::new();
    let mut springs = BodySprings::new();
    let vel = Vec3::new(0.0, 0.0, 8.4);
    let mut pos = Vec3::ZERO;

    for _ in 0..240 {
        gait::advance(&mut gait, run_input(pos, vel), &loco);
        let (jp, _) = pose::locomotion_pose(
            &mut gait,
            &mut springs,
            0.0,
            pos,
            AnimState::Sprint,
            &loco,
            &bio,
        );
        assert!(
            jp.root_lateral.abs() <= bio.lateral_bound + 1.0e-3,
            "the sprung lateral offset stays inside its bound"
        );
        assert!(
            jp.root_lift.abs() <= bio.vertical_bound + 1.0e-3,
            "the sprung vertical offset stays inside its bound"
        );
        pos = pos.add(vel.mul_scalar(DT));
    }
}

// ----- virtual muscle (springs) -------------------------------------------

#[test]
fn a_spring_converges_on_a_held_target_and_stays_bounded() {
    let mut s = Spring::at(0.0);
    for _ in 0..600 {
        let v = s.step(1.0, 400.0, 1.0, 1.0);
        assert!(v.is_finite() && v.abs() < 4.0, "spring stays bounded: {v}");
    }
    assert!(
        (s.value - 1.0).abs() < 1.0e-3,
        "a critically damped spring settles on its target, got {}",
        s.value
    );
    assert!(s.velocity.abs() < 1.0e-2, "and comes to rest");
}

#[test]
fn a_spring_rejects_non_finite_input_and_extreme_stiffness() {
    let mut s = Spring::at(0.5);
    // NaN / infinite targets and stiffness must not poison the state.
    s.step(f32::NAN, 300.0, 1.0, 0.5);
    assert!(s.value.is_finite() && s.velocity.is_finite());
    s.step(f32::INFINITY, f32::NAN, f32::NAN, 0.5);
    assert!(s.value.is_finite() && s.velocity.is_finite());
    // A wildly over-stiff request is clamped rather than exploding.
    for _ in 0..400 {
        s.step(1.0, 1.0e9, 1.0, 0.5);
        assert!(s.value.is_finite(), "clamped stiffness stays stable");
    }
    assert!(s.value.abs() < 10.0, "no divergence, got {}", s.value);
}

#[test]
fn the_per_tick_correction_is_capped() {
    let mut s = Spring::at(0.0);
    let cap = 0.01_f32;
    let mut previous = 0.0;
    for _ in 0..200 {
        let v = s.step(100.0, 900.0, 1.0, cap);
        assert!(
            (v - previous).abs() <= cap + 1.0e-6,
            "no tick moves more than the configured cap"
        );
        previous = v;
    }
}

#[test]
fn the_spring_bank_resets_cleanly_across_a_discontinuity() {
    let mut springs = BodySprings::new();
    for _ in 0..30 {
        springs.root_lift.step(0.4, 500.0, 1.0, 0.1);
        springs.arm_swing.step(0.9, 200.0, 0.8, 0.2);
    }
    assert!(springs.root_lift.value.abs() > 1.0e-3, "the bank moved");
    springs.reset();
    assert_eq!(springs.root_lift.value, 0.0);
    assert_eq!(springs.root_lift.velocity, 0.0);
    assert_eq!(springs.arm_swing.value, 0.0);
    assert!(springs.is_finite());
}

// ----- determinism and joint-transform safety ------------------------------

#[test]
fn identical_input_streams_produce_identical_animation_state() {
    let (a_gait, a_springs, a_pos) = sprint(300, 8.4);
    let (b_gait, b_springs, b_pos) = sprint(300, 8.4);
    assert_eq!(a_gait.phase.to_bits(), b_gait.phase.to_bits());
    assert_eq!(a_pos.z.to_bits(), b_pos.z.to_bits());
    assert_eq!(
        a_springs.root_lift.value.to_bits(),
        b_springs.root_lift.value.to_bits(),
        "the virtual-muscle state replays bit-for-bit"
    );
    assert_eq!(
        a_springs.pelvis_yaw.value.to_bits(),
        b_springs.pelvis_yaw.value.to_bits()
    );
    assert_eq!(
        a_springs.arm_swing.velocity.to_bits(),
        b_springs.arm_swing.velocity.to_bits()
    );
}

#[test]
fn no_nan_or_infinity_can_enter_a_player_joint_transform() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let states = [
        AnimState::Idle,
        AnimState::Jog,
        AnimState::Sprint,
        AnimState::DropBack,
        AnimState::ReadyStance,
    ];
    // Hostile inputs: standstill, full sprint, violent direction changes, and
    // an absurd acceleration spike — the pose must stay finite throughout.
    for anim in states {
        let mut gait = GaitState::new();
        let mut springs = BodySprings::new();
        let mut pos = Vec3::ZERO;
        for tick in 0..400 {
            let swing = (tick as f32 * 0.31).sin();
            let speed = 8.4 * (tick as f32 * 0.07).sin().abs();
            let vel = Vec3::new(swing * speed, 0.0, speed);
            gait::advance(&mut gait, run_input(pos, vel), &loco);
            let (jp, carriage) = pose::locomotion_pose(
                &mut gait,
                &mut springs,
                swing * 3.0,
                pos,
                anim,
                &loco,
                &bio,
            );
            assert!(carriage.is_finite(), "{anim:?}: carriage finite at {tick}");
            assert!(springs.is_finite(), "{anim:?}: springs finite at {tick}");
            assert_pose_finite(&jp, anim, tick);
            pos = pos.add(vel.mul_scalar(DT));
        }
    }
}

fn assert_pose_finite(jp: &JointPose, anim: AnimState, tick: usize) {
    assert!(
        jp.root_lift.is_finite()
            && jp.root_lateral.is_finite()
            && jp.root_pitch.is_finite()
            && jp.root_roll.is_finite(),
        "{anim:?}: the visual body root is finite at tick {tick}"
    );
    for index in 0..PART_COUNT {
        let q = jp.joints[index];
        assert!(
            q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && q.w.is_finite(),
            "{anim:?}: joint {index} is finite at tick {tick}"
        );
        // A non-unit quaternion would silently scale or shear the limb box.
        let norm = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
        assert!(
            (norm - 1.0).abs() < 1.0e-3,
            "{anim:?}: joint {index} stays a unit quaternion at tick {tick} (|q| = {norm})"
        );
    }
    // And the resolved body transform the rig hands to the renderer.
    let body = rig::body_transform(Vec3::ZERO, 0.0, jp, 0.0);
    assert!(
        body.translation.x.is_finite()
            && body.translation.y.is_finite()
            && body.translation.z.is_finite(),
        "{anim:?}: the visual body root transform is finite at tick {tick}"
    );
    let pelvis = body.transform_point(
        axiom_end_zone::player::model::PARTS[PELVIS].offset,
    );
    assert!(
        pelvis.x.is_finite() && pelvis.y.is_finite() && pelvis.z.is_finite(),
        "{anim:?}: the pelvis resolves finite at tick {tick}"
    );
}
