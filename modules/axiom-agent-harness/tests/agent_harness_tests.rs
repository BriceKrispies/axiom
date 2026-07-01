//! Behavioural proofs for the [`AgentHarnessApi`] facade, driven only through its
//! public surface (primitives in, primitives out). Covers the held-control
//! decision, the goal-seek policy on every side, and determinism.

use axiom_agent::AgentApi;
use axiom_agent_harness::AgentHarnessApi;
use axiom_kernel::Meters;

#[test]
fn micro_metres_round_trip_sign_and_zero() {
    // Zero maps to zero both ways.
    assert_eq!(AgentHarnessApi::micro(Meters::new(0.0).unwrap()), 0);
    assert_eq!(AgentHarnessApi::metres(0).get(), 0.0);

    // A positive length encodes to its millionths and decodes back.
    let positive = Meters::new(2.5).unwrap();
    assert_eq!(AgentHarnessApi::micro(positive), 2_500_000);
    assert!((AgentHarnessApi::metres(2_500_000).get() - 2.5).abs() < 1.0e-6);

    // Sign is preserved through both directions.
    let negative = Meters::new(-3.25).unwrap();
    assert_eq!(AgentHarnessApi::micro(negative), -3_250_000);
    assert!((AgentHarnessApi::metres(-3_250_000).get() + 3.25).abs() < 1.0e-6);

    // Full round-trip of a decoded value re-encodes to the same integer.
    assert_eq!(
        AgentHarnessApi::micro(AgentHarnessApi::metres(7_125_000)),
        7_125_000
    );
}

/// Facing -Z: the forward vector the growth/retro FPS first-person frame uses at yaw 0.
const FORWARD_NEG_Z: (i64, i64) = (0, -1_000_000);
const SELF_ORIGIN: (i64, i64, i64, i64) = (0, 0, 0, 0);

#[test]
fn control_vocabulary_bits_are_distinct() {
    let bits = [
        AgentHarnessApi::FORWARD,
        AgentHarnessApi::BACKWARD,
        AgentHarnessApi::TURN_LEFT,
        AgentHarnessApi::TURN_RIGHT,
        AgentHarnessApi::STRAFE_LEFT,
        AgentHarnessApi::STRAFE_RIGHT,
        AgentHarnessApi::ACTION_PRIMARY,
        AgentHarnessApi::ACTION_SECONDARY,
    ];
    // Each is a single distinct bit: their OR has exactly 8 bits set.
    let union = bits.iter().fold(0u32, |acc, b| acc | b);
    assert_eq!(
        union.count_ones(),
        8,
        "control bits must be 8 distinct flags"
    );
}

#[test]
fn decide_hold_echoes_the_held_control_via_a_replay_decision() {
    let goal = (0, 0, -2_000_000);
    let (control, reason, brain_kind, emitted) =
        AgentHarnessApi::decide_hold(7, 1, SELF_ORIGIN, goal, AgentHarnessApi::FORWARD);
    assert_eq!(control, AgentHarnessApi::FORWARD, "held control is emitted");
    assert_eq!(
        reason,
        AgentApi::REASON_REPLAY_EMITTED,
        "a replay step emitted it"
    );
    assert_eq!(
        brain_kind,
        AgentApi::BRAIN_KIND_REPLAY,
        "the replay brain decided"
    );
    assert_eq!(emitted, 1, "exactly one intent emitted");
}

#[test]
fn decide_hold_carries_a_combined_control_bitmask() {
    // A single held control_code can hold several bits at once.
    let held = AgentHarnessApi::FORWARD | AgentHarnessApi::STRAFE_RIGHT;
    let (control, _, _, _) = AgentHarnessApi::decide_hold(1, 1, SELF_ORIGIN, (0, 0, 0), held);
    assert_eq!(control, held, "the whole bitmask round-trips");
}

#[test]
fn decide_seek_walks_straight_when_the_goal_is_ahead() {
    let goal = (0, 0, -10_000_000); // dead ahead along -Z
    let (control, _, _, _) =
        AgentHarnessApi::decide_seek(1, 1, SELF_ORIGIN, FORWARD_NEG_Z, goal, 500_000);
    assert_eq!(
        control,
        AgentHarnessApi::FORWARD,
        "ahead ⇒ forward, no turn"
    );
}

#[test]
fn decide_seek_turns_left_for_a_goal_on_one_side() {
    let goal = (10_000_000, 0, -10_000_000); // off to +X
    let (control, _, _, _) =
        AgentHarnessApi::decide_seek(1, 1, SELF_ORIGIN, FORWARD_NEG_Z, goal, 500_000);
    assert_ne!(
        control & AgentHarnessApi::TURN_LEFT,
        0,
        "turns toward the goal"
    );
    assert_eq!(
        control & AgentHarnessApi::TURN_RIGHT,
        0,
        "not the other way"
    );
    assert_ne!(
        control & AgentHarnessApi::FORWARD,
        0,
        "keeps walking while turning"
    );
}

#[test]
fn decide_seek_turns_right_for_a_goal_on_the_other_side() {
    let goal = (-10_000_000, 0, -10_000_000); // off to -X
    let (control, _, _, _) =
        AgentHarnessApi::decide_seek(1, 1, SELF_ORIGIN, FORWARD_NEG_Z, goal, 500_000);
    assert_ne!(
        control & AgentHarnessApi::TURN_RIGHT,
        0,
        "turns toward the goal"
    );
    assert_eq!(control & AgentHarnessApi::TURN_LEFT, 0, "not the other way");
}

#[test]
fn decide_seek_stops_within_the_arrive_radius() {
    let goal = (0, 0, 0); // already there
    let (control, _, _, _) =
        AgentHarnessApi::decide_seek(1, 1, SELF_ORIGIN, FORWARD_NEG_Z, goal, 1_000_000);
    assert_eq!(control, 0, "arrived ⇒ stop holding any control");
}

#[test]
fn decide_goto_walks_toward_a_far_goal_and_reports_not_arrived() {
    let goal = (0, 0, -10_000_000); // dead ahead, far
    let (control, _, _, _, arrived) =
        AgentHarnessApi::decide_goto(1, 1, SELF_ORIGIN, FORWARD_NEG_Z, goal, 500_000);
    assert_eq!(
        control,
        AgentHarnessApi::FORWARD,
        "far + ahead ⇒ walk forward"
    );
    assert_eq!(arrived, 0, "still far ⇒ not arrived");
}

#[test]
fn decide_goto_stops_and_reports_arrived_within_radius() {
    let goal = (0, 0, 0); // already there
    let (control, _, _, _, arrived) =
        AgentHarnessApi::decide_goto(1, 1, SELF_ORIGIN, FORWARD_NEG_Z, goal, 1_000_000);
    assert_eq!(control, 0, "arrived ⇒ stop holding any control");
    assert_eq!(arrived, 1, "within the radius ⇒ arrived flag set");
}

#[test]
fn decide_look_at_is_aimed_when_the_point_is_dead_ahead() {
    // Target straight ahead along -Z, level with the eye.
    let target = (0, 0, -10_000_000);
    let (yaw_turn, pitch, aimed) =
        AgentHarnessApi::decide_look_at(SELF_ORIGIN, FORWARD_NEG_Z, target);
    assert_eq!(aimed, 1, "already facing it ⇒ aimed");
    assert!(yaw_turn.abs() < 30_000, "no meaningful turn needed");
    assert!(pitch.abs() < 30_000, "level target ⇒ ~level pitch");
}

#[test]
fn decide_look_at_turns_left_for_a_point_on_the_left() {
    // Forward is -Z; a target off to +X needs a left turn (positive yaw_turn).
    let target = (10_000_000, 0, 0);
    let (yaw_turn, _pitch, aimed) =
        AgentHarnessApi::decide_look_at(SELF_ORIGIN, FORWARD_NEG_Z, target);
    assert!(yaw_turn > 0, "point on the left ⇒ positive (left) turn");
    assert_eq!(aimed, 0, "a 90° turn is not yet aimed");
}

#[test]
fn decide_look_at_turns_right_for_a_point_on_the_right() {
    let target = (-10_000_000, 0, 0);
    let (yaw_turn, _pitch, _aimed) =
        AgentHarnessApi::decide_look_at(SELF_ORIGIN, FORWARD_NEG_Z, target);
    assert!(yaw_turn < 0, "point on the right ⇒ negative (right) turn");
}

#[test]
fn decide_look_at_pitches_down_for_a_point_below_and_up_for_above() {
    // Eye lifted to y = 5 units; "look at the ground" at y = 0 pitches down.
    let eye = (0, 5_000_000, 0, 0);
    let ground = (0, 0, -10_000_000);
    let (_yaw, pitch_down, _) = AgentHarnessApi::decide_look_at(eye, FORWARD_NEG_Z, ground);
    assert!(
        pitch_down < 0,
        "target below the eye ⇒ pitch down (negative)"
    );

    let peak = (0, 50_000_000, -10_000_000);
    let (_yaw, pitch_up, _) = AgentHarnessApi::decide_look_at(eye, FORWARD_NEG_Z, peak);
    assert!(pitch_up > 0, "target above the eye ⇒ pitch up (positive)");
}

#[test]
fn decisions_are_deterministic() {
    let goal = (3_000_000, 0, -8_000_000);
    let a = AgentHarnessApi::decide_seek(42, 9, SELF_ORIGIN, FORWARD_NEG_Z, goal, 500_000);
    let b = AgentHarnessApi::decide_seek(42, 9, SELF_ORIGIN, FORWARD_NEG_Z, goal, 500_000);
    assert_eq!(a, b, "same inputs ⇒ identical decision tuple");

    let h1 = AgentHarnessApi::decide_hold(5, 2, SELF_ORIGIN, goal, AgentHarnessApi::FORWARD);
    let h2 = AgentHarnessApi::decide_hold(5, 2, SELF_ORIGIN, goal, AgentHarnessApi::FORWARD);
    assert_eq!(h1, h2, "hold decisions are deterministic too");
}
