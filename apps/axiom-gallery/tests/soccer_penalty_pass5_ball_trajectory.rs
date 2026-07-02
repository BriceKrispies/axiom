//! Pass 5 proofs: the deterministic parametric ball trajectory.
//!
//! Fixed-tick, closed-form, integer-and-`f32` arithmetic over constants;
//! determinism is proven by structural equality across independent reruns. No
//! randomness, no wall-clock time, and everything is built over explicit
//! ordered vectors — never a map.

use axiom_gallery::soccer_penalty::penalty_ball::{
    arc_height_for, flight_ticks, penalty_spot, sin_pi_approx, world_target, PenaltyBallState,
    PenaltyShotFlightDescriptor, AIM_HALF_SPAN, AIM_TOP, MAX_FLIGHT_TICKS, MIN_FLIGHT_TICKS,
};
use axiom_gallery::soccer_penalty::penalty_interaction::{PenaltyInteractionState, PenaltyShotFlightState};
use axiom_gallery::soccer_penalty::penalty_render_plan::{PenaltyDrawLayer, PenaltyRenderContent};
use axiom_gallery::soccer_penalty::penalty_scene::{
    BALL_RADIUS, GOAL_HALF_WIDTH, GOAL_HEIGHT, GOAL_LINE_Z, PENALTY_SPOT_Z,
};
use axiom_gallery::soccer_penalty::{PenaltyInputIntent, SoccerPenaltyApp};
use axiom_math::Vec3;

const EPS: f32 = 1.0e-4;

fn close(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() < EPS && (a.y - b.y).abs() < EPS && (a.z - b.z).abs() < EPS
}

fn repeat(intent: PenaltyInputIntent, n: usize) -> Vec<PenaltyInputIntent> {
    (0..n).map(|_| intent).collect()
}

/// Advance from `state` with neutral intents until the ball arrives (bounded).
fn fly_to_arrival(mut state: PenaltyInteractionState) -> (PenaltyInteractionState, u32) {
    let mut steps = 0;
    while state.ball_state() != PenaltyBallState::ArrivedAtGoalPlane && steps < 500 {
        state = state.advance(PenaltyInputIntent::neutral());
        steps += 1;
    }
    (state, steps)
}

// --- ball at rest -----------------------------------------------------------

#[test]
fn default_ball_starts_at_penalty_spot() {
    let s = PenaltyInteractionState::start();
    assert!(close(s.ball_pose().position, penalty_spot()));
    assert_eq!(s.ball_state(), PenaltyBallState::AtPenaltySpot);
}

#[test]
fn ball_stays_at_spot_while_aiming_and_charging() {
    let aiming = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::aiming(100, 100), 6));
    assert!(close(aiming.ball_pose().position, penalty_spot()));
    let charging = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::charging(0, 0), 6));
    assert!(close(charging.ball_pose().position, penalty_spot()));
    // And while the preview is locked (before launch).
    let mut seq = repeat(PenaltyInputIntent::charging(0, 0), 3);
    seq.push(PenaltyInputIntent::releasing());
    let locked = PenaltyInteractionState::run(&seq);
    assert_eq!(locked.state, PenaltyShotFlightState::LockedPreview);
    assert!(close(locked.ball_pose().position, penalty_spot()));
}

// --- mappings ---------------------------------------------------------------

#[test]
fn target_x_maps_to_world_goal_space() {
    // The aim reaches slightly beyond the posts at the extremes (AIM_HALF_SPAN).
    assert!((world_target(100, 50).x - AIM_HALF_SPAN).abs() < EPS);
    assert!((world_target(-100, 50).x + AIM_HALF_SPAN).abs() < EPS);
    assert!(world_target(0, 50).x.abs() < EPS);
    assert!((world_target(50, 50).x - AIM_HALF_SPAN * 0.5).abs() < EPS);
    // Extreme aim is beyond the goal frame; centered aim is well inside it.
    assert!(world_target(100, 50).x > GOAL_HALF_WIDTH);
    assert!(world_target(40, 50).x < GOAL_HALF_WIDTH);
}

#[test]
fn target_y_maps_to_world_goal_space() {
    assert!(world_target(0, 0).y.abs() < EPS);
    assert!((world_target(0, 100).y - AIM_TOP).abs() < EPS);
    assert!((world_target(0, 50).y - AIM_TOP * 0.5).abs() < EPS);
    // Extreme aim is above the crossbar; centered aim is under it.
    assert!(world_target(0, 100).y > GOAL_HEIGHT);
    assert!(world_target(0, 40).y < GOAL_HEIGHT);
    // Always on the goal plane.
    assert!((world_target(30, 70).z - GOAL_LINE_Z).abs() < EPS);
}

#[test]
fn power_maps_to_flight_ticks_stronger_is_shorter() {
    assert_eq!(flight_ticks(0), MAX_FLIGHT_TICKS);
    assert_eq!(flight_ticks(100), MIN_FLIGHT_TICKS);
    // Monotonic non-increasing in power.
    let seq: Vec<u32> = (0..=100).step_by(10).map(flight_ticks).collect();
    seq.windows(2).for_each(|w| assert!(w[1] <= w[0], "stronger power must not lengthen flight"));
    // Clamped.
    assert!(flight_ticks(200) >= MIN_FLIGHT_TICKS);
    assert!(flight_ticks(-50) <= MAX_FLIGHT_TICKS);
}

#[test]
fn sin_pi_approx_is_a_well_formed_arc() {
    assert!(sin_pi_approx(0.0).abs() < EPS);
    assert!(sin_pi_approx(1.0).abs() < EPS);
    assert!((sin_pi_approx(0.5) - 1.0).abs() < EPS);
    // Rises to the apex then falls.
    assert!(sin_pi_approx(0.25) < sin_pi_approx(0.5));
    assert!(sin_pi_approx(0.75) < sin_pi_approx(0.5));
    // Arc height falls with power.
    assert!(arc_height_for(100) < arc_height_for(0));
}

// --- trajectory shape -------------------------------------------------------

#[test]
fn trajectory_starts_at_spot_and_ends_at_target() {
    let d = descriptor_for(40, 74, 96);
    assert!(close(d.trajectory.position_at(0), penalty_spot()));
    assert!(close(d.trajectory.position_at(d.trajectory.total_ticks), world_target(40, 74)));
}

#[test]
fn z_progresses_monotonically_toward_the_goal_plane() {
    let d = descriptor_for(-30, 20, 40);
    let total = d.trajectory.total_ticks;
    let zs: Vec<f32> = (0..=total).map(|e| d.trajectory.position_at(e).z).collect();
    // Starts at the spot's z, ends at the goal plane, strictly decreasing.
    assert!((zs[0] - PENALTY_SPOT_Z).abs() < EPS);
    assert!((zs[total as usize] - GOAL_LINE_Z).abs() < EPS);
    zs.windows(2).for_each(|w| assert!(w[1] < w[0], "z must move monotonically toward the goal"));
}

fn descriptor_for(tx: i32, ty: i32, power: i32) -> PenaltyShotFlightDescriptor {
    use axiom_gallery::soccer_penalty::PenaltyShotPreview;
    PenaltyShotFlightDescriptor::from_preview(PenaltyShotPreview {
        target_x: tx,
        target_y: ty,
        power,
        release_tick: 0,
    })
}

// --- flight lifecycle -------------------------------------------------------

#[test]
fn releasing_launches_a_deterministic_flight() {
    let mut seq = repeat(PenaltyInputIntent::charging(0, 0), 5);
    seq.push(PenaltyInputIntent::releasing());
    let locked = PenaltyInteractionState::run(&seq);
    // The tick after the lock launches the ball at the spot.
    let launched = locked.advance(PenaltyInputIntent::neutral());
    assert_eq!(launched.state, PenaltyShotFlightState::BallInFlight);
    assert_eq!(launched.ball_state(), PenaltyBallState::InFlight);
    assert!(close(launched.ball_pose().position, penalty_spot()));
    // The next tick the ball has moved off the spot toward the goal.
    let moving = launched.advance(PenaltyInputIntent::neutral());
    assert!(moving.ball_pose().position.z < PENALTY_SPOT_Z);
}

#[test]
fn ball_enters_arrived_state_at_the_goal_plane() {
    // Aim to the top-right corner so the shot clears the Pass 6 goalie volumes
    // and reaches the goal plane untouched.
    let mut seq = repeat(PenaltyInputIntent::aiming(100, 0), 12);
    seq.extend(repeat(PenaltyInputIntent::aiming(0, 100), 5));
    seq.extend(repeat(PenaltyInputIntent::charging(0, 0), 6));
    seq.push(PenaltyInputIntent::releasing());
    let locked = PenaltyInteractionState::run(&seq);
    let (arrived, _) = fly_to_arrival(locked);
    assert_eq!(arrived.state, PenaltyShotFlightState::ArrivedAtGoalPlane);
    let flight = arrived.flight.expect("flight present");
    assert!(flight.arrived());
    assert_eq!(flight.elapsed_ticks, flight.total());
    // The ball is frozen at the goal plane. (Pass 8 resolves the shot on the
    // following tick; the ball position stays put.)
    let next = arrived.advance(PenaltyInputIntent::neutral());
    assert_eq!(next.state, PenaltyShotFlightState::Resolved);
    assert_eq!(next.ball_pose().position, arrived.ball_pose().position);
}

#[test]
fn reset_returns_ball_to_spot_and_state_to_aiming() {
    let mut seq = repeat(PenaltyInputIntent::charging(0, 0), 4);
    seq.push(PenaltyInputIntent::releasing());
    let (flying, _) = {
        let locked = PenaltyInteractionState::run(&seq);
        (locked.advance(PenaltyInputIntent::neutral()).advance(PenaltyInputIntent::neutral()), ())
    };
    assert_eq!(flying.state, PenaltyShotFlightState::BallInFlight);
    let reset = flying.advance(PenaltyInputIntent::resetting());
    assert_eq!(reset.state, PenaltyShotFlightState::Aiming);
    assert_eq!(reset.flight, None);
    assert!(close(reset.ball_pose().position, penalty_spot()));
}

// --- shadow -----------------------------------------------------------------

#[test]
fn ball_shadow_updates_deterministically_during_flight() {
    let mut seq = repeat(PenaltyInputIntent::charging(0, 0), 3);
    seq.push(PenaltyInputIntent::releasing());
    let launched = PenaltyInteractionState::run(&seq).advance(PenaltyInputIntent::neutral());
    let mid = launched.advance(PenaltyInputIntent::neutral()).advance(PenaltyInputIntent::neutral());
    let pose = mid.ball_pose();
    // Shadow tracks the ball's x/z and stays on the pitch (constant y).
    assert!((pose.shadow_center.x - pose.position.x).abs() < EPS);
    assert!((pose.shadow_center.z - pose.position.z).abs() < EPS);
    let resting = PenaltyInteractionState::start().ball_pose();
    assert!((pose.shadow_center.y - resting.shadow_center.y).abs() < EPS);
    // Airborne ball → smaller shadow than at rest.
    assert!(pose.shadow_radius_x <= resting.shadow_radius_x);
}

// --- render integration -----------------------------------------------------

#[test]
fn ball_and_shadow_render_items_keep_their_layers() {
    let mut seq = repeat(PenaltyInputIntent::charging(30, 30), 4);
    seq.push(PenaltyInputIntent::releasing());
    let flying = PenaltyInteractionState::run(&seq)
        .advance(PenaltyInputIntent::neutral())
        .advance(PenaltyInputIntent::neutral())
        .advance(PenaltyInputIntent::neutral());
    let frame = SoccerPenaltyApp::build_frame(&flying);

    let layer_of = |label: &str| {
        frame
            .render_plan
            .items
            .iter()
            .find(|it| it.label == label)
            .map(|it| it.layer())
            .expect("item present")
    };
    assert_eq!(layer_of("ball"), PenaltyDrawLayer::Ball);
    assert_eq!(layer_of("shadow.ball"), PenaltyDrawLayer::ActorShadow);

    // The ball render item reflects the live (moved) position.
    let ball = frame.render_plan.items.iter().find(|it| it.label == "ball").unwrap();
    match ball.content {
        PenaltyRenderContent::World { position, .. } => assert!(position.z < PENALTY_SPOT_Z),
        PenaltyRenderContent::Hud { .. } => panic!("ball must be a world item"),
    }
}

#[test]
fn trail_items_are_deterministic_and_in_foreground_effects() {
    let mut seq = repeat(PenaltyInputIntent::charging(0, 0), 3);
    seq.push(PenaltyInputIntent::releasing());
    // Several ticks into flight so a trail has accumulated.
    let flying = (0..6).fold(PenaltyInteractionState::run(&seq), |s, _| {
        s.advance(PenaltyInputIntent::neutral())
    });
    let pose = flying.ball_pose();
    assert!(pose.trail_len > 0, "a trail should have accumulated");

    let frame = SoccerPenaltyApp::build_frame(&flying);
    let trail: Vec<_> = frame.render_plan.items.iter().filter(|it| it.label == "ball.trail").collect();
    assert_eq!(trail.len(), pose.trail_len as usize);
    trail.iter().for_each(|it| assert_eq!(it.layer(), PenaltyDrawLayer::ForegroundEffects));

    // Deterministic: an identical rebuild yields an identical frame.
    assert_eq!(frame, SoccerPenaltyApp::build_frame(&flying));
    // Trail draws after all non-HUD world items but before the HUD.
    let first_hud = frame
        .render_plan
        .items
        .iter()
        .position(|it| matches!(it.content, PenaltyRenderContent::Hud { .. }))
        .unwrap();
    let last_trail = frame
        .render_plan
        .items
        .iter()
        .rposition(|it| it.label == "ball.trail")
        .unwrap();
    assert!(last_trail < first_hud, "trail must render before the HUD");
}

// --- the required full-flow test -------------------------------------------

#[test]
fn full_flow_lands_on_the_mapped_target_and_is_reproducible() {
    // Aim to the far top-right corner — beyond the Pass 7 diving keeper's
    // reach — so the ball clears and reaches the goal plane.
    let script = {
        let mut seq = Vec::new();
        seq.extend(repeat(PenaltyInputIntent::aiming(100, 0), 12)); // aim far right
        seq.extend(repeat(PenaltyInputIntent::aiming(0, 100), 3)); // aim up 3
        seq.extend(repeat(PenaltyInputIntent::charging(0, 0), 12)); // charge 12
        seq.push(PenaltyInputIntent::releasing()); // release
        seq
    };

    let run_once = || {
        let locked = PenaltyInteractionState::run(&script);
        let (arrived, steps) = fly_to_arrival(locked);
        (arrived, steps)
    };

    let (a, steps_a) = run_once();
    // The ball lands exactly on the mapped goal-plane target (x 96, y 74).
    assert_eq!(a.state, PenaltyShotFlightState::ArrivedAtGoalPlane);
    assert!(close(a.ball_pose().position, world_target(96, 74)));

    // Re-run from a fresh state: identical final state and identical step count.
    let (b, steps_b) = run_once();
    assert_eq!(a, b, "the same shot must reproduce the same final state");
    assert_eq!(steps_a, steps_b);

    // Identical sampled flight descriptors.
    assert_eq!(a.flight, b.flight);
}

// A stray import guard so BALL_RADIUS stays referenced by an assertion.
#[test]
fn ball_pose_radius_is_the_ball_radius() {
    assert_eq!(PenaltyInteractionState::start().ball_pose().radius, BALL_RADIUS);
}
