//! Behavioral proofs driven **only** through the public facade.
//!
//! Every test here constructs and steps agents solely through [`AgentApi`],
//! binding the sealed contract values by inference and asserting on their public
//! accessors — never naming a sealed type. These are the determinism and
//! boundary proofs; the per-`src`-file unit tests cover the internals. Nothing
//! here merely proves "does not panic": each test asserts a concrete value.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, KernelErrorCode, KernelErrorScope, Tick};
use axiom_runtime::RuntimeStep;

fn step_at(tick: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(0), Tick::new(tick), 16_666_667, 0)
}

// ---------- identity & profiles ----------

#[test]
fn agent_id_construction_is_deterministic() {
    assert_eq!(AgentApi::create_agent_id(7), AgentApi::create_agent_id(7));
    assert_ne!(AgentApi::create_agent_id(7), AgentApi::create_agent_id(8));
    assert_eq!(AgentApi::create_agent_id(7).raw(), 7);
}

#[test]
fn default_profiles_have_stable_expected_values() {
    let perfect = AgentApi::debug_perfect_profile();
    assert_eq!(perfect.max_actions_per_tick(), 8);
    assert_eq!(perfect.aim_error_milli_degrees(), 0);
    let human = AgentApi::human_like_profile();
    assert_eq!(human.max_actions_per_tick(), 3);
    assert_eq!(human.reaction_delay_ticks(), 12);
    assert_ne!(perfect, human);
}

// ---------- intents & channels ----------

#[test]
fn every_intent_factory_sets_its_kind() {
    use axiom_agent::AgentApi as A;
    // Bind the kind constants off a representative intent's accessor isn't
    // possible (codes are sealed on ActionIntent), so assert the kinds are all
    // distinct and that each factory yields a stable, distinct code.
    let kinds = [
        A::noop_intent().kind_code(),
        A::wait_ticks_intent(1).kind_code(),
        A::press_control_intent(1).kind_code(),
        A::release_control_intent(1).kind_code(),
        A::move_axis_intent(1, 2).kind_code(),
        A::look_axis_intent(1, 2).kind_code(),
        A::pointer_move_intent(1, 2).kind_code(),
        A::pointer_down_intent(1).kind_code(),
        A::pointer_up_intent(1).kind_code(),
        A::look_at_subject_intent(1).kind_code(),
        A::look_at_point_intent(1, 2, 3).kind_code(),
        A::move_toward_subject_intent(1).kind_code(),
        A::move_toward_point_intent(1, 2, 3).kind_code(),
        A::interact_with_subject_intent(1).kind_code(),
        A::use_affordance_intent(1).kind_code(),
        A::focus_attention_intent(1).kind_code(),
    ];
    let mut sorted = kinds.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), kinds.len(), "every intent factory has a distinct kind");
}

#[test]
fn intent_factories_carry_their_payload() {
    assert_eq!(AgentApi::wait_ticks_intent(9).ticks(), 9);
    assert_eq!(AgentApi::press_control_intent(7).control_code(), 7);
    assert_eq!(AgentApi::release_control_intent(7).control_code(), 7);
    assert_eq!(AgentApi::move_axis_intent(2, -5).value(), -5);
    assert_eq!(AgentApi::look_axis_intent(2, 5).axis_code(), 2);
    let pm = AgentApi::pointer_move_intent(11, 22);
    assert_eq!((pm.x(), pm.y()), (11, 22));
    assert_eq!(AgentApi::pointer_down_intent(1).control_code(), 1);
    assert_eq!(AgentApi::pointer_up_intent(1).control_code(), 1);
    assert_eq!(AgentApi::look_at_subject_intent(8).subject_code(), 8);
    let lp = AgentApi::look_at_point_intent(1, 2, 3);
    assert_eq!((lp.x(), lp.y(), lp.z()), (1, 2, 3));
    assert_eq!(AgentApi::move_toward_subject_intent(8).subject_code(), 8);
    let mp = AgentApi::move_toward_point_intent(4, 5, 6);
    assert_eq!((mp.x(), mp.y(), mp.z()), (4, 5, 6));
    assert_eq!(AgentApi::interact_with_subject_intent(8).subject_code(), 8);
    assert_eq!(AgentApi::use_affordance_intent(9).affordance_code(), 9);
    assert_eq!(AgentApi::focus_attention_intent(8).subject_code(), 8);
}

#[test]
fn every_channel_factory_returns_a_distinct_channel() {
    let codes = [
        AgentApi::channel_semantic().code(),
        AgentApi::channel_geometric().code(),
        AgentApi::channel_screen_sample().code(),
        AgentApi::channel_replay().code(),
        AgentApi::channel_debug().code(),
    ];
    assert_eq!(codes, [1, 2, 3, 4, 5]);
}

// ---------- bounded observation ----------

#[test]
fn observation_builder_preserves_order_and_builds() {
    let id = AgentApi::create_agent_id(1);
    let mut b = AgentApi::observation_builder(id, Tick::new(3), 2, 2, 2);
    b.add_channel(AgentApi::channel_semantic()).unwrap();
    b.add_channel(AgentApi::channel_replay()).unwrap();
    b.add_legal_action(10).unwrap();
    b.add_legal_action(20).unwrap();
    b.add_fact(AgentApi::observation_fact(100, 1, 0, 0, 0, 0)).unwrap();
    b.add_fact(AgentApi::observation_fact(200, 2, 0, 0, 0, 0)).unwrap();
    let obs = b.build();
    assert_eq!(obs.tick(), Tick::new(3));
    assert_eq!(obs.legal_actions(), &[10, 20]);
    assert_eq!(obs.fact_count(), 2);
    assert_eq!(obs.channels()[1].code(), AgentApi::channel_replay().code());
    assert_eq!(obs.facts()[0].kind_code(), 100);
}

#[test]
fn observation_builder_overflows_deterministically() {
    let id = AgentApi::create_agent_id(1);
    let mut b = AgentApi::observation_builder(id, Tick::new(0), 1, 1, 1);
    assert!(b.add_channel(AgentApi::channel_semantic()).is_ok());
    let channel_err = b.add_channel(AgentApi::channel_debug()).unwrap_err();
    assert_eq!(channel_err.code(), KernelErrorCode::OutOfBounds);
    assert_eq!(channel_err.scope(), KernelErrorScope::Memory);
    assert!(b.add_legal_action(1).is_ok());
    assert_eq!(b.add_legal_action(2).unwrap_err().code(), KernelErrorCode::OutOfBounds);
    assert!(b.add_fact(AgentApi::observation_fact(1, 1, 0, 0, 0, 0)).is_ok());
    assert_eq!(
        b.add_fact(AgentApi::observation_fact(2, 2, 0, 0, 0, 0)).unwrap_err().code(),
        KernelErrorCode::OutOfBounds
    );
}

#[test]
fn empty_observation_has_no_entries() {
    let id = AgentApi::create_agent_id(2);
    let obs = AgentApi::empty_observation(id, Tick::new(9));
    assert_eq!(obs.agent_id(), id);
    assert_eq!(obs.tick(), Tick::new(9));
    assert_eq!(obs.fact_count(), 0);
    assert_eq!(obs.legal_action_count(), 0);
}

// ---------- bounded action queue ----------

#[test]
fn action_queue_is_fifo_and_overflows_deterministically() {
    let mut q = AgentApi::action_queue(2);
    assert!(q.is_empty());
    q.push(AgentApi::press_control_intent(1)).unwrap();
    q.push(AgentApi::press_control_intent(2)).unwrap();
    let overflow = q.push(AgentApi::press_control_intent(3)).unwrap_err();
    assert_eq!(overflow.code(), KernelErrorCode::OutOfBounds);
    assert_eq!(overflow.scope(), KernelErrorScope::Memory);
    assert_eq!(q.pop().unwrap().control_code(), 1);
    assert_eq!(q.pop().unwrap().control_code(), 2);
    assert!(q.pop().is_none());
}

// ---------- empty memory ----------

#[test]
fn empty_memory_starts_empty() {
    let mem = AgentApi::empty_memory(4);
    assert!(mem.is_empty());
    assert_eq!(mem.capacity(), 4);
}

// ---------- scripted brain through the runtime ----------

// The canonical reason / brain-kind codes are exposed on the facade itself
// (AgentApi::REASON_* / BRAIN_KIND_*), so these proofs reference them
// symbolically rather than by magic number.

#[test]
fn canonical_report_vocabulary_is_exposed_on_the_facade() {
    // Every code in the canonical table is reachable by name through the facade.
    assert_eq!(AgentApi::BRAIN_KIND_NONE, 0);
    assert_eq!(AgentApi::BRAIN_KIND_SCRIPTED, 1);
    assert_eq!(AgentApi::BRAIN_KIND_REPLAY, 2);
    assert_eq!(AgentApi::REASON_NO_REASON, 0);
    assert_eq!(AgentApi::REASON_NO_MATCHING_RULE, 1);
    assert_eq!(AgentApi::REASON_MATCHED_RULE, 2);
    assert_eq!(AgentApi::REASON_REPLAY_EMITTED, 3);
    assert_eq!(AgentApi::REASON_REPLAY_EMPTY, 4);
    assert_eq!(AgentApi::REASON_REPLAY_COMPLETE, 5);
    assert_eq!(AgentApi::REASON_ACTION_BUDGET_ZERO, 6);
}

#[test]
fn scripted_brain_emits_configured_intent_and_rule_reason_on_match() {
    let id = AgentApi::create_agent_id(1);
    let mut brain = AgentApi::scripted_brain(vec![AgentApi::script_rule(
        100,
        AgentApi::press_control_intent(7),
        AgentApi::REASON_MATCHED_RULE,
    )]);
    let mut mem = AgentApi::empty_memory(4);
    let mut b = AgentApi::observation_builder(id, Tick::new(0), 1, 1, 1);
    b.add_fact(AgentApi::observation_fact(100, 1, 0, 0, 0, 0)).unwrap();
    let obs = b.build();
    let (report, queue) = AgentApi::step(
        id,
        AgentApi::debug_perfect_profile(),
        &mut brain,
        &obs,
        &mut mem,
        step_at(5),
    );
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.intents()[0].control_code(), 7);
    assert_eq!(report.emitted_action_count(), 1);
    assert_eq!(report.selected_brain_kind_code(), AgentApi::BRAIN_KIND_SCRIPTED);
    assert_eq!(
        report.reason_code(),
        AgentApi::REASON_MATCHED_RULE,
        "the firing rule's reason code is reported"
    );
    assert_eq!(report.tick(), Tick::new(5));
}

#[test]
fn scripted_brain_emits_noop_when_no_rule_matches() {
    let id = AgentApi::create_agent_id(1);
    let mut brain = AgentApi::scripted_brain(vec![AgentApi::script_rule(
        100,
        AgentApi::press_control_intent(7),
        AgentApi::REASON_MATCHED_RULE,
    )]);
    let mut mem = AgentApi::empty_memory(4);
    let obs = AgentApi::empty_observation(id, Tick::new(0));
    let (report, queue) = AgentApi::step(
        id,
        AgentApi::debug_perfect_profile(),
        &mut brain,
        &obs,
        &mut mem,
        step_at(1),
    );
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.intents()[0].kind_code(), AgentApi::noop_intent().kind_code());
    assert_eq!(report.emitted_action_count(), 1);
    assert_eq!(report.reason_code(), AgentApi::REASON_NO_MATCHING_RULE);
}

#[test]
fn scripted_brain_with_zero_budget_emits_nothing_with_budget_zero_reason() {
    let id = AgentApi::create_agent_id(1);
    let mut brain = AgentApi::scripted_brain(vec![AgentApi::script_rule(
        100,
        AgentApi::press_control_intent(7),
        AgentApi::REASON_MATCHED_RULE,
    )]);
    let mut mem = AgentApi::empty_memory(4);
    let mut b = AgentApi::observation_builder(id, Tick::new(0), 1, 1, 1);
    b.add_fact(AgentApi::observation_fact(100, 1, 0, 0, 0, 0)).unwrap();
    let obs = b.build();
    let frozen = AgentApi::profile_with_action_budget(AgentApi::debug_perfect_profile(), 0);
    let (report, queue) = AgentApi::step(id, frozen, &mut brain, &obs, &mut mem, step_at(5));
    assert!(queue.is_empty());
    assert_eq!(report.emitted_action_count(), 0);
    assert_eq!(report.first_emitted_action_kind_code(), AgentApi::noop_intent().kind_code());
    assert_eq!(report.reason_code(), AgentApi::REASON_ACTION_BUDGET_ZERO);
}

// ---------- replay brain through the runtime ----------

#[test]
fn replay_brain_emits_recorded_actions_then_noop() {
    let id = AgentApi::create_agent_id(1);
    let mut brain = AgentApi::replay_brain(vec![
        AgentApi::press_control_intent(1),
        AgentApi::press_control_intent(2),
    ]);
    let mut mem = AgentApi::empty_memory(8);
    let obs = AgentApi::empty_observation(id, Tick::new(0));
    let profile = AgentApi::debug_perfect_profile();
    let (r1, q1) = AgentApi::step(id, profile, &mut brain, &obs, &mut mem, step_at(0));
    let (r2, q2) = AgentApi::step(id, profile, &mut brain, &obs, &mut mem, step_at(1));
    let (r3, q3) = AgentApi::step(id, profile, &mut brain, &obs, &mut mem, step_at(2));
    assert_eq!(q1.intents()[0].control_code(), 1);
    assert_eq!(r1.selected_brain_kind_code(), AgentApi::BRAIN_KIND_REPLAY);
    assert_eq!(r1.reason_code(), AgentApi::REASON_REPLAY_EMITTED);
    assert_eq!(q2.intents()[0].control_code(), 2);
    assert_eq!(r2.reason_code(), AgentApi::REASON_REPLAY_EMITTED);
    // Past the end of a non-empty recording: a Noop reported as replay_complete.
    assert_eq!(q3.intents()[0].kind_code(), AgentApi::noop_intent().kind_code());
    assert_eq!(r3.reason_code(), AgentApi::REASON_REPLAY_COMPLETE);
}

#[test]
fn empty_replay_brain_emits_noop_with_reason_four() {
    let id = AgentApi::create_agent_id(1);
    let mut brain = AgentApi::replay_brain(Vec::new());
    let mut mem = AgentApi::empty_memory(2);
    let obs = AgentApi::empty_observation(id, Tick::new(0));
    let (report, queue) = AgentApi::step(
        id,
        AgentApi::debug_perfect_profile(),
        &mut brain,
        &obs,
        &mut mem,
        step_at(0),
    );
    assert_eq!(queue.intents()[0].kind_code(), AgentApi::noop_intent().kind_code());
    assert_eq!(
        report.reason_code(),
        AgentApi::REASON_REPLAY_EMPTY,
        "replay_empty (distinct from replay_complete)"
    );
}

// ---------- determinism proofs ----------

#[test]
fn identical_inputs_replay_to_identical_report_and_actions() {
    let run = || {
        let id = AgentApi::create_agent_id(1);
        let mut brain = AgentApi::scripted_brain(vec![AgentApi::script_rule(
            100,
            AgentApi::press_control_intent(7),
            AgentApi::REASON_MATCHED_RULE,
        )]);
        let mut mem = AgentApi::empty_memory(8);
        let mut b = AgentApi::observation_builder(id, Tick::new(0), 1, 1, 1);
        b.add_fact(AgentApi::observation_fact(100, 1, 0, 0, 0, 0)).unwrap();
        let obs = b.build();
        let (report, queue) = AgentApi::step(
            id,
            AgentApi::debug_perfect_profile(),
            &mut brain,
            &obs,
            &mut mem,
            step_at(5),
        );
        (report, queue.intents().to_vec())
    };
    assert_eq!(run(), run(), "same observation + brain + memory + step must replay identically");
}

#[test]
fn a_different_matching_rule_yields_a_different_report() {
    let id = AgentApi::create_agent_id(1);
    let mut brain = AgentApi::scripted_brain(vec![
        AgentApi::script_rule(100, AgentApi::press_control_intent(7), AgentApi::REASON_MATCHED_RULE),
        AgentApi::script_rule(200, AgentApi::wait_ticks_intent(3), AgentApi::REASON_MATCHED_RULE),
    ]);
    let profile = AgentApi::debug_perfect_profile();

    let build_obs = |kind: u16| {
        let mut b = AgentApi::observation_builder(id, Tick::new(0), 1, 1, 1);
        b.add_fact(AgentApi::observation_fact(kind, 1, 0, 0, 0, 0)).unwrap();
        b.build()
    };

    let mut mem_a = AgentApi::empty_memory(4);
    let (report_a, _qa) = AgentApi::step(id, profile, &mut brain, &build_obs(100), &mut mem_a, step_at(5));
    let mut mem_b = AgentApi::empty_memory(4);
    let (report_b, _qb) = AgentApi::step(id, profile, &mut brain, &build_obs(200), &mut mem_b, step_at(5));

    // Different rules fire, so the reports' first emitted action kind differs —
    // and the result is deterministic when each is re-run.
    assert_ne!(report_a, report_b);
    assert_eq!(
        report_a.first_emitted_action_kind_code(),
        AgentApi::press_control_intent(0).kind_code()
    );
    assert_eq!(
        report_b.first_emitted_action_kind_code(),
        AgentApi::wait_ticks_intent(0).kind_code()
    );
}
