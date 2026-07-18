//! Camera + juice proofs: event-driven mode transitions, impulse decay to
//! exactly zero, an untouched base rig, replay-identical poses, and bounded,
//! seeded, sim-inert presentation effects.

use axiom::prelude::Vec3;
use axiom_end_zone::camera::impulse::{MAX_AMPLITUDE, MAX_FOV_KICK, MAX_IMPULSES};
use axiom_end_zone::camera::modes::CameraMode;
use axiom_end_zone::camera::{CameraImpulse, ImpulseSample, ImpulseStack};
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::events::SimEvent;
use axiom_end_zone::presentation::{effect_instances, EffectInstance};
use axiom_end_zone::showcase::{run_trace, DiagnosticCommand, ShowcaseRun};

/// The tick of the first event matching `pick`, from a default trace.
fn event_tick(pick: impl Fn(&SimEvent) -> bool) -> u64 {
    let trace = run_trace(EndZoneConfig::default(), 700);
    trace
        .events
        .iter()
        .find(|e| pick(&e.event))
        .expect("event occurs in the showcase")
        .tick
}

fn mode_tick(trace: &axiom_end_zone::showcase::ShowcaseTrace, mode: CameraMode) -> Option<u64> {
    trace
        .camera_modes
        .iter()
        .find(|(_, m)| *m == mode)
        .map(|(t, _)| *t)
}

#[test]
fn throw_events_select_the_pass_flight_camera() {
    let trace = run_trace(EndZoneConfig::default(), 700);
    let throw = event_tick(|e| matches!(e, SimEvent::Throw { .. }));
    assert_eq!(mode_tick(&trace, CameraMode::PassFlight), Some(throw));
}

#[test]
fn catch_events_begin_the_transfer_to_the_catching_player() {
    let trace = run_trace(EndZoneConfig::default(), 700);
    let attempt = event_tick(|e| matches!(e, SimEvent::CatchAttempt { .. }));
    assert_eq!(
        mode_tick(&trace, CameraMode::CatchResolve),
        Some(attempt),
        "CatchResolve starts at the attempt — before/at the possession transfer"
    );
}

#[test]
fn possession_transfer_resolves_to_ball_carrier_follow() {
    let trace = run_trace(EndZoneConfig::default(), 700);
    let resolve = mode_tick(&trace, CameraMode::CatchResolve).expect("resolve happens");
    let follow = mode_tick(&trace, CameraMode::BallCarrierFollow).expect("follow happens");
    assert!(
        follow > resolve,
        "the blend hands off to the carrier camera"
    );
}

#[test]
fn a_ball_carrier_ground_impact_adds_a_camera_impulse_and_the_impact_camera() {
    let mut run = ShowcaseRun::new(EndZoneConfig::default());
    let mut saw_impact = false;
    for _ in 0..700 {
        let out = run.step(&[]);
        let offense = out.snapshot.player(out.snapshot.quarterback).team;
        let carrier_down = out.events.iter().any(|e| {
            matches!(
                e.event,
                SimEvent::GroundImpact { player, .. }
                    if out.snapshot.player(player).team == offense
            )
        });
        if carrier_down {
            saw_impact = true;
            assert!(
                run.director.active_impulses() > 0,
                "the carrier's landing shook the camera"
            );
            assert_eq!(out.camera_mode, CameraMode::Impact);
        }
    }
    assert!(saw_impact, "the showcase tackles the carrier to the turf");
}

#[test]
fn a_defenders_ground_impact_does_not_steal_the_camera() {
    use axiom_end_zone::camera::CameraDirector;
    use axiom_end_zone::data::CameraTuning;
    use axiom_end_zone::events::{EventId, StampedEvent};
    use axiom_end_zone::presentation::snapshot::capture;
    use axiom_end_zone::state::SimState;

    let sim = SimState::new(EndZoneConfig::default());
    let snapshot = capture(&sim);
    let offense = snapshot.player(snapshot.quarterback).team;
    let defender = snapshot
        .players
        .iter()
        .find(|p| p.team != offense)
        .expect("a defender exists")
        .id;
    let offender = snapshot
        .players
        .iter()
        .find(|p| p.team == offense)
        .expect("an offense player exists")
        .id;
    let ground_impact = |player| StampedEvent {
        id: EventId::new(snapshot.tick, 0),
        tick: snapshot.tick,
        event: SimEvent::GroundImpact {
            player,
            position: Vec3::new(0.0, 0.0, 0.0),
            strength: 1.0,
        },
    };

    // A diving defender eating turf must not cut to the impact camera or shake.
    let mut director = CameraDirector::new(1, CameraTuning::default());
    director.step(&snapshot, &[ground_impact(defender)]);
    assert_ne!(
        director.effective_mode(),
        CameraMode::Impact,
        "a defender's whiff does not steal focus"
    );
    assert_eq!(
        director.active_impulses(),
        0,
        "and does not shake the frame"
    );

    // The ball carrier (offense) going down still earns the emphasis.
    let mut carrier_cam = CameraDirector::new(1, CameraTuning::default());
    carrier_cam.step(&snapshot, &[ground_impact(offender)]);
    assert_eq!(
        carrier_cam.effective_mode(),
        CameraMode::Impact,
        "the carrier's landing cuts to the impact camera"
    );
    assert!(carrier_cam.active_impulses() > 0, "and shakes the frame");
}

#[test]
fn camera_impulses_decay_exactly_to_zero() {
    let mut stack = ImpulseStack::new();
    stack.push(CameraImpulse::seeded(
        42,
        Vec3::new(0.3, 1.0, 0.1),
        0.8,
        6.0,
        20,
    ));
    let mut saw_motion = false;
    let mut last = ImpulseSample::ZERO;
    for _ in 0..21 {
        last = stack.step();
        saw_motion |= last.eye_offset.length() > 0.0 || last.fov_kick > 0.0;
    }
    assert!(saw_motion, "the impulse visibly shook");
    assert_eq!(
        last,
        ImpulseSample::ZERO,
        "the FINAL sampled contribution is exactly zero"
    );
    assert_eq!(stack.active(), 0, "expired impulses are removed");
    assert_eq!(
        stack.step(),
        ImpulseSample::ZERO,
        "the stack contributes EXACTLY zero after expiry"
    );
}

#[test]
fn impulse_amplitudes_are_clamped_and_the_stack_is_bounded() {
    let huge = CameraImpulse::seeded(7, Vec3::UNIT_Y, 999.0, 999.0, 30);
    assert!(huge.amplitude <= MAX_AMPLITUDE);
    assert!(huge.fov_kick <= MAX_FOV_KICK);
    let mut stack = ImpulseStack::new();
    for seed in 0..(MAX_IMPULSES as u64 + 6) {
        stack.push(CameraImpulse::seeded(seed, Vec3::UNIT_Y, 0.5, 1.0, 30));
    }
    assert!(
        stack.active() <= MAX_IMPULSES,
        "the stack never grows past its cap"
    );
}

#[test]
fn additive_shake_does_not_alter_the_base_camera_rig() {
    // Once every impulse has expired, the FINAL pose equals the impulse-free
    // base pose bit-for-bit — shake added exactly zero permanent drift.
    let mut run = ShowcaseRun::new(EndZoneConfig::default());
    let mut post_impact_checked = false;
    let mut had_impulses = false;
    for _ in 0..1400 {
        let out = run.step(&[]);
        had_impulses |= run.director.active_impulses() > 0;
        if had_impulses && run.director.active_impulses() == 0 {
            let base = run.director.base_pose();
            assert_eq!(out.camera.eye, base.eye);
            assert_eq!(out.camera.target, base.target);
            assert_eq!(out.camera.fov_degrees, base.fov_degrees);
            post_impact_checked = true;
        }
    }
    assert!(
        post_impact_checked,
        "the run reached a quiet post-impact frame"
    );
}

#[test]
fn replaying_the_same_event_stream_produces_the_same_camera_poses() {
    let a = run_trace(EndZoneConfig::default(), 700);
    let b = run_trace(EndZoneConfig::default(), 700);
    assert_eq!(a.camera_poses, b.camera_poses);
    assert_eq!(a.camera_modes, b.camera_modes);
}

// --- juice ------------------------------------------------------------------

#[test]
fn effects_are_bounded_clamped_deterministic_and_expiring() {
    let run_effects = |seed: u64| {
        let mut run = ShowcaseRun::new(EndZoneConfig::with_seed(seed));
        let mut per_tick: Vec<Vec<EffectInstance>> = Vec::new();
        let mut peak = 0usize;
        // Effects are transient: once spawned they decay back to none in the
        // quiet stretch between plays. (Checking for a return-to-empty is robust
        // to the play cadence; asserting empty at an exact tick is not.)
        let mut spawned = false;
        let mut cleared_after_spawn = false;
        for _ in 0..900 {
            let out = run.step(&[]);
            let mut instances = Vec::new();
            for effect in run.juice.effects() {
                assert!(effect.strength <= 1.0, "strength clamped at spawn");
                effect_instances(
                    effect,
                    out.snapshot.tick,
                    run.juice.tuning(),
                    &mut instances,
                );
            }
            let live = run.juice.effects().len();
            peak = peak.max(live);
            spawned |= live > 0;
            cleared_after_spawn |= spawned && live == 0;
            per_tick.push(instances);
        }
        assert!(peak > 0, "the showcase spawned effects");
        assert!(
            peak <= run.juice.tuning().max_effects,
            "the effect pool is bounded"
        );
        assert!(
            cleared_after_spawn,
            "effects expire — the pool returns to empty between plays"
        );
        per_tick
    };
    let a = run_effects(EndZoneConfig::default().seed);
    let b = run_effects(EndZoneConfig::default().seed);
    assert_eq!(a, b, "the same impact events produce the same effects");
}

#[test]
fn squash_and_field_wobble_decay_exactly_to_zero() {
    let mut run = ShowcaseRun::new(EndZoneConfig::default());
    let mut peak_wobble = 0.0f32;
    for _ in 0..1400 {
        let out = run.step(&[]);
        peak_wobble = peak_wobble.max(run.juice.field_wobble(out.snapshot.tick).abs());
    }
    assert!(peak_wobble > 0.0, "the ground impact wobbled the field");
    assert!(
        peak_wobble <= run.juice.tuning().field_wobble_amplitude + 1.0e-6,
        "wobble amplitude is clamped"
    );
    let final_tick = run.sim.tick;
    assert_eq!(
        run.juice.field_wobble(final_tick),
        0.0,
        "wobble is EXACTLY zero after expiry"
    );
    for id in 0..14u8 {
        assert_eq!(
            run.juice
                .squash_for(axiom_end_zone::identity::PlayerId(id), final_tick),
            0.0,
            "squash is EXACTLY zero after expiry"
        );
    }
}

#[test]
fn presentation_effects_do_not_mutate_simulation_state() {
    // Run A: untouched. Run B: diagnostic camera forcing + debug overlays
    // toggling all the way through. The authoritative digests must match.
    let mut a = ShowcaseRun::new(EndZoneConfig::default());
    let mut b = ShowcaseRun::new(EndZoneConfig::default());
    for t in 0..700u64 {
        a.step(&[]);
        let noise: &[DiagnosticCommand] = match t % 5 {
            0 => &[DiagnosticCommand::ToggleDebug],
            1 => &[DiagnosticCommand::ForceFormationCamera],
            2 => &[DiagnosticCommand::ForceQuarterbackCamera],
            3 => &[DiagnosticCommand::AutomaticCamera],
            _ => &[],
        };
        b.step(noise);
    }
    assert_eq!(
        a.sim.digest(),
        b.sim.digest(),
        "presentation input never touches the sim"
    );
}
