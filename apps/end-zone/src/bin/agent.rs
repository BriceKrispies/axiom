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
//! cargo run -p axiom-end-zone --bin agent                # play until all three land
//! cargo run -p axiom-end-zone --bin agent -- --trace     # every decision, with the
//!                                                        # charge contest's terms
//! cargo run -p axiom-end-zone --bin agent -- --ab        # A/B: does the down
//!                                                        # button change anything?
//! ```
//!
//! Exit code `0` means every mechanic was demonstrated; `1` means one was not,
//! which is a real failure of the game rather than of the harness.

use axiom_end_zone::agent::{self, DEFAULT_REACTION_MILLIS};
use axiom_end_zone::attempt::AttemptOutcome;
use axiom_end_zone::autopilot::Aggression;
use axiom_end_zone::events::{RunbackMoveCode, SimEvent};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::scenario;
use axiom_end_zone::showcase::ShowcaseRun;

/// What a run proved, and what it measured.
#[derive(Debug, Default, Clone)]
struct Tally {
    handoff: bool,
    ran_forward: bool,
    dodge: Option<String>,
    charge: Option<String>,
    hurdle: Option<String>,
    carries: u32,
    touchdowns: u32,
    tackled: u32,
    yards: f32,
    /// Down-button presses the simulation accepted.
    charges_thrown: u32,
    charges_won: u32,
    charges_lost: u32,
    /// Tackles the carrier shed vs tackles that landed.
    sheds: u32,
    tackles: u32,
    dodges: u32,
    hurdles: u32,
}

impl Tally {
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
    let ab = args.iter().any(|a| a == "--ab");
    let seed = value("--seed", scenario::VALIDATION_SEED);
    let reaction = value("--reaction-ms", u64::from(DEFAULT_REACTION_MILLIS)) as u32;
    let carries = value("--carries", if ab { 20 } else { 12 }) as u32;

    if ab {
        ab_test(seed, carries, reaction);
        return;
    }

    println!("=== End Zone — agent validation run ===");
    println!(
        "seed {seed}   up to {carries} carries   policy BALANCED   reaction {reaction} ms"
    );
    println!("the agent drives the same controls a person does: 1/2/3 to call a");
    println!("play, then juke / shoulder / leap. Nothing else is touched.\n");

    let tally = play(seed, carries, Aggression::BALANCED, reaction, trace, true);
    report(&tally);
    std::process::exit(i32::from(!tally.complete()));
}

/// The A/B the down button deserves: the identical seed, the identical agent,
/// the identical everything — except that one arm may press shoulder-charge and
/// the other never does.
///
/// This is the measurement that catches a dead control. A button whose presence
/// does not move a single number is not a mechanic, and the only way to know is
/// to run the game with it and without it and put the two columns next to each
/// other.
fn ab_test(seed: u64, carries: u32, reaction: u32) {
    println!("=== End Zone — shoulder-charge A/B ===");
    println!(
        "seed {seed}, {carries} carries per arm, identical agent ({reaction} ms reaction) and defense."
    );
    println!("The ONLY difference is whether the down button may be pressed.\n");

    let with = play(seed, carries, Aggression::BALANCED, reaction, false, false);
    let without = play(seed, carries, Aggression::NO_SHOULDER, reaction, false, false);

    let row = |label: &str, a: String, b: String| println!("  {label:<26} {a:>14} {b:>14}");
    println!("  {:<26} {:>14} {:>14}", "", "WITH down", "WITHOUT down");
    println!("  {}", "-".repeat(56));
    row(
        "carries",
        with.carries.to_string(),
        without.carries.to_string(),
    );
    row(
        "charges thrown",
        with.charges_thrown.to_string(),
        without.charges_thrown.to_string(),
    );
    row(
        "  of those, won / lost",
        format!("{} / {}", with.charges_won, with.charges_lost),
        format!("{} / {}", without.charges_won, without.charges_lost),
    );
    row(
        "tackled",
        format!(
            "{} ({:.0}%)",
            with.tackled,
            100.0 * with.tackled as f32 / with.carries.max(1) as f32
        ),
        format!(
            "{} ({:.0}%)",
            without.tackled,
            100.0 * without.tackled as f32 / without.carries.max(1) as f32
        ),
    );
    row(
        "touchdowns",
        with.touchdowns.to_string(),
        without.touchdowns.to_string(),
    );
    row(
        "yards per carry",
        format!("{:.1}", with.yards / with.carries.max(1) as f32),
        format!("{:.1}", without.yards / without.carries.max(1) as f32),
    );
    row(
        "tackles shed / landed",
        format!("{} / {}", with.sheds, with.tackles),
        format!("{} / {}", without.sheds, without.tackles),
    );
    row(
        "dodges / hurdles",
        format!("{} / {}", with.dodges, with.hurdles),
        format!("{} / {}", without.dodges, without.hurdles),
    );

    let moved = with.charges_won > 0 && with.tackled != without.tackled;
    println!(
        "\n{}",
        match moved {
            true => "The down button changes the game: charges are thrown, some are won, \
                     and the tackled count differs between the two arms.",
            false => "The down button did NOT change the outcome — it is not yet a mechanic.",
        }
    );
    std::process::exit(i32::from(!moved));
}

/// Play `carries` carries under `policy`, returning what happened.
///
/// The loop itself lives in [`axiom_end_zone::agent::drive`] — it has to, because
/// the reaction delay line cannot be named outside `axiom-agent`'s facade and so
/// must share a scope with the tick loop. What is left here is what belongs in a
/// binary: watching, tallying and printing.
fn play(
    seed: u64,
    carries: u32,
    policy: Aggression,
    reaction_millis: u32,
    trace: bool,
    narrate: bool,
) -> Tally {
    let config = RunConfig::new(seed);
    let mut run = ShowcaseRun::new_run(&config);
    let mut tally = Tally::default();
    let mut start_downfield: Option<f32> = None;
    let mut last_phase = None;
    let mut counted = 0u32;

    agent::drive(&mut run, policy, reaction_millis, carries, &mut |report| {
        let seen = report.observation;
        if narrate && last_phase != Some(report.phase) {
            println!(
                "  t{:>5}  [{}]  carry {}  {}",
                seen.tick, seen.phase, seen.carry, seen.concept
            );
            last_phase = Some(report.phase);
        }
        if narrate {
            if let Some(play) = report.decision.call_play {
                println!(
                    "  t{:>5}  agent -> CALL PLAY {}  (reason {}, {} intent)",
                    seen.tick,
                    play + 1,
                    report.decision.reason_code,
                    report.decision.emitted
                );
            }
            if let Some(wanted) = report.decision.wanted {
                println!(
                    "  t{:>5}  agent -> {:<11} | speed {:.1} yd/s, nearest {:.1} yd, jump {}",
                    seen.tick,
                    wanted.label(),
                    seen.speed,
                    seen.threats.first().map(|t| t.distance).unwrap_or(99.0),
                    match seen.jump_available {
                        true => "ready".to_string(),
                        false => format!("{}t", seen.jump_cooldown_left),
                    }
                );
            }
        }
        if trace {
            // Why the tell is or is not open, every tick there is anybody to
            // charge. Printing only the successes tells you nothing about the
            // failures, which is the thing that needed diagnosing.
            if let Some(enc) = report.encounter {
                if enc.gap <= 9.0 {
                    let c = enc.predicted_charge;
                    println!(
                        "         gap {:5.2} meet {:>5} | close {:5.2} align {:4.2} time {:4.2}                          brace {:4.2} | {:5.2} vs {:5.2} {} | tell {}",
                        enc.gap,
                        match enc.contact_in_ticks {
                            Some(t) => format!("{t}t"),
                            None => "none".to_string(),
                        },
                        c.closing_speed,
                        c.alignment,
                        c.timing,
                        c.brace,
                        c.impulse,
                        c.resistance,
                        if c.won { "WIN " } else { "lose" },
                        if seen.charge_target.is_some() { "ON" } else { "-" },
                    );
                }
            }
        }

        // Forward progress is measured, not assumed.
        if seen.carrying {
            match start_downfield {
                None => start_downfield = Some(seen.position.2),
                Some(start) => {
                    tally.ran_forward |= (seen.position.2 - start).abs() > 3.0;
                }
            }
        }

        for stamped in report.events {
            record(&mut tally, stamped.tick, stamped.event, narrate);
            if matches!(stamped.event, SimEvent::PlayEnded { .. }) {
                start_downfield = None;
            }
        }

        if report.ledger.attempts > counted {
            counted = report.ledger.attempts;
            tally.carries = counted;
            if let Some(last) = report.ledger.last {
                tally.touchdowns += u32::from(last.outcome == AttemptOutcome::Touchdown);
                tally.tackled += u32::from(last.outcome == AttemptOutcome::Tackled);
                tally.yards += last.yards;
                if narrate {
                    println!(
                        "  ---- carry {} : {} {:+.1} yd   {} dodge / {} broke / {} over ----
",
                        last.index,
                        last.outcome.label(),
                        last.yards,
                        last.dodges,
                        last.broken,
                        last.hurdled
                    );
                }
            }
        }
    });
    tally
}

/// Fold one simulation event into the tally (and narrate it, when asked).
fn record(tally: &mut Tally, tick: u64, event: SimEvent, narrate: bool) {
    match event {
        SimEvent::Handoff {
            quarterback, back, ..
        } => {
            tally.handoff = true;
            if narrate {
                println!("  t{tick:>5}  HANDOFF  qb {} -> back {}", quarterback.0, back.0);
            }
        }
        SimEvent::RunbackMove {
            move_code: RunbackMoveCode::Shoulder,
            ..
        } => tally.charges_thrown += 1,
        SimEvent::TackleDodged { defender, gap, .. } => {
            tally.dodges += 1;
            let line = format!("t{tick} defender {} beaten, {gap:.2} yd off", defender.0);
            if narrate {
                println!("  t{tick:>5}  *** SUCCESSFUL DODGE          {line}");
            }
            tally.dodge.get_or_insert(line);
        }
        SimEvent::TackleBroken {
            defender,
            impulse,
            resistance,
            ..
        } => {
            tally.charges_won += 1;
            let line = format!(
                "t{tick} defender {} run through, impulse {impulse:.2} > resistance {resistance:.2}",
                defender.0
            );
            if narrate {
                println!("  t{tick:>5}  *** SUCCESSFUL CHARGE         {line}");
            }
            tally.charge.get_or_insert(line);
        }
        SimEvent::DefenderHurdled {
            defender,
            clearance,
            ..
        } => {
            tally.hurdles += 1;
            let line = format!(
                "t{tick} defender {} passed beneath, {clearance:.2} yd of daylight",
                defender.0
            );
            if narrate {
                println!("  t{tick:>5}  *** SUCCESSFUL JUMP OVER      {line}");
            }
            tally.hurdle.get_or_insert(line);
        }
        SimEvent::ChargeStuffed {
            defender,
            impulse,
            resistance,
            ..
        } => {
            tally.charges_lost += 1;
            if narrate {
                println!(
                    "  t{tick:>5}  --- charge stuffed by {} ({impulse:.2} < {resistance:.2})",
                    defender.0
                );
            }
        }
        SimEvent::TackleShed {
            tackler,
            impulse,
            resistance,
            balance_left,
            ..
        } => {
            tally.sheds += 1;
            if narrate {
                println!(
                    "  t{tick:>5}  ~~~ SHED a tackle from {}  ({impulse:.2} < {resistance:.2}, \
                     balance now {balance_left:.2})",
                    tackler.0
                );
            }
        }
        SimEvent::TackleContact {
            tackler, strength, ..
        } => {
            tally.tackles += 1;
            if narrate {
                println!("  t{tick:>5}  ### TACKLED by {} (strength {strength:.2})", tackler.0);
            }
        }
        SimEvent::PlayEnded { reason } => {
            if narrate {
                println!("  t{tick:>5}  whistle: {reason:?}");
            }
        }
        _ => {}
    }
}

fn report(tally: &Tally) {
    let mark = |ok: bool| if ok { "PASS" } else { "FAIL" };
    println!("=== validation summary ===");
    println!(
        "  [{}] a play was selected and executed",
        mark(tally.carries > 0)
    );
    println!(
        "  [{}] quarterback-to-running-back handoff",
        mark(tally.handoff)
    );
    println!("  [{}] automatic forward running", mark(tally.ran_forward));
    println!(
        "  [{}] confirmed successful dodge           {}",
        mark(tally.dodge.is_some()),
        tally.dodge.as_deref().unwrap_or("-")
    );
    println!(
        "  [{}] confirmed successful shoulder charge {}",
        mark(tally.charge.is_some()),
        tally.charge.as_deref().unwrap_or("-")
    );
    println!(
        "  [{}] confirmed successful jump over a man {}",
        mark(tally.hurdle.is_some()),
        tally.hurdle.as_deref().unwrap_or("-")
    );
    println!(
        "  [{}] contact is a contest: {} tackles shed vs {} landed",
        mark(tally.sheds > 0 && tally.tackles > 0),
        tally.sheds,
        tally.tackles
    );
    println!("  [{}] a carry reached the end zone", mark(tally.touchdowns > 0));
    println!(
        "\n  {} carries, {:.1} yd/carry, {} TD, {} tackled",
        tally.carries,
        tally.yards / tally.carries.max(1) as f32,
        tally.touchdowns,
        tally.tackled
    );
    println!(
        "  moves: {} dodges, {} charges thrown ({} won / {} lost), {} hurdles",
        tally.dodges, tally.charges_thrown, tally.charges_won, tally.charges_lost, tally.hurdles
    );
    println!(
        "\n{}",
        match tally.complete() {
            true => "ALL MECHANICS DEMONSTRATED THROUGH REAL PLAY.",
            false => "SOME MECHANIC WAS NOT DEMONSTRATED.",
        }
    );
}
