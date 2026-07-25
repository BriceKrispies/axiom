//! The headless autopilot plays a real score-attack run through to a touchdown.
//!
//! This is the proof that End Zone can be driven end-to-end with no human at the
//! controls: the deterministic user-slot policy (`autopilot::steer` +
//! `autopilot::should_throw`) calls a play, reads the field, throws to an open
//! receiver, and runs it into the end zone — replaying bit-for-bit every time.
//!
//! Watch a run play out down-by-down with:
//!   cargo test -p axiom-end-zone --test autopilot autopilot_one -- --ignored --nocapture
//! Sweep many seeds with:
//!   cargo test -p axiom-end-zone --test autopilot autopilot_sweep -- --ignored --nocapture

use axiom_end_zone::autopilot;
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::showcase::{DiagnosticCommand, ShowcaseRun};

/// The default showcase seed, which scores in three plays.
const DEFAULT_SEED: u64 = 0x51A7_0E2D;
/// A generous per-run tick budget (a scored run needs well under this).
const TICK_CAP: usize = 30_000;

struct Outcome {
    seed: u64,
    plays: u32,
    scored: bool,
    over: bool,
    max_yard: f32,
    ticks: usize,
}

/// Play one autopiloted run until it scores, the run ends, or the tick cap.
/// With `verbose`, prints a down-by-down log.
fn play_run(seed: u64, verbose: bool) -> Outcome {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(seed));
    let mut prev = run.drive_state().expect("a real run has drive state");
    let mut plays = 0u32;
    let mut max_yard = 0.0f32;
    let mut threw = false;
    for tick in 0..TICK_CAP {
        // Call the default play whenever the huddle opens; let the snap auto-fire.
        if run.huddle().is_some() {
            run.call_play(0);
        }
        run.sim.user_stick = autopilot::steer(&run.sim);
        let cmds: &[DiagnosticCommand] = if autopilot::should_throw(&run.sim) {
            threw = true;
            &[DiagnosticCommand::PrimaryAction]
        } else {
            &[]
        };
        run.step(cmds);
        max_yard = max_yard.max(run.sim.ball_yard_line());
        let d = run.drive_state().expect("a real run has drive state");
        let play_ended = d.down != prev.down
            || (d.los_yard - prev.los_yard).abs() > 0.01
            || d.touchdowns != prev.touchdowns
            || d.over != prev.over;
        if play_ended {
            plays += 1;
            if verbose {
                println!(
                    "  play {plays:2} @tick {tick:5}: down {}->{}  los {:.1}->{:.1}  \
                     td {}  score {}  threw={threw}{}",
                    prev.down,
                    d.down,
                    prev.los_yard,
                    d.los_yard,
                    d.touchdowns,
                    d.score,
                    if d.over { "  RUN OVER" } else { "" }
                );
            }
            threw = false;
            prev = d;
        }
        if d.touchdowns > 0 {
            return Outcome { seed, plays, scored: true, over: d.over, max_yard, ticks: tick };
        }
        if d.over {
            return Outcome { seed, plays, scored: false, over: true, max_yard, ticks: tick };
        }
    }
    Outcome { seed, plays, scored: false, over: prev.over, max_yard, ticks: TICK_CAP }
}

#[test]
fn the_autopilot_scores_a_touchdown() {
    let o = play_run(DEFAULT_SEED, false);
    assert!(o.scored, "the autopilot must reach the end zone");
    assert!(!o.over, "it scores rather than turning the ball over on downs");
    assert!(o.max_yard >= 100.0, "the carrier crossed the goal line");
    assert!(o.ticks < TICK_CAP, "it scored well within the tick budget");
}

#[test]
fn an_autopiloted_run_replays_identically() {
    let digest = |seed| {
        let o = play_run(seed, false);
        (o.scored, o.plays, o.ticks, o.max_yard.to_bits())
    };
    assert_eq!(digest(DEFAULT_SEED), digest(DEFAULT_SEED));
}

#[test]
#[ignore = "diagnostic sweep; run with --ignored --nocapture"]
fn autopilot_sweep() {
    let seeds: Vec<u64> = (0..24).map(|i| 0x51A7_0000 + i * 0x1_0001).collect();
    let mut scored = 0;
    for &seed in &seeds {
        let o = play_run(seed, false);
        println!(
            "seed {:#010x}: plays {:2}  maxYard {:5.1}  ticks {:5}  {}",
            seed,
            o.plays,
            o.max_yard,
            o.ticks,
            if o.scored {
                "TOUCHDOWN"
            } else if o.over {
                "run over"
            } else {
                "capped"
            }
        );
        scored += u32::from(o.scored);
    }
    println!("\nscored on {scored}/{} seeds", seeds.len());
    assert_eq!(scored, seeds.len() as u32, "the autopilot scores on every seed");
}

/// Nearest opposing (can-act) player to `pos`, yards on the ground plane.
fn nearest_defender(run: &ShowcaseRun, team: axiom_end_zone::identity::TeamId, pos: axiom::prelude::Vec3) -> f32 {
    run.sim
        .players
        .iter()
        .filter(|p| p.team != team && p.anim.can_act())
        .map(|p| {
            let dx = p.pos.x - pos.x;
            let dz = p.pos.z - pos.z;
            (dx * dx + dz * dz).sqrt()
        })
        .fold(f32::INFINITY, f32::min)
}

#[test]
#[ignore = "coverage probe; run with --ignored --nocapture"]
fn autopilot_probe() {
    use axiom_end_zone::events::SimEvent;
    let mut run = ShowcaseRun::new_run(&RunConfig::new(DEFAULT_SEED));
    let offense = run.sim.players[run.sim.quarterback.index()].team;
    let mut prev = run.drive_state().expect("drive state");
    let mut sample = 0u32;
    for tick in 0..TICK_CAP {
        if run.huddle().is_some() {
            run.call_play(0);
        }
        run.sim.user_stick = autopilot::steer(&run.sim);
        let cmds: &[DiagnosticCommand] = if autopilot::should_throw(&run.sim) {
            &[DiagnosticCommand::PrimaryAction]
        } else {
            &[]
        };
        let out = run.step(cmds);
        for stamped in &out.events {
            match stamped.event {
                SimEvent::Snap { .. } => {
                    println!("  @tick {tick:5} SNAP   defense call #{:?}", run.last_defense_index());
                }
                SimEvent::Throw { target, .. } => {
                    let open = nearest_defender(&run, offense, target);
                    println!("  @tick {tick:5} THROW  → target yard-spot, nearest defender to it: {open:5.1} yd");
                }
                SimEvent::CatchCompleted { player } => {
                    let pos = run.sim.players[player.index()].pos;
                    let open = nearest_defender(&run, offense, pos);
                    println!("  @tick {tick:5} CATCH  by #{}  nearest defender: {open:5.1} yd  (yard {:.0})", player.0, run.sim.ball_yard_line());
                }
                SimEvent::TackleContact { target, .. } => {
                    println!("  @tick {tick:5} TACKLE on #{}  (yard {:.0})", target.0, run.sim.ball_yard_line());
                }
                SimEvent::Intercepted { defender, .. } => {
                    println!("  @tick {tick:5} INTERCEPTED by #{}", defender.0);
                }
                _ => {}
            }
        }
        // Sample coverage on the ball-carrier every ~12 ticks.
        if let Some(id) = run.sim.controlled_player() {
            sample += 1;
            if sample % 12 == 0 {
                let carrier = run.sim.players[id.index()];
                // Nearest defender: id, distance, speed.
                let (near_id, near_d, near_spd) = run
                    .sim
                    .players
                    .iter()
                    .filter(|p| p.team != offense && p.anim.can_act())
                    .map(|p| {
                        let dx = p.pos.x - carrier.pos.x;
                        let dz = p.pos.z - carrier.pos.z;
                        (p.id.0, (dx * dx + dz * dz).sqrt(), p.speed())
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap_or((0, f32::INFINITY, 0.0));
                // The safety is the last defensive slot (id 13).
                let safety = run.sim.players[13];
                let sdx = safety.pos.x - carrier.pos.x;
                let sdz = safety.pos.z - carrier.pos.z;
                let sdist = (sdx * sdx + sdz * sdz).sqrt();
                println!(
                    "      carry #{:<2} yard {:5.1} spd {:4.1} | nearest #{near_id} {near_d:5.1}yd spd {near_spd:4.1} | safety#13 {sdist:5.1}yd spd {:4.1} anim {:?}",
                    carrier.id.0,
                    run.sim.ball_yard_line(),
                    carrier.speed(),
                    safety.speed(),
                    safety.anim,
                );
            }
        }
        let d = run.drive_state().expect("drive state");
        if d.touchdowns != prev.touchdowns || d.over != prev.over {
            println!("  --- play resolved: td {} over {} ---", d.touchdowns, d.over);
            prev = d;
        }
        if d.touchdowns > 0 || d.over {
            break;
        }
    }
}

#[test]
#[ignore = "verbose single run; run with --ignored --nocapture"]
fn autopilot_one() {
    let o = play_run(DEFAULT_SEED, true);
    println!(
        "\nseed {:#010x}: {} in {} plays, maxYard {:.1}",
        o.seed,
        if o.scored { "TOUCHDOWN" } else { "no score" },
        o.plays,
        o.max_yard
    );
}
