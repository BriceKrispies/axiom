//! Every shot the kicker can take, played against seeded keepers.
//!
//! The question this file exists to answer is the one no amount of reading the
//! code answers: **how often does the keeper actually save it, and does the shape
//! of a shot change that?** It sweeps the authorable space — every corner, every
//! bend, every arc, every place a curve can break — plays each shot against a run
//! of seeded keepers, and asserts on what comes back.
//!
//! Two sizes. The coarse matrix runs on every `cargo test`; the full one is
//! `#[ignore]`d and printed by `examples/keeper_report.rs`:
//!
//! ```sh
//! cargo test --release -p axiom-bend-it --test keeper_sweep -- --ignored --nocapture
//! cargo run  --release -p axiom-bend-it --example keeper_report -- 48
//! ```
//!
//! **A seed is a keeper.** Each one produces exactly one nerve for a cold
//! attempt, so a handful of seeds is a handful of keepers, and a handful of
//! keepers aliases hard: with eight, one keeper that guesses right hands every
//! right-aimed shot in the whole matrix a free save, and the report comes back
//! visibly lopsided. Ask for enough of them.

use std::sync::OnceLock;

use axiom_bend_it::matrix::{
    coarse_matrix, full_matrix, group_by, keepers, sweep_detailed, totals, Outcomes, ShotSpec,
};
use axiom_bend_it::play::ShotResult;
use axiom_bend_it::tuning::Tuning;

/// Keepers for the always-on sweep. Enough that the guess distribution is not
/// itself the result.
const COARSE_KEEPERS: u64 = 24;

/// The sweep every test below reads, played exactly once.
///
/// Rust runs tests in parallel threads, so without this each of them would sweep
/// the whole matrix again and the file would cost six times what it needs to.
fn coarse() -> &'static [(ShotSpec, ShotResult)] {
    static SWEEP: OnceLock<Vec<(ShotSpec, ShotResult)>> = OnceLock::new();
    SWEEP.get_or_init(|| {
        sweep_detailed(&coarse_matrix(), &keepers(COARSE_KEEPERS), Tuning::DEFAULT)
    })
}

/// The save rate of everything matching `pick`.
fn rate(results: &[(ShotSpec, ShotResult)], pick: impl Fn(&ShotSpec) -> bool) -> f32 {
    results
        .iter()
        .filter(|(spec, _)| pick(spec))
        .fold(Outcomes::default(), |mut out, (_, r)| {
            out.record(*r);
            out
        })
        .save_rate()
}

#[test]
fn every_shot_in_the_matrix_resolves() {
    // A shot that never resolves is the failure mode that would quietly poison
    // every number below it, so it is checked first and on its own terms: the
    // sweep reports `Miss` for anything that ran out of ticks, and a shot
    // authored inside the goal can never legitimately be one.
    let results = coarse();
    assert!(!results.is_empty());
    let stalled = results.iter().filter(|(_, r)| *r == ShotResult::Miss).count();
    assert_eq!(
        stalled,
        0,
        "{stalled} of {} shots never resolved",
        results.len()
    );
}

#[test]
fn the_keeper_saves_a_believable_share_of_everything() {
    let results = coarse();
    let overall = totals(results);
    let saved = overall.save_rate();
    println!(
        "coarse sweep: {} penalties, keeper saved {:.1}%, scored {:.1}%, frame {:.2}%",
        overall.total(),
        saved * 100.0,
        overall.goal_rate() * 100.0,
        overall.frame as f32 / overall.total().max(1) as f32 * 100.0
    );
    assert!(
        (0.25..=0.70).contains(&saved),
        "the keeper saved {:.1}% of {} penalties — a keeper that saves nearly \
         everything or nearly nothing is not a keeper",
        saved * 100.0,
        overall.total()
    );
    // And the two outcomes that are supposed to be rare, are.
    assert!(overall.misses == 0, "an authored shot cannot go wide");
    assert!(
        overall.frame as f32 / overall.total() as f32 <= 0.05,
        "the frame should be a rarity, not a mechanic"
    );
}

#[test]
fn where_you_aim_is_the_biggest_thing_you_control() {
    let results = coarse();
    let corners = rate(results, |s| s.h.abs() >= 1.0);
    let middle = rate(results, |s| s.h == 0.0);
    assert!(
        middle > corners + 0.15,
        "a shot down the middle ({:.1}%) should be far easier to save than one \
         into the corner ({:.1}%)",
        middle * 100.0,
        corners * 100.0
    );
}

#[test]
fn a_flat_shot_is_the_easiest_thing_in_the_game_to_save() {
    // The keeper reads the first fraction of the flight and extrapolates it. A
    // shot with no arc on it is exactly the shot that extrapolation is good at,
    // so putting *some* shape on the ball has to be worth something — otherwise
    // the height half of the mechanic is decoration.
    let results = coarse();
    let flat = rate(results, |s| s.loft == 0.0);
    let shaped = rate(results, |s| s.loft != 0.0);
    assert!(
        flat > shaped + 0.05,
        "flat shots are saved {:.1}% and shaped ones {:.1}% — shaping the flight \
         is not buying anything",
        flat * 100.0,
        shaped * 100.0
    );
}

#[test]
fn the_keeper_is_beatable_from_everywhere_and_unbeatable_from_nowhere() {
    // No cell of the goal may be a guaranteed save or a guaranteed goal: either
    // one is a hole a player would find in ten minutes and then never leave.
    let results = coarse();
    let by_corner = group_by(results, |s| {
        format!("h {:+.2} v {:.2}", s.h, s.v)
    });
    by_corner.iter().for_each(|(name, out)| {
        assert!(
            out.goals > 0,
            "{name} is a guaranteed save across {} shapes",
            out.total()
        );
        assert!(
            out.saves > 0,
            "{name} is a guaranteed goal across {} shapes",
            out.total()
        );
    });
}

#[test]
fn the_same_sweep_twice_is_the_same_sweep() {
    // Re-swept from scratch rather than read from the shared sweep: the claim is
    // that the sweep is reproducible, and reading one cached answer twice would
    // prove nothing at all.
    let fresh = || {
        totals(&sweep_detailed(
            &coarse_matrix()[..40],
            &keepers(4),
            Tuning::DEFAULT,
        ))
    };
    let (a, b) = (fresh(), fresh());
    assert_eq!(a, b, "the sweep must be reproducible to be worth anything");
}

#[test]
#[ignore = "the exhaustive matrix; run with --release --ignored --nocapture"]
fn the_exhaustive_matrix() {
    let matrix = full_matrix();
    let seeds = keepers(48);
    println!(
        "\n{} shapes x {} keepers = {} penalties",
        matrix.len(),
        seeds.len(),
        matrix.len() * seeds.len()
    );
    let results = sweep_detailed(&matrix, &seeds, Tuning::DEFAULT);
    let overall = totals(&results);
    println!(
        "KEEPER SAVED {:.1}%   scored {:.1}%   frame {:.2}%\n",
        overall.save_rate() * 100.0,
        overall.goal_rate() * 100.0,
        overall.frame as f32 / overall.total().max(1) as f32 * 100.0
    );
    // The same claims as the coarse sweep, over the whole space this time.
    assert_eq!(overall.misses, 0);
    assert!((0.25..=0.70).contains(&overall.save_rate()));
    assert!(rate(&results, |s| s.h == 0.0) > rate(&results, |s| s.h.abs() >= 1.0) + 0.15);
    assert!(rate(&results, |s| s.loft == 0.0) > rate(&results, |s| s.loft != 0.0) + 0.05);
    // Left and right are the same game.
    let left = rate(&results, |s| s.h < 0.0);
    let right = rate(&results, |s| s.h > 0.0);
    assert!(
        (left - right).abs() < 0.04,
        "one side of the goal is easier than the other: {:.1}% vs {:.1}%",
        left * 100.0,
        right * 100.0
    );
}
