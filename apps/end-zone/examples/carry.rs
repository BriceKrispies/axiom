//! Headless inspector for one End Zone **carry**.
//!
//! Runs the real game — the real attempt loop, the real AI, the real contact
//! framework — with no browser and no scene attached, driving the running back
//! with the deterministic autopilot policy, and prints what happened tick by
//! tick: the phase, the exchange, every move committed, every success signal,
//! and the whistle. It is how a change to the run game is read from a terminal
//! instead of inferred from the camera.
//!
//! ```sh
//! cargo run -p axiom-end-zone --example carry -- [--seed N] [--carries N] [--verbose]
//! ```

use axiom_end_zone::attempt::AttemptPhase;
use axiom_end_zone::autopilot::{call_play, decide_move, Aggression};
use axiom_end_zone::events::SimEvent;
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::showcase::ShowcaseRun;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str, default: u64| -> u64 {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let seed = value("--seed", 7);
    let carries = value("--carries", 3);
    let verbose = args.iter().any(|a| a == "--verbose");

    let config = RunConfig::new(seed);
    let mut run = ShowcaseRun::new_run(&config);
    let mut resolved = 0u64;
    let mut last_phase = None;

    println!("== End Zone carry inspector  seed {seed} ==");
    for _ in 0..(carries * 900) {
        let step = run.attempt().expect("a session always has an attempt view");
        if let Some(play) = call_play(&step) {
            run.select_concept(play);
        }
        if let Some(wanted) = decide_move(&run.sim, &step, Aggression::BALANCED) {
            run.command(wanted);
        }
        if last_phase != Some(step.phase) {
            println!("t{:>5}  phase {}", run.sim.tick, step.phase.label());
            last_phase = Some(step.phase);
        }
        let out = run.step(&[]);
        for stamped in &out.events {
            match stamped.event {
                SimEvent::Handoff { back, .. } => {
                    println!("t{:>5}  HANDOFF -> player {}", stamped.tick, back.0)
                }
                SimEvent::RunbackMove {
                    move_code, speed, ..
                } => {
                    if verbose {
                        println!(
                            "t{:>5}  move {:<11} at {speed:.1} yd/s",
                            stamped.tick,
                            move_code.label()
                        );
                    }
                }
                SimEvent::TackleDodged { defender, gap, .. } => println!(
                    "t{:>5}  *** DODGED player {} (gap {gap:.2} yd)",
                    stamped.tick, defender.0
                ),
                SimEvent::DefenderHurdled {
                    defender,
                    clearance,
                    ..
                } => println!(
                    "t{:>5}  *** HURDLED player {} (clearance {clearance:.2} yd)",
                    stamped.tick, defender.0
                ),
                SimEvent::TackleBroken {
                    defender,
                    impulse,
                    resistance,
                    ..
                } => println!(
                    "t{:>5}  *** BROKE TACKLE player {} ({impulse:.2} vs {resistance:.2})",
                    stamped.tick, defender.0
                ),
                SimEvent::ChargeStuffed {
                    defender,
                    impulse,
                    resistance,
                    ..
                } => println!(
                    "t{:>5}  --- charge stuffed by player {} ({impulse:.2} vs {resistance:.2})",
                    stamped.tick, defender.0
                ),
                SimEvent::PlayEnded { reason } => {
                    println!("t{:>5}  whistle: {reason:?}", stamped.tick)
                }
                _ => {}
            }
        }
        if matches!(run.attempt().map(|s| s.phase), Some(AttemptPhase::Resolving)) {
            resolved += 1;
            if let Some(ledger) = run.ledger() {
                if let Some(last) = ledger.last {
                    println!(
                        "        carry {} -> {} {:+.1} yd   {} dodge {} broke {} over",
                        last.index,
                        last.outcome.label(),
                        last.yards,
                        last.dodges,
                        last.broken,
                        last.hurdled
                    );
                }
            }
            if resolved >= carries {
                break;
            }
        }
    }
    if let Some(ledger) = run.ledger() {
        println!(
            "== {} carries, {:.1} yd/carry, {} TD, {} moves ==",
            ledger.attempts,
            ledger.yards_per_attempt(),
            ledger.touchdowns,
            ledger.moves()
        );
    }
}
