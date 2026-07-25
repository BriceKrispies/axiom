//! Defensive-balance work harness (all `#[ignore]` — diagnostics, not gates).
//!
//! The defense is a coupled AI-balance problem: DB speed and pursuit geometry
//! interact, and every change ripples into the deterministic showcase. These
//! probes measure defensive quality across THREE scenarios so a change can be
//! judged as a real improvement, not a trade:
//!
//!   1. `scripted_completion_probe` — the fixed throw-at-170 showcase pass
//!      (the `ai.rs` scenario). The completion must be RUN DOWN and tackled,
//!      not break free. Shows which defender makes the play and where.
//!   2. `defense_report` — the adversarial autopilot sweep, reported as
//!      quality metrics (TD rate, plays-to-score, stops) rather than pass/fail.
//!      The autopilot is a perfect player, so the target is "competitive," not
//!      "shut out."
//!
//! Run: cargo test -p axiom-end-zone --test defense_balance <name> -- --ignored --nocapture

use axiom_end_zone::autopilot;
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::showcase::{DiagnosticCommand, ShowcaseRun};
use axiom_end_zone::state::{SimCommand, SimState};

/// Planar distance between two players, yards.
fn gap(a: &axiom_end_zone::player::PlayerSim, b: &axiom_end_zone::player::PlayerSim) -> f32 {
    let dx = a.pos.x - b.pos.x;
    let dz = a.pos.z - b.pos.z;
    (dx * dx + dz * dz).sqrt()
}

#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn scripted_completion_probe() {
    // The exact ai.rs scenario: snap at 80, forced throw at 170, no steering.
    let mut sim = SimState::new(EndZoneConfig::default());
    // Secondary ids: corners are defense slots 4/5 (ids 11/12), safety slot 6 (id 13).
    let secondary = [11usize, 12, 13];
    let mut caught = false;
    for t in 0..700u64 {
        let cmds: &[SimCommand] = match t {
            0 => &[SimCommand::BeginPlay],
            80 => &[SimCommand::Snap],
            170 => &[SimCommand::ThrowNow],
            _ => &[],
        };
        let events = sim.step(cmds).to_vec();
        for e in &events {
            let label = format!("{:?}", e.event);
            if label.contains("CatchCompleted") {
                caught = true;
                println!("  @{t} {label}  (carrier yard {:.1})", sim.ball_yard_line());
            } else if label.contains("Tackle") || label.contains("PlayEnded") {
                println!("  @{t} {label}  (yard {:.1})", sim.ball_yard_line());
            }
        }
        if caught && t % 12 == 0 {
            if let Some(carrier) = sim.possession {
                let c = &sim.players[carrier.index()];
                let cover: Vec<String> = secondary
                    .iter()
                    .map(|&i| {
                        format!(
                            "#{i} {:.1}yd spd{:.1} {:?}",
                            gap(&sim.players[i], c),
                            sim.players[i].speed(),
                            sim.players[i].anim
                        )
                    })
                    .collect();
                println!(
                    "     carrier#{} yard{:.1} spd{:.1} | {}",
                    carrier.0,
                    sim.ball_yard_line(),
                    c.speed(),
                    cover.join("  ")
                );
            }
        }
    }
    println!("  end_reason = {:?}", sim.end_reason);
}

#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn sack_probe() {
    use axiom::prelude::Vec3;
    let mut sim = SimState::new(EndZoneConfig::default());
    let qb = sim.quarterback.index();
    let mut dives = 0u32;
    for t in 0..400u64 {
        let cmds: &[SimCommand] = match t {
            0 => &[SimCommand::BeginPlay],
            80 => &[SimCommand::Snap],
            _ => &[],
        };
        sim.step(cmds);
        // How close is the nearest defender to the QB, and is anyone diving at him?
        let qbpos = sim.players[qb].pos;
        let qteam = sim.players[qb].team;
        let (near, diving) = sim.players.iter().filter(|p| p.team != qteam).fold(
            (f32::INFINITY, false),
            |(d, dv), p| {
                let gap = ((p.pos.x - qbpos.x).powi(2) + (p.pos.z - qbpos.z).powi(2)).sqrt();
                (d.min(gap), dv || p.anim == axiom_end_zone::player::AnimState::Dive)
            },
        );
        dives += u32::from(diving);
        if t % 40 == 0 || sim.phase == axiom_end_zone::state::PlayPhase::Ended {
            println!(
                "  t{t:3} phase {:?} qb.anim {:?} nearestDef {near:.1}yd anyDiving={diving}",
                sim.phase, sim.players[qb].anim
            );
        }
        if sim.phase == axiom_end_zone::state::PlayPhase::Ended {
            println!("  SACKED/ended at t{t}, reason {:?}", sim.end_reason);
            break;
        }
    }
    println!("  total dive-ticks against QB region: {dives}");
    let _ = Vec3::ZERO;
}

// NOTE (2026-07-25): `throw_flight_probe` and `throw_probe` were disabled during
// the defense-fix reconstruction. They depended on a SEPARATE piece of lost
// uncommitted work — a throw-physics rework that added `BehaviorTuning::pass_gravity`
// and gave `football::flight::solve_throw` a tuning-taking signature. That field/
// signature does not exist on the committed baseline, so the probes no longer
// compile. They are unrelated to the defensive-breakaway fix; restore them if/when
// the throw-physics work is reconstructed.

#[test]
#[ignore = "diagnostic; run with --ignored --nocapture"]
fn defense_report() {
    let seeds: Vec<u64> = (0..24).map(|i| 0x51A7_0000 + i * 0x1_0001).collect();
    let mut tds = 0u32;
    let mut stops = 0u32;
    let mut total_plays = 0u32;
    let mut scored_plays = 0u32;
    for &seed in &seeds {
        let mut run = ShowcaseRun::new_run(&RunConfig::new(seed));
        let mut prev = run.drive_state().expect("drive");
        let mut plays = 0u32;
        for _ in 0..30_000 {
            if run.huddle().is_some() {
                run.call_play(0);
            }
            run.sim.user_stick = autopilot::steer(&run.sim);
            let cmds: &[DiagnosticCommand] = if autopilot::should_throw(&run.sim) {
                &[DiagnosticCommand::PrimaryAction]
            } else {
                &[]
            };
            run.step(cmds);
            let d = run.drive_state().expect("drive");
            let ended = d.down != prev.down
                || (d.los_yard - prev.los_yard).abs() > 0.01
                || d.touchdowns != prev.touchdowns
                || d.over != prev.over;
            plays += u32::from(ended);
            prev = d;
            if d.touchdowns > 0 || d.over {
                break;
            }
        }
        let d = run.drive_state().expect("drive");
        total_plays += plays;
        tds += u32::from(d.touchdowns > 0);
        stops += u32::from(d.touchdowns == 0);
        if d.touchdowns > 0 {
            scored_plays += plays;
        }
    }
    let n = seeds.len() as u32;
    let avg_to_score = if tds > 0 { scored_plays as f32 / tds as f32 } else { 0.0 };
    println!("\n=== DEFENSE REPORT (autopilot = perfect player) ===");
    println!("  seeds:            {n}");
    println!("  touchdowns:       {tds}/{n}  ({:.0}%)", 100.0 * tds as f32 / n as f32);
    println!("  stops (no TD):    {stops}/{n}");
    println!("  avg plays/run:    {:.1}", total_plays as f32 / n as f32);
    println!("  avg plays/score:  {avg_to_score:.1}");
}
