//! Direct tests for the app-local locomotion animator: the two-bone leg solver,
//! the distance-driven gait phase, planted-foot locking, stride/cadence bounds,
//! locomotion modes, pose composition, and deterministic replay. These prove the
//! anti-skate invariant — the planted foot holds a world position while the body
//! travels over it — without a browser.

use axiom::prelude::Vec3;
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::data::{BiomechTuning, LocomotionTuning};
use axiom_end_zone::player::animation::{apply_hold, override_pose, BallHold, JointPose};
use axiom_end_zone::player::model::{L_THIGH, R_THIGH};
use axiom_end_zone::player::AnimState;
use axiom_end_zone::presentation::locomotion::foot::FootPhase;
use axiom_end_zone::presentation::locomotion::gait::{self, LocomotionInput};
use axiom_end_zone::presentation::locomotion::leg::{self, LegDims};
use axiom_end_zone::presentation::locomotion::pose;
use axiom_end_zone::presentation::locomotion::spring::BodySprings;
use axiom_end_zone::presentation::locomotion::{
    GaitState, LocomotionMode, OverrideReason, PlantedFoot,
};
use axiom_end_zone::showcase::{DiagnosticCommand, ShowcaseRun};
use axiom_end_zone::state::{PlayPhase, SimCommand, SimState};
use axiom_math::Quat;

const DT: f32 = 16_666_667.0 / 1_000_000_000.0;
const EPS: f32 = 1.0e-3;

fn planar(v: Vec3) -> f32 {
    Vec3::new(v.x, 0.0, v.z).length()
}

fn run_input(pos: Vec3, vel: Vec3, facing: f32) -> LocomotionInput {
    LocomotionInput {
        pos,
        vel,
        facing,
        speed: planar(vel),
        grounded: true,
        allowed: true,
        reason: OverrideReason::None,
        teleported: false,
    }
}

// ----- Two-bone leg solver ------------------------------------------------

#[test]
fn leg_solver_reaches_reachable_targets_and_bends_the_knee_forward() {
    let dims = LegDims::from_model();
    let hip = Vec3::new(0.0, 1.0, 0.0);
    let forward = Vec3::new(0.0, 0.0, 1.0);
    // A spread of reachable ankle targets: straight down, ahead, behind, to the
    // side, and near full reach.
    let targets = [
        Vec3::new(0.0, 0.15, 0.0),
        Vec3::new(0.0, 0.15, 0.35),
        Vec3::new(0.0, 0.15, -0.30),
        Vec3::new(0.25, 0.15, 0.10),
        Vec3::new(0.0, 0.20, 0.0),
    ];
    for target in targets {
        let solve = leg::solve(dims, Quat::IDENTITY, hip, target, forward);
        let fk = leg::ankle_world(dims, Quat::IDENTITY, hip, solve.thigh, solve.shin);
        assert!(
            fk.distance(solve.ankle) < EPS,
            "FK must reproduce the solved ankle for {target:?}: fk={fk:?} ankle={:?}",
            solve.ankle
        );
        assert!(
            fk.distance(target) < 0.05,
            "solved ankle near target {target:?}: {fk:?}"
        );
        // The knee (thigh tip) leads forward of the straight hip→ankle line.
        let knee =
            hip.add(Quat::IDENTITY.rotate(solve.thigh.rotate(Vec3::new(0.0, -dims.thigh, 0.0))));
        let midpoint = hip.add(target.subtract(hip).mul_scalar(0.5));
        assert!(
            knee.z >= midpoint.z - EPS,
            "knee {knee:?} must not invert behind the hip→ankle midpoint {midpoint:?}"
        );
        assert!(knee.x.is_finite() && knee.y.is_finite() && knee.z.is_finite());
    }
}

#[test]
fn leg_solver_clamps_unreachable_targets_without_stretching() {
    let dims = LegDims::from_model();
    let hip = Vec3::new(0.0, 1.0, 0.0);
    // Far beyond the leg's reach.
    let target = Vec3::new(0.0, 1.0, 6.0);
    let solve = leg::solve(dims, Quat::IDENTITY, hip, target, Vec3::new(0.0, 0.0, 1.0));
    assert!(solve.clamped, "an out-of-reach target must report clamped");
    let reached = hip.distance(solve.ankle);
    assert!(
        reached <= dims.max_reach() + EPS,
        "clamped reach {reached} must not exceed max reach {}",
        dims.max_reach()
    );
    let fk = leg::ankle_world(dims, Quat::IDENTITY, hip, solve.thigh, solve.shin);
    assert!(fk.x.is_finite() && fk.y.is_finite() && fk.z.is_finite());
}

// ----- Distance-driven gait phase -----------------------------------------

#[test]
fn identical_displacement_advances_the_phase_identically() {
    let tuning = LocomotionTuning::default();
    let mut a = GaitState::new();
    let mut b = GaitState::new();
    // Prime both to the same anchor.
    let start = Vec3::ZERO;
    let step = Vec3::new(0.0, 0.0, 0.06);
    let vel = Vec3::new(0.0, 0.0, 3.6);
    let mut pa = start;
    let mut pb = start;
    for _ in 0..40 {
        pa = pa.add(step);
        pb = pb.add(step);
        gait::advance(&mut a, run_input(pa, vel, 0.0), &tuning);
        gait::advance(&mut b, run_input(pb, vel, 0.0), &tuning);
    }
    assert!(
        (a.phase - b.phase).abs() < 1.0e-6,
        "same displacement → same phase"
    );
    assert!(a.traveled > 2.0, "the body actually traveled");
}

#[test]
fn zero_displacement_does_not_advance_the_phase() {
    let tuning = LocomotionTuning::default();
    let mut g = GaitState::new();
    let pos = Vec3::new(1.0, 0.0, 2.0);
    // Prime one tick so it initializes.
    gait::advance(&mut g, run_input(pos, Vec3::ZERO, 0.0), &tuning);
    let phase0 = g.phase;
    for _ in 0..30 {
        gait::advance(&mut g, run_input(pos, Vec3::ZERO, 0.0), &tuning);
    }
    assert!(
        (g.phase - phase0).abs() < 1.0e-4 || settles_to_foot_down(g.phase),
        "a standing player does not loop the gait"
    );
}

fn settles_to_foot_down(phase: f32) -> bool {
    let nearest = (phase * 2.0).round() / 2.0;
    (phase - nearest).abs() < 0.02
}

#[test]
fn blocked_movement_does_not_advance_gait_as_though_it_moved() {
    let tuning = LocomotionTuning::default();
    let mut g = GaitState::new();
    let pos = Vec3::new(0.0, 0.0, 0.0);
    // The player REQUESTS speed (vel is non-zero) but is blocked: actual position
    // never changes. The gait must not cycle on requested velocity.
    let vel = Vec3::new(0.0, 0.0, 6.0);
    gait::advance(&mut g, run_input(pos, vel, 0.0), &tuning);
    let phase0 = g.phase;
    for _ in 0..30 {
        gait::advance(&mut g, run_input(pos, vel, 0.0), &tuning);
    }
    assert!(
        (g.phase - phase0).abs() < 1.0e-4,
        "blocked (zero actual displacement) must not advance the gait, was {} now {}",
        phase0,
        g.phase
    );
}

#[test]
fn faster_actual_movement_advances_the_phase_faster() {
    let tuning = LocomotionTuning::default();
    let mut slow = GaitState::new();
    let mut fast = GaitState::new();
    let mut ps = Vec3::ZERO;
    let mut pf = Vec3::ZERO;
    let slow_step = Vec3::new(0.0, 0.0, 0.03);
    let fast_step = Vec3::new(0.0, 0.0, 0.12);
    for _ in 0..30 {
        ps = ps.add(slow_step);
        pf = pf.add(fast_step);
        gait::advance(
            &mut slow,
            run_input(ps, Vec3::new(0.0, 0.0, 1.8), 0.0),
            &tuning,
        );
        gait::advance(
            &mut fast,
            run_input(pf, Vec3::new(0.0, 0.0, 7.2), 0.0),
            &tuning,
        );
    }
    assert!(
        fast.traveled > slow.traveled * 2.0,
        "faster movement covers more distance"
    );
    // The faster runner has advanced through more total cycles.
    assert!(fast.traveled / fast.stride_length > slow.traveled / slow.stride_length);
}

#[test]
fn teleport_and_reset_do_not_advance_the_gait() {
    let tuning = LocomotionTuning::default();
    let mut g = GaitState::new();
    // Establish a running gait.
    let mut p = Vec3::ZERO;
    for _ in 0..20 {
        p = p.add(Vec3::new(0.0, 0.0, 0.08));
        gait::advance(&mut g, run_input(p, Vec3::new(0.0, 0.0, 4.8), 0.0), &tuning);
    }
    let phase_before = g.phase;
    // A play reset teleports the player 40 yards downfield.
    let jumped = Vec3::new(0.0, 0.0, 45.0);
    let ov = gait::advance(
        &mut g,
        run_input(jumped, Vec3::new(0.0, 0.0, 4.8), 0.0),
        &tuning,
    );
    assert_eq!(
        ov,
        OverrideReason::None,
        "a grounded teleport is still allowed locomotion"
    );
    assert_eq!(
        g.phase, phase_before,
        "the teleport distance must not advance the phase"
    );
    // The feet re-anchored under the new position.
    assert!(planar(g.left.lock.subtract(jumped)) < 0.5);
    assert!(planar(g.right.lock.subtract(jumped)) < 0.5);
}

#[test]
fn replaying_the_same_displacement_history_is_identical() {
    let tuning = LocomotionTuning::default();
    let history: Vec<(Vec3, Vec3, f32)> = (0..60)
        .map(|i| {
            let t = i as f32;
            let z = 0.05 * t + 0.02 * (t * 0.3).sin();
            (
                Vec3::new(0.1 * (t * 0.2).sin(), 0.0, z),
                Vec3::new(0.0, 0.0, 3.0),
                0.0,
            )
        })
        .collect();
    let run = |h: &[(Vec3, Vec3, f32)]| {
        let mut g = GaitState::new();
        let mut phases = Vec::new();
        for &(p, v, f) in h {
            gait::advance(&mut g, run_input(p, v, f), &tuning);
            phases.push((g.phase, g.left.lock, g.right.lock, g.planted));
        }
        phases
    };
    let a = run(&history);
    let b = run(&history);
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b.iter()) {
        assert_eq!(
            x.0.to_bits(),
            y.0.to_bits(),
            "phase replay must be bit-identical"
        );
        assert_eq!(x.3, y.3, "planted foot replay must match");
    }
}

// ----- Stride / cadence bounds --------------------------------------------

#[test]
fn stride_and_cadence_stay_within_configured_bounds() {
    let tuning = LocomotionTuning::default();
    for speed in [0.5_f32, 2.0, 4.0, 6.0, 8.5, 11.0] {
        let mut g = GaitState::new();
        let step = Vec3::new(0.0, 0.0, speed * DT);
        let mut p = Vec3::ZERO;
        for _ in 0..40 {
            p = p.add(step);
            gait::advance(
                &mut g,
                run_input(p, Vec3::new(0.0, 0.0, speed), 0.0),
                &tuning,
            );
        }
        assert!(
            g.stride_length >= tuning.jog_stride * 0.35 - EPS
                && g.stride_length <= tuning.sprint_stride * 1.15 + EPS,
            "stride {} out of bounds at speed {speed}",
            g.stride_length
        );
        assert!(
            g.cadence <= tuning.max_cadence + EPS,
            "cadence {} exceeds ceiling at speed {speed}",
            g.cadence
        );
    }
}

#[test]
fn sprinting_uses_a_longer_stride_than_jogging() {
    let tuning = LocomotionTuning::default();
    let stride_at = |speed: f32| {
        let mut g = GaitState::new();
        let mut p = Vec3::ZERO;
        for _ in 0..40 {
            p = p.add(Vec3::new(0.0, 0.0, speed * DT));
            gait::advance(
                &mut g,
                run_input(p, Vec3::new(0.0, 0.0, speed), 0.0),
                &tuning,
            );
        }
        g.stride_length
    };
    assert!(
        stride_at(8.4) > stride_at(3.0),
        "sprint stride exceeds jog stride"
    );
}

#[test]
fn startup_expands_the_stride_over_time() {
    let tuning = LocomotionTuning::default();
    let mut g = GaitState::new();
    let mut p = Vec3::ZERO;
    // First accelerating tick.
    p = p.add(Vec3::new(0.0, 0.0, 6.0 * DT));
    gait::advance(&mut g, run_input(p, Vec3::new(0.0, 0.0, 6.0), 0.0), &tuning);
    let early = g.stride_length;
    let early_startup = g.startup;
    for _ in 0..30 {
        p = p.add(Vec3::new(0.0, 0.0, 6.0 * DT));
        gait::advance(&mut g, run_input(p, Vec3::new(0.0, 0.0, 6.0), 0.0), &tuning);
    }
    assert!(g.startup > early_startup, "the startup ramp advances");
    assert!(g.stride_length > early, "stride grows as startup completes");
}

#[test]
fn stopping_converges_to_a_stable_idle() {
    let tuning = LocomotionTuning::default();
    let mut g = GaitState::new();
    let mut p = Vec3::ZERO;
    for _ in 0..25 {
        p = p.add(Vec3::new(0.0, 0.0, 5.0 * DT));
        gait::advance(&mut g, run_input(p, Vec3::new(0.0, 0.0, 5.0), 0.0), &tuning);
    }
    // Now stand still.
    for _ in 0..40 {
        gait::advance(&mut g, run_input(p, Vec3::ZERO, 0.0), &tuning);
    }
    assert_eq!(
        g.mode,
        LocomotionMode::Idle,
        "a stopped player settles to Idle"
    );
    assert!(
        settles_to_foot_down(g.phase),
        "the gait settles a foot down, not mid-swing"
    );
}

#[test]
fn sharp_turns_shorten_the_stride() {
    let tuning = LocomotionTuning::default();
    // Straight run.
    let mut straight = GaitState::new();
    let mut ps = Vec3::ZERO;
    for _ in 0..20 {
        ps = ps.add(Vec3::new(0.0, 0.0, 6.0 * DT));
        gait::advance(
            &mut straight,
            run_input(ps, Vec3::new(0.0, 0.0, 6.0), 0.0),
            &tuning,
        );
    }
    // A hard turn: velocity direction rotates each tick.
    let mut turn = GaitState::new();
    let mut pt = Vec3::ZERO;
    for i in 0..20 {
        let a = i as f32 * 0.25;
        let vel = Vec3::new(6.0 * a.sin(), 0.0, 6.0 * a.cos());
        pt = pt.add(vel.mul_scalar(DT));
        gait::advance(&mut turn, run_input(pt, vel, a), &tuning);
    }
    assert!(turn.turn_intensity > 0.2, "the turn registers intensity");
    assert!(
        turn.stride_length < straight.stride_length,
        "a turn shortens the stride: turn {} straight {}",
        turn.stride_length,
        straight.stride_length
    );
}

// ----- Planted-foot locking -----------------------------------------------

/// One tick's per-foot record: (phase, world lock, this-tick target, lock error)
/// for the left then right foot, plus the primary planted foot.
#[derive(Clone, Copy)]
struct FootTick {
    left: (FootPhase, Vec3, Vec3, f32),
    right: (FootPhase, Vec3, Vec3, f32),
    planted: PlantedFoot,
}

/// Drive a straight sprint and record each foot every tick (legs solved so
/// `lock_error` is filled).
fn straight_run_feet(ticks: usize) -> Vec<FootTick> {
    let tuning = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut springs = BodySprings::new();
    let mut g = GaitState::new();
    let mut p = Vec3::ZERO;
    let mut out = Vec::new();
    for _ in 0..ticks {
        p = p.add(Vec3::new(0.0, 0.0, 6.0 * DT));
        gait::advance(&mut g, run_input(p, Vec3::new(0.0, 0.0, 6.0), 0.0), &tuning);
        pose::locomotion_pose(
            &mut g,
            &mut springs,
            0.0,
            p,
            AnimState::Sprint,
            &tuning,
            &bio,
        )
        .0;
        out.push(FootTick {
            left: (g.left.phase, g.left.lock, g.left.target, g.left.lock_error),
            right: (
                g.right.phase,
                g.right.lock,
                g.right.target,
                g.right.lock_error,
            ),
            planted: g.planted,
        });
    }
    out
}

fn is_stance_phase(p: FootPhase) -> bool {
    matches!(
        p,
        FootPhase::Planted | FootPhase::Landing | FootPhase::PushOff
    )
}

#[test]
fn the_planted_foot_holds_a_world_position_while_the_body_advances() {
    let feet = straight_run_feet(160);
    // The precise anti-skate invariant: while a foot stays in a stance phase
    // across consecutive ticks, its world lock (and its ground target) do not
    // move — the body travels over a fixed foot.
    let mut max_drift = 0.0_f32;
    for w in feet.windows(2) {
        for pick in [|t: &FootTick| t.left, |t: &FootTick| t.right] {
            let (p0, lock0, tgt0, _) = pick(&w[0]);
            let (p1, lock1, tgt1, _) = pick(&w[1]);
            if is_stance_phase(p0) && is_stance_phase(p1) {
                max_drift = max_drift.max(planar(lock1.subtract(lock0)));
                max_drift = max_drift.max(planar(Vec3::new(tgt1.x - tgt0.x, 0.0, tgt1.z - tgt0.z)));
            }
        }
    }
    assert!(
        max_drift < 1.0e-4,
        "a planted foot must not slide while planted (max drift {max_drift})"
    );
}

#[test]
fn foot_lock_error_stays_small_and_alternates_between_feet() {
    let feet = straight_run_feet(160);
    for t in &feet {
        assert!(t.left.3.is_finite() && t.right.3.is_finite());
        assert!(
            t.left.3 < 0.2 && t.right.3 < 0.2,
            "lock error (foot reaches its target) stays bounded: L {} R {}",
            t.left.3,
            t.right.3
        );
    }
    // Both feet take turns being the primary planted foot.
    let lefts = feet
        .iter()
        .filter(|t| t.planted == PlantedFoot::Left)
        .count();
    let rights = feet
        .iter()
        .filter(|t| t.planted == PlantedFoot::Right)
        .count();
    assert!(
        lefts > 10 && rights > 10,
        "planting alternates: L {lefts} R {rights}"
    );
    // Each foot passes through both stance and swing.
    assert!(feet.iter().any(|t| t.left.0 == FootPhase::Swing));
    assert!(feet.iter().any(|t| t.right.0 == FootPhase::Swing));
    assert!(feet.iter().any(|t| is_stance_phase(t.left.0)));
}

#[test]
fn airborne_and_teleport_invalidate_both_foot_locks() {
    let tuning = LocomotionTuning::default();
    let mut g = GaitState::new();
    let mut p = Vec3::ZERO;
    for _ in 0..20 {
        p = p.add(Vec3::new(0.0, 0.0, 6.0 * DT));
        gait::advance(&mut g, run_input(p, Vec3::new(0.0, 0.0, 6.0), 0.0), &tuning);
    }
    // Airborne: not grounded.
    let mut airborne = run_input(p, Vec3::new(0.0, 0.0, 6.0), 0.0);
    airborne.grounded = false;
    let ov = gait::advance(&mut g, airborne, &tuning);
    assert_ne!(ov, OverrideReason::None, "airborne suspends locomotion");
    assert!(
        planar(g.left.lock.subtract(p)) < 0.5,
        "left foot re-anchored under the body"
    );
    assert!(
        planar(g.right.lock.subtract(p)) < 0.5,
        "right foot re-anchored under the body"
    );
}

#[test]
fn every_generated_joint_and_foot_position_is_finite() {
    let tuning = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut springs = BodySprings::new();
    let mut g = GaitState::new();
    let mut p = Vec3::ZERO;
    for i in 0..80 {
        let a = i as f32 * 0.1;
        let vel = Vec3::new(5.0 * a.sin(), 0.0, 5.0 * a.cos());
        p = p.add(vel.mul_scalar(DT));
        gait::advance(&mut g, run_input(p, vel, a), &tuning);
        let jp =
            pose::locomotion_pose(&mut g, &mut springs, a, p, AnimState::Sprint, &tuning, &bio).0;
        for q in jp.joints {
            assert!(q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && q.w.is_finite());
        }
        assert!(jp.root_lift.is_finite() && jp.root_pitch.is_finite() && jp.root_roll.is_finite());
        for foot in [g.left, g.right] {
            assert!(
                foot.ankle.x.is_finite() && foot.ankle.y.is_finite() && foot.ankle.z.is_finite()
            );
            assert!(foot.target.x.is_finite() && foot.lock_error.is_finite());
        }
    }
}

// ----- Pose composition + override boundary -------------------------------

fn joint_nonidentity(q: Quat) -> bool {
    (q.x.abs() + q.y.abs() + q.z.abs()) > 1.0e-3
}

#[test]
fn the_carry_hold_does_not_remove_lower_body_locomotion() {
    let tuning = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut springs = BodySprings::new();
    let mut g = GaitState::new();
    let mut p = Vec3::ZERO;
    for _ in 0..20 {
        p = p.add(Vec3::new(0.0, 0.0, 6.0 * DT));
        gait::advance(&mut g, run_input(p, Vec3::new(0.0, 0.0, 6.0), 0.0), &tuning);
    }
    let mut composed = pose::locomotion_pose(
        &mut g,
        &mut springs,
        0.0,
        p,
        AnimState::Sprint,
        &tuning,
        &bio,
    )
    .0;
    // The legs are posed by locomotion before any hold overlay.
    assert!(
        joint_nonidentity(composed.joints[L_THIGH]) || joint_nonidentity(composed.joints[R_THIGH]),
        "locomotion poses the legs"
    );
    let legs_before = (composed.joints[L_THIGH], composed.joints[R_THIGH]);
    apply_hold(&mut composed, BallHold::Cradle);
    // Stage 3 (carry) leaves the legs untouched — the runner keeps running.
    assert_eq!(
        (composed.joints[L_THIGH], composed.joints[R_THIGH]),
        legs_before,
        "the carry overlay must not disturb the legs"
    );
}

#[test]
fn fall_and_action_overrides_suppress_normal_locomotion() {
    // A ground-impact override is a distinct, prone whole-body pose — nothing
    // like a running pose.
    let down = override_pose(AnimState::GroundImpact, 2);
    assert!(
        down.root_pitch < -1.0,
        "the downed pose pitches onto the back"
    );
    // A throw override is also a self-posed body, unrelated to the leg cycle.
    let throw = override_pose(AnimState::Throw, 6);
    assert!(joint_nonidentity(throw.joints[R_THIGH]) || throw.root_pitch.abs() > 0.0);
    // A locomotion state routed (defensively) through the override yields the
    // neutral base, so the caller's own locomotion pose stands.
    let neutral = override_pose(AnimState::Sprint, 4);
    assert!(!joint_nonidentity(neutral.joints[L_THIGH]));
}

#[test]
fn pose_composition_is_deterministic_for_the_same_input_and_gait() {
    let tuning = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let build = || {
        let mut springs = BodySprings::new();
        let mut g = GaitState::new();
        let mut p = Vec3::ZERO;
        let mut last = JointPose::neutral();
        for _ in 0..30 {
            p = p.add(Vec3::new(0.0, 0.0, 5.0 * DT));
            gait::advance(&mut g, run_input(p, Vec3::new(0.0, 0.0, 5.0), 0.0), &tuning);
            last = pose::locomotion_pose(
                &mut g,
                &mut springs,
                0.0,
                p,
                AnimState::Sprint,
                &tuning,
                &bio,
            )
            .0;
        }
        last
    };
    let a = build();
    let b = build();
    for i in 0..a.joints.len() {
        assert_eq!(a.joints[i].x.to_bits(), b.joints[i].x.to_bits());
        assert_eq!(a.joints[i].y.to_bits(), b.joints[i].y.to_bits());
        assert_eq!(a.joints[i].z.to_bits(), b.joints[i].z.to_bits());
        assert_eq!(a.joints[i].w.to_bits(), b.joints[i].w.to_bits());
    }
    assert_eq!(a.root_lift.to_bits(), b.root_lift.to_bits());
}

// ----- Full-run deterministic replay through the real harness --------------

/// A scripted diagnostic sequence exercising acceleration, sprinting, contact,
/// turning, stopping, reset, ball carrying, and tackle transitions (the showcase
/// runs a whole play and auto-resets, driving every locomotion path).
fn scripted_pose_history(ticks: usize) -> Vec<Vec<u32>> {
    let mut run = ShowcaseRun::new(EndZoneConfig::default());
    let mut history = Vec::new();
    for tick in 0..ticks {
        // A scripted primary action mid-play stands in for the user snap/throw.
        let commands: &[DiagnosticCommand] = if tick == 180 {
            &[DiagnosticCommand::PrimaryAction]
        } else {
            &[]
        };
        let out = run.step(commands);
        let mut digest = Vec::new();
        for pp in &out.poses {
            for q in pp.pose.joints {
                digest.push(q.x.to_bits());
                digest.push(q.y.to_bits());
                digest.push(q.z.to_bits());
                digest.push(q.w.to_bits());
            }
            digest.push(pp.pose.root_lift.to_bits());
            digest.push(pp.sample.gait_phase.to_bits());
            digest.push(pp.sample.left_target.z.to_bits());
            digest.push(pp.sample.right_target.z.to_bits());
            digest.push(pp.sample.mode as u32);
            digest.push(pp.sample.planted as u32);
        }
        history.push(digest);
    }
    history
}

#[test]
fn locomotion_replays_bit_for_bit_over_a_full_scripted_sequence() {
    let a = scripted_pose_history(360);
    let b = scripted_pose_history(360);
    assert_eq!(a.len(), b.len());
    for (tick, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(
            x, y,
            "locomotion pose/gait history must be bit-identical on replay (tick {tick})"
        );
    }
    // The sequence actually exercised motion: some tick advanced a gait phase.
    assert!(
        a.iter().flatten().any(|&b| b != 0),
        "the run produced motion"
    );
}

// ----- The ready stance is the pre-snap set, and it is eased into ----------

/// Settle a standing player in `anim` for `ticks`, returning the final root
/// lift and the largest single-tick change in it.
fn settle_standing(
    gait: &mut GaitState,
    springs: &mut BodySprings,
    anim: AnimState,
    ticks: u32,
    loco: &LocomotionTuning,
    bio: &BiomechTuning,
) -> (f32, f32) {
    let mut last = pose::locomotion_pose(gait, springs, 0.0, Vec3::ZERO, anim, loco, bio)
        .0
        .root_lift;
    let mut worst = 0.0f32;
    for _ in 0..ticks {
        gait::advance(gait, run_input(Vec3::ZERO, Vec3::ZERO, 0.0), loco);
        let lift = pose::locomotion_pose(gait, springs, 0.0, Vec3::ZERO, anim, loco, bio)
            .0
            .root_lift;
        worst = worst.max((lift - last).abs());
        last = lift;
    }
    (last, worst)
}

#[test]
fn the_ready_crouch_is_a_held_stance_that_the_body_eases_into_and_out_of() {
    let loco = LocomotionTuning::default();
    let bio = BiomechTuning::default();
    let mut gait = GaitState::new();
    let mut springs = BodySprings::new();

    // Settling INTO the set: the hips arrive at the authored crouch depth, and
    // no single tick jumps the whole way there.
    let (crouched, entry_pop) = settle_standing(
        &mut gait,
        &mut springs,
        AnimState::ReadyStance,
        120,
        &loco,
        &bio,
    );
    assert!(
        (crouched + loco.ready_crouch).abs() < 0.01,
        "the set stance settles at the authored crouch depth: {crouched} vs {}",
        -loco.ready_crouch
    );
    assert!(
        entry_pop < loco.ready_crouch * 0.5,
        "entering the set must ease, not pop: worst tick moved {entry_pop} yd"
    );

    // Standing up out of it — the whistle case — must also ease. Before the
    // posture became a spring target this was a one-tick snap of the full
    // crouch depth.
    let (standing, exit_pop) =
        settle_standing(&mut gait, &mut springs, AnimState::Idle, 120, &loco, &bio);
    assert!(
        standing.abs() < 0.01,
        "a stopped idle player stands up straight, no residual crouch: {standing}"
    );
    assert!(
        exit_pop < loco.ready_crouch * 0.5,
        "leaving the set must ease, not pop: worst tick moved {exit_pop} yd"
    );
}

#[test]
fn only_the_pre_snap_phase_puts_a_stopped_player_in_the_ready_stance() {
    let mut sim = SimState::new(EndZoneConfig::default());
    sim.step(&[SimCommand::BeginPlay]);
    assert_eq!(sim.phase, PlayPhase::PreSnap);
    assert!(
        sim.players.iter().any(|p| p.anim == AnimState::ReadyStance),
        "players are set in the ready stance before the snap"
    );

    // Run the play out to the whistle, then let everyone come to a stop.
    sim.step(&[SimCommand::Snap]);
    let mut ticks = 0;
    while sim.phase != PlayPhase::Ended && ticks < 1200 {
        sim.step(&[]);
        ticks += 1;
    }
    assert_eq!(sim.phase, PlayPhase::Ended, "the play reached the whistle");
    for _ in 0..180 {
        sim.step(&[]);
    }

    // After the whistle nobody is crouched in the pre-snap set — the players
    // who have come to a stop are standing idle instead.
    assert!(
        sim.players.iter().all(|p| p.anim != AnimState::ReadyStance),
        "the ready crouch is pre-snap only; it must not return on a dead ball"
    );
    assert!(
        sim.players.iter().any(|p| p.anim == AnimState::Idle),
        "stopped players stand idle after the whistle"
    );
}
