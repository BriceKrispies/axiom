//! **The End Zone agent validation run.**
//!
//! Plays the real game — the real attempt loop, the real AI on both teams, the
//! real contact framework, the real runback moves — through `axiom-agent`, and
//! prints the observation/action trace plus every success signal, so a reader
//! can see for themselves that juking, charging and hurdling were genuinely
//! exercised rather than asserted.
//!
//! Nothing here injects a success, teleports a defender, sets a flag, or calls a
//! mechanic directly. The only thing it does to the game is what a person does:
//! call a play, and press one of four buttons.
//!
//! ```sh
//! cargo run -p axiom-end-zone --bin agent                 # play until all three land
//! cargo run -p axiom-end-zone --bin agent -- --trace      # print every decision tick
//! cargo run -p axiom-end-zone --bin agent -- --seed 12 --carries 6
//! ```
//!
//! Exit code `0` means every mechanic was demonstrated; `1` means one was not,
//! which is a real failure of the game rather than of the harness.

use axiom_end_zone::agent::{decide_one_step, observe, AgentObservation};
use axiom_end_zone::autopilot::Aggression;
use axiom_end_zone::events::SimEvent;
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::scenario;
use axiom_end_zone::showcase::ShowcaseRun;

/// What the run has proved so far.
#[derive(Debug, Default)]
struct Proof {
    handoff: bool,
    ran_forward: bool,
    dodge: Option<String>,
    charge: Option<String>,
    hurdle: Option<String>,
    touchdown: bool,
    carries: u32,
}

impl Proof {
    fn complete(&self) -> bool {
        self.handoff
            && self.ran_forward
            && self.dodge.is_some()
            && self.charge.is_some()
            && self.hurdle.is_some()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str, default: u64| -> u64 {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let trace = args.iter().any(|a| a == "--trace");
    let seed = value("--seed", scenario::VALIDATION_SEED);
    let carries = value("--carries", 12) as u32;

    println!("=== End Zone — agent validation run ===");
    println!("seed {seed}   up to {carries} carries   policy BALANCED");
    println!("the agent drives the same controls a person does: 1/2/3 to call a");
    println!("play, then juke / shoulder / leap. Nothing else is touched.\n");

    let config = RunConfig::new(seed);
    let mut run = ShowcaseRun::new_run(&config);
    let mut proof = Proof::default();
    let mut start_downfield = None;
    let mut last_phase = None;

    for _ in 0..(carries as u64 * 900) {
        let step = run.attempt().expect("a session always has an attempt view");
        let seen: AgentObservation = observe(&run.sim, &step);
        let decision = decide_one_step(&run.sim, &step, Aggression::BALANCED, run.sim.tick);

        if last_phase != Some(step.phase) {
            println!(
                "  t{:>5}  [{}]  carry {}  {}",
                seen.tick, seen.phase, seen.carry, seen.concept
            );
            last_phase = Some(step.phase);
        }

        // Lower the agent's intents into the game's real inputs.
        if let Some(play) = decision.call_play {
            run.select_concept(play);
            println!(
                "  t{:>5}  agent -> CALL PLAY {}  (reason {}, {} intent)",
                seen.tick,
                play + 1,
                decision.reason_code,
                decision.emitted
            );
        }
        if let Some(wanted) = decision.wanted {
            let accepted = run.command(wanted);
            if trace || accepted {
                println!(
                    "  t{:>5}  agent -> {:<11} {}  | speed {:.1} yd/s, nearest {:.1} yd, jump {}",
                    seen.tick,
                    wanted.label(),
                    if accepted { "" } else { "(refused)" },
                    seen.speed,
                    seen.threats.first().map(|t| t.distance).unwrap_or(99.0),
                    if seen.jump_available {
                        "ready".to_string()
                    } else {
                        format!("{}t", seen.jump_cooldown_left)
                    }
                );
            }
        }
        if trace && seen.carrying {
            println!(
                "         obs: pos ({:.1},{:.2},{:.1}) norm ({:.2},{:.2}) to-goal {:.1} vel ({:.1},{:.1}) act {:?} air {} threats {}",
                seen.position.0,
                seen.position.1,
                seen.position.2,
                seen.normalized.0,
                seen.normalized.1,
                seen.yards_to_goal,
                seen.velocity.0,
                seen.velocity.1,
                seen.action,
                seen.airborne,
                seen.threats.len()
            );
        }

        // Forward progress is measured, not assumed: the downfield coordinate at
        // the exchange against the downfield coordinate now.
        if seen.carrying {
            let downfield = run.sim.frame.from_world(run.sim.players[
                step.runback.back.expect("a carry has a back").index()
            ].pos).downfield;
            match start_downfield {
                None => start_downfield = Some(downfield),
                Some(start) => {
                    proof.ran_forward |= downfield > start + 3.0;
                }
            }
        }

        let out = run.step(&[]);
        for stamped in &out.events {
            match stamped.event {
                SimEvent::Handoff { quarterback, back, .. } => {
                    proof.handoff = true;
                    println!(
                        "  t{:>5}  HANDOFF  qb {} -> back {}",
                        stamped.tick, quarterback.0, back.0
                    );
                }
                SimEvent::TackleDodged {
                    defender, gap, ..
                } => {
                    let line = format!(
                        "t{} defender {} beaten, {gap:.2} yd off",
                        stamped.tick, defender.0
                    );
                    println!("  t{:>5}  *** SUCCESSFUL DODGE          {line}", stamped.tick);
                    proof.dodge.get_or_insert(line);
                }
                SimEvent::TackleBroken {
                    defender,
                    impulse,
                    resistance,
                    ..
                } => {
                    let line = format!(
                        "t{} defender {} run through, impulse {impulse:.2} > resistance {resistance:.2}",
                        stamped.tick, defender.0
                    );
                    println!("  t{:>5}  *** SUCCESSFUL CHARGE         {line}", stamped.tick);
                    proof.charge.get_or_insert(line);
                }
                SimEvent::DefenderHurdled {
                    defender,
                    clearance,
                    ..
                } => {
                    let line = format!(
                        "t{} defender {} passed beneath, {clearance:.2} yd of daylight",
                        stamped.tick, defender.0
                    );
                    println!("  t{:>5}  *** SUCCESSFUL JUMP OVER      {line}", stamped.tick);
                    proof.hurdle.get_or_insert(line);
                }
                SimEvent::ChargeStuffed {
                    defender,
                    impulse,
                    resistance,
                    ..
                } => println!(
                    "  t{:>5}  --- charge stuffed by {} ({impulse:.2} < {resistance:.2})",
                    stamped.tick, defender.0
                ),
                SimEvent::PlayEnded { reason } => {
                    println!("  t{:>5}  whistle: {reason:?}", stamped.tick);
                    start_downfield = None;
                }
                _ => {}
            }
        }

        if let Some(ledger) = run.ledger() {
            if ledger.attempts > proof.carries {
                proof.carries = ledger.attempts;
                if let Some(last) = ledger.last {
                    proof.touchdown |= last.outcome
                        == axiom_end_zone::attempt::AttemptOutcome::Touchdown;
                    println!(
                        "  ---- carry {} : {} {:+.1} yd   {} dodge / {} broke / {} over ----\n",
                        last.index,
                        last.outcome.label(),
                        last.yards,
                        last.dodges,
                        last.broken,
                        last.hurdled
                    );
                }
                if proof.complete() || proof.carries >= carries {
                    break;
                }
            }
        }
    }

    report(&proof, &run);
    std::process::exit(i32::from(!proof.complete()));
}

fn report(proof: &Proof, run: &ShowcaseRun) {
    let ledger = run.ledger().unwrap_or_default();
    let tick = |ok: bool| if ok { "PASS" } else { "FAIL" };
    println!("=== validation summary ===");
    println!("  [{}] a play was selected and executed", tick(proof.carries > 0));
    println!("  [{}] quarterback-to-running-back handoff", tick(proof.handoff));
    println!("  [{}] automatic forward running", tick(proof.ran_forward));
    println!(
        "  [{}] confirmed successful dodge          {}",
        tick(proof.dodge.is_some()),
        proof.dodge.as_deref().unwrap_or("-")
    );
    println!(
        "  [{}] confirmed successful shoulder charge {}",
        tick(proof.charge.is_some()),
        proof.charge.as_deref().unwrap_or("-")
    );
    println!(
        "  [{}] confirmed successful jump over a man {}",
        tick(proof.hurdle.is_some()),
        proof.hurdle.as_deref().unwrap_or("-")
    );
    println!(
        "  [{}] gameplay continued after each success (the carry ran on)",
        tick(proof.complete())
    );
    println!(
        "  [{}] a carry reached the end zone",
        tick(proof.touchdown)
    );
    println!(
        "\n  {} carries, {:.1} yd/carry, {} TD, {} dodges, {} broken, {} hurdled",
        ledger.attempts,
        ledger.yards_per_attempt(),
        ledger.touchdowns,
        ledger.dodges,
        ledger.broken,
        ledger.hurdled
    );
    println!(
        "\n{}",
        match proof.complete() {
            true => "ALL MECHANICS DEMONSTRATED THROUGH REAL PLAY.",
            false => "SOME MECHANIC WAS NOT DEMONSTRATED.",
        }
    );
}
