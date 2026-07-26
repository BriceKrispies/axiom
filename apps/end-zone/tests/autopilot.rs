//! The headless autopilot plays the decision-window prototype end to end.
//!
//! This is two things at once. As a **test** it proves the attempt loop can be
//! driven with no human at the controls, resets reliably, and replays
//! bit-for-bit. As a **tuning instrument** it sweeps [`Patience`] profiles so we
//! can check the prototype's central claim with numbers instead of vibes:
//! waiting for the deeper read must pay MORE and cost MORE. If an impatient and
//! a greedy quarterback post the same yards and the same disaster rate, the
//! trade-off is fake.
//!
//! Watch a session play out attempt-by-attempt with:
//!   cargo test -p axiom-end-zone --test autopilot autopilot_one -- --ignored --nocapture
//! Sweep the patience profiles with:
//!   cargo test -p axiom-end-zone --test autopilot patience_sweep -- --ignored --nocapture

use axiom_end_zone::attempt::{AttemptOutcome, AttemptPhase};
use axiom_end_zone::autopilot::{self, Patience};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::showcase::ShowcaseRun;

/// The default prototype seed.
const DEFAULT_SEED: u64 = 0x51A7_0E2D;
/// Generous per-attempt tick budget (an attempt is ~500 ticks of simulation).
const TICKS_PER_ATTEMPT: usize = 1_200;

/// What a session of autopiloted attempts produced.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Session {
    attempts: u32,
    completions: u32,
    touchdowns: u32,
    interceptions: u32,
    sacks: u32,
    scrambles: u32,
    /// Windows offered, summed over attempts.
    windows: u32,
    /// Attempts where every window was allowed to close.
    declined: u32,
    total_yards: f32,
    best_yards: f32,
    /// How many attempts committed to each read.
    by_read: [u32; 3],
    /// Yards each read produced, summed — the reward gradient the prototype
    /// lives or dies by: read 3 must pay more per completion than read 1.
    yards_by_read: [f32; 3],
    /// Completions per read.
    hits_by_read: [u32; 3],
}

const EMPTY: Session = Session {
    attempts: 0,
    completions: 0,
    touchdowns: 0,
    interceptions: 0,
    sacks: 0,
    scrambles: 0,
    windows: 0,
    declined: 0,
    total_yards: 0.0,
    best_yards: f32::MIN,
    by_read: [0; 3],
    yards_by_read: [0.0; 3],
    hits_by_read: [0; 3],
};

/// Play `attempts` autopiloted attempts under `patience`.
fn play(seed: u64, patience: Patience, attempts: u32, verbose: bool) -> Session {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(seed));
    let mut session = EMPTY;
    let mut seen = 0u32;
    for _ in 0..(attempts as usize + 1) * TICKS_PER_ATTEMPT {
        if let Some(step) = run.attempt() {
            if let Some(choice) = autopilot::decide(&step, patience) {
                run.choose(choice);
            }
        }
        run.set_user_stick(autopilot::steer(&run.sim));
        run.step(&[]);

        let Some(ledger) = run.ledger() else { break };
        let Some(record) = ledger.last else { continue };
        if record.index <= seen {
            continue;
        }
        seen = record.index;
        session.attempts += 1;
        session.completions += u32::from(record.outcome.is_completion());
        session.touchdowns += u32::from(matches!(
            record.outcome,
            AttemptOutcome::Touchdown | AttemptOutcome::ScrambleTouchdown
        ));
        session.interceptions += u32::from(record.outcome == AttemptOutcome::Intercepted);
        session.sacks += u32::from(record.outcome == AttemptOutcome::Sacked);
        session.scrambles += u32::from(matches!(
            record.outcome,
            AttemptOutcome::Scramble | AttemptOutcome::ScrambleTouchdown
        ));
        session.windows += record.windows;
        session.declined += u32::from(record.declined);
        session.total_yards += record.yards;
        session.best_yards = session.best_yards.max(record.yards);
        if let Some(read) = record.read {
            let slot = read.min(2);
            session.by_read[slot] += 1;
            session.yards_by_read[slot] += record.yards;
            session.hits_by_read[slot] += u32::from(record.outcome.is_completion());
        }
        if verbose {
            println!(
                "  attempt {:3}: {:<16} {:+6.1} yd   read {:?}   windows {}{}",
                record.index,
                record.outcome.label(),
                record.yards,
                record.read.map(|r| r + 1),
                record.windows,
                if record.declined {
                    "   DECLINED ALL"
                } else {
                    ""
                }
            );
        }
        if session.attempts >= attempts {
            break;
        }
    }
    session
}

fn yards_per_attempt(s: &Session) -> f32 {
    match s.attempts {
        0 => 0.0,
        n => s.total_yards / n as f32,
    }
}

#[test]
fn the_autopilot_plays_ten_consecutive_attempts_without_stalling() {
    let s = play(DEFAULT_SEED, Patience::BALANCED, 10, false);
    assert_eq!(s.attempts, 10, "ten attempts resolve inside the budget");
    assert!(
        s.windows >= 10,
        "every attempt offered at least one decision window, got {}",
        s.windows
    );
}

#[test]
fn every_attempt_offers_at_least_one_decision_window() {
    // The develop deadline guarantees this: a window opens whether or not the
    // read ever looks good, so the loop can never silently skip the question.
    for seed in [0x0A11_0001u64, 0x0A11_0002, 0x0A11_0003] {
        let s = play(seed, Patience::BALANCED, 6, false);
        assert_eq!(s.attempts, 6, "seed {seed:#x} resolved six attempts");
        assert!(
            s.windows >= s.attempts,
            "seed {seed:#x}: {} windows over {} attempts",
            s.windows,
            s.attempts
        );
    }
}

#[test]
fn an_autopiloted_session_replays_identically() {
    let digest = |seed| {
        let s = play(seed, Patience::BALANCED, 6, false);
        (
            s.attempts,
            s.completions,
            s.interceptions,
            s.sacks,
            s.total_yards.to_bits(),
            s.by_read,
        )
    };
    assert_eq!(digest(DEFAULT_SEED), digest(DEFAULT_SEED));
}

#[test]
fn every_read_is_a_live_option_and_none_is_a_trap() {
    // A read that never completes is not a choice, it is a trap; a read that
    // always completes is not a choice either. This is the check that caught
    // the original 22-yard post, which completed 4% of the time.
    let mut totals = EMPTY;
    for (i, patience) in [Patience::IMPATIENT, Patience::BALANCED, Patience::GREEDY]
        .into_iter()
        .enumerate()
    {
        let s = play(0x5B8E_0000u64.wrapping_add(i as u64), patience, 8, false);
        totals.attempts += s.attempts;
        totals.completions += s.completions;
        for slot in 0..3 {
            totals.by_read[slot] += s.by_read[slot];
            totals.hits_by_read[slot] += s.hits_by_read[slot];
        }
    }
    assert!(
        totals.completions > 0 && totals.completions < totals.attempts,
        "the loop must both complete and fail passes, got {}/{}",
        totals.completions,
        totals.attempts
    );
    let used = totals.by_read.iter().filter(|&&n| n > 0).count();
    assert!(
        used >= 2,
        "more than one read must be worth taking, got {:?}",
        totals.by_read
    );
    for slot in 0..3 {
        if totals.by_read[slot] < 4 {
            continue; // too small a sample to judge
        }
        let rate = totals.hits_by_read[slot] as f32 / totals.by_read[slot] as f32;
        assert!(
            (0.15..=0.95).contains(&rate),
            "read {} completes {:.0}% of the time ({}/{}) — it is a trap or a gimme, \
             not a decision",
            slot + 1,
            rate * 100.0,
            totals.hits_by_read[slot],
            totals.by_read[slot]
        );
    }
}

#[test]
fn the_reads_are_ordered_by_how_long_they_take_to_come_open() {
    // Read 1 must be available before read 3 — otherwise "wait a little longer
    // for the better option" is not the trade the player is making.
    let mut r = ShowcaseRun::new_run(&RunConfig::new(0x0DDE_0001));
    let mut first_broken = [None::<u64>; 3];
    for tick in 0..400u64 {
        if let Some(step) = r.attempt() {
            for slot in 0..3 {
                if first_broken[slot].is_none() && step.read.read(slot).broken {
                    first_broken[slot] = Some(tick);
                }
            }
        }
        r.step(&[]);
    }
    let (Some(one), Some(three)) = (first_broken[0], first_broken[2]) else {
        panic!("both the short and the deep read develop, got {first_broken:?}");
    };
    assert!(
        one < three,
        "the short read must break before the deep one: {one} vs {three}"
    );
}

#[test]
#[ignore = "tuning sweep; run with --ignored --nocapture"]
fn patience_sweep() {
    let seeds: Vec<u64> = (0..8).map(|i| 0x51A7_0000 + i * 0x1_0001).collect();
    println!(
        "{:<10} {:>5} {:>5} {:>5} {:>5} {:>5} {:>7} {:>7}  {:>12}",
        "patience", "att", "comp", "int", "sack", "scrm", "yds/att", "best", "reads 1/2/3"
    );
    for (name, patience) in [
        ("impatient", Patience::IMPATIENT),
        ("balanced", Patience::BALANCED),
        ("greedy", Patience::GREEDY),
    ] {
        let mut total = EMPTY;
        for &seed in &seeds {
            let s = play(seed, patience, 8, false);
            total.attempts += s.attempts;
            total.completions += s.completions;
            total.interceptions += s.interceptions;
            total.sacks += s.sacks;
            total.scrambles += s.scrambles;
            total.windows += s.windows;
            total.declined += s.declined;
            total.total_yards += s.total_yards;
            total.best_yards = total.best_yards.max(s.best_yards);
            for slot in 0..3 {
                total.by_read[slot] += s.by_read[slot];
                total.yards_by_read[slot] += s.yards_by_read[slot];
                total.hits_by_read[slot] += s.hits_by_read[slot];
            }
        }
        let per_hit = |slot: usize| match total.hits_by_read[slot] {
            0 => 0.0,
            n => total.yards_by_read[slot] / n as f32,
        };
        println!(
            "{name:<10} {:>5} {:>5} {:>5} {:>5} {:>5} {:>7.2} {:>7.1}  {:>4}/{:>3}/{:>3}",
            total.attempts,
            total.completions,
            total.interceptions,
            total.sacks,
            total.scrambles,
            yards_per_attempt(&total),
            total.best_yards,
            total.by_read[0],
            total.by_read[1],
            total.by_read[2],
        );
        println!(
            "{:<10} yards per completion by read: {:.1} / {:.1} / {:.1}   \
             hit rate: {}/{}  {}/{}  {}/{}",
            "",
            per_hit(0),
            per_hit(1),
            per_hit(2),
            total.hits_by_read[0],
            total.by_read[0],
            total.hits_by_read[1],
            total.by_read[1],
            total.hits_by_read[2],
            total.by_read[2],
        );
    }
}

#[test]
#[ignore = "verbose single session; run with --ignored --nocapture"]
fn autopilot_one() {
    let s = play(DEFAULT_SEED, Patience::BALANCED, 12, true);
    println!(
        "\n{} attempts: {} complete, {} INT, {} sack, {:.2} yds/att, best {:.1}",
        s.attempts,
        s.completions,
        s.interceptions,
        s.sacks,
        yards_per_attempt(&s),
        s.best_yards
    );
}

#[test]
#[ignore = "diagnostic trace; run with --ignored --nocapture"]
fn autopilot_probe() {
    use axiom_end_zone::events::SimEvent;
    let mut run = ShowcaseRun::new_run(&RunConfig::new(DEFAULT_SEED));
    for tick in 0..TICKS_PER_ATTEMPT * 4 {
        if let Some(step) = run.attempt() {
            if step.phase.in_window() && step.window_left == 0 {
                println!("  @tick {tick:5} WINDOW CLOSED unchosen");
            }
            if let Some(choice) = autopilot::decide(&step, Patience::BALANCED) {
                println!(
                    "  @tick {tick:5} CHOOSE {choice:?}  pressure {:.2}  \
                     openness {:.2}/{:.2}/{:.2}",
                    step.read.pressure,
                    step.read.read(0).openness,
                    step.read.read(1).openness,
                    step.read.read(2).openness,
                );
                run.choose(choice);
            }
        }
        run.set_user_stick(autopilot::steer(&run.sim));
        let out = run.step(&[]);
        for stamped in &out.events {
            match stamped.event {
                SimEvent::Snap { .. } => println!(
                    "  @tick {tick:5} SNAP   defense #{:?}",
                    run.last_defense_index()
                ),
                SimEvent::Throw { .. } => println!("  @tick {tick:5} THROW"),
                SimEvent::CatchCompleted { player } => {
                    println!("  @tick {tick:5} CATCH  by #{}", player.0)
                }
                SimEvent::Intercepted { defender, .. } => {
                    println!("  @tick {tick:5} INTERCEPTED by #{}", defender.0)
                }
                SimEvent::PlayEnded { reason } => {
                    println!("  @tick {tick:5} WHISTLE {reason:?}")
                }
                _ => {}
            }
        }
        if matches!(
            run.attempt().map(|s| s.phase),
            Some(AttemptPhase::Result { .. })
        ) && run.ledger().and_then(|l| l.last).map(|r| r.index) == Some(4)
        {
            break;
        }
    }
}
