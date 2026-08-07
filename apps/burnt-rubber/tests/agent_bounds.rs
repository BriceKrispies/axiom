//! What is actually limiting the lap, measured rather than guessed.
//!
//! Two bounds and a census. Without these, tuning is a random walk: the sweep
//! plateaued at 90.78 s and no single axis moved it, which says the limit is
//! structural and the next change has to be aimed at whichever of these numbers
//! is actually binding.

use axiom_burnt_rubber::agent::{drive_one_step, DriverTuning};
use axiom_burnt_rubber::sim::{RacePhase, RaceSim};
use axiom_burnt_rubber::tuning::Tuning;

const LIMIT: u32 = 60 * 60 * 20;

/// Race, optionally topping the boost meter up every step — the "infinite
/// boost" bound. Returns (seconds, near misses, steps at full throttle).
fn race(top_up: bool) -> (f32, u32, u32) {
    let driver = DriverTuning::FAST;
    // "Infinite boost" is expressed as *data* — a tuning whose meter never
    // drains — rather than by adding a mutable boost accessor to the simulation.
    // A public setter that exists only so a diagnostic can reach inside is
    // exactly the API-widening this repo bans.
    let mut tuning = Tuning::DEFAULT;
    top_up.then(|| tuning.race.boost_drain_rate = 0.0);
    let mut sim = RaceSim::new(axiom_burnt_rubber::DEFAULT_SEED, tuning);
    let mut steps = 0u32;
    let mut countdown_steps = 0u32;
    while (sim.phase() != RacePhase::Finished) & (steps < LIMIT) {
        (sim.phase() == RacePhase::Countdown).then(|| countdown_steps += 1);
        let (command, _) = drive_one_step(&sim, &driver, u64::from(steps));
        sim.step(command);
        steps += 1;
    }
    (sim.elapsed_seconds(), sim.near_miss_count(), countdown_steps)
}

#[test]
fn what_is_limiting_the_lap() {
    let (normal, nm, countdown) = race(false);
    let (unlimited, nm_u, _) = race(true);

    println!("\n=== bounds ===");
    println!("as raced          : {normal:.2} s  ({nm} near misses)");
    println!(
        "with INFINITE boost: {unlimited:.2} s  ({nm_u} near misses)  <- the floor the boost lever can reach"
    );
    println!(
        "countdown          : {countdown} steps ({:.2} s) — dead time in every run",
        f32::from(countdown as u16) / 60.0
    );
    println!("boost lever is worth at most {:.2} s", normal - unlimited);
}

#[test]
fn where_does_the_traffic_actually_sit() {
    let driver = DriverTuning::FAST;
    let mut sim = RaceSim::shipping();
    let mut traffic_lane: std::collections::BTreeMap<i32, u32> = std::collections::BTreeMap::new();
    let mut unscored_lane: std::collections::BTreeMap<i32, u32> =
        std::collections::BTreeMap::new();
    let mut seen: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut best_delta: std::collections::BTreeMap<u32, (i32, i32)> =
        std::collections::BTreeMap::new();

    let mut steps = 0u32;
    while (sim.phase() != RacePhase::Finished) & (steps < LIMIT) {
        let (command, _) = drive_one_step(&sim, &driver, u64::from(steps));
        let here = sim.track().sample_at(sim.car().distance);
        let player_lane = sim.track().lane_at_lateral(&here, sim.car().lateral);
        let (cd, cf) = (sim.car().distance, sim.car().forward_speed);
        sim.traffic().active().for_each(|other| {
            seen.insert(other.slot).then(|| {
                *traffic_lane.entry(other.lane).or_insert(0) += 1;
            });
            let along = (cd - other.distance).abs();
            ((along < 6.65) && (cf > other.speed)).then(|| {
                let d = (player_lane - other.lane).abs();
                let e = best_delta.entry(other.slot).or_insert((i32::MAX, other.lane));
                (d < e.0).then(|| e.0 = d);
            });
        });
        sim.step(command);
        steps += 1;
    }

    best_delta.values().for_each(|&(d, lane)| {
        (d != 1).then(|| *unscored_lane.entry(lane).or_insert(0) += 1);
    });

    println!("\n=== traffic census ===");
    println!("cars spawned, by lane   : {traffic_lane:?}");
    println!("UNSCORED overtakes, by that car's lane: {unscored_lane:?}");
    println!("(a car in lane L is scorable only from lane L-1 or L+1)");
}

#[test]
fn how_is_the_boost_actually_spent() {
    let driver = DriverTuning::FAST;
    let mut sim = RaceSim::shipping();
    let mut episodes: Vec<u32> = Vec::new();
    let mut current = 0u32;
    let mut steps = 0u32;
    let mut speed_hist: std::collections::BTreeMap<i32, u32> = std::collections::BTreeMap::new();
    while (sim.phase() != RacePhase::Finished) & (steps < LIMIT) {
        let (command, _) = drive_one_step(&sim, &driver, u64::from(steps));
        sim.step(command);
        let boosting = sim.boost().active();
        boosting.then(|| current += 1);
        (!boosting && current > 0).then(|| {
            episodes.push(current);
            current = 0;
        });
        *speed_hist
            .entry((sim.car().speed() / 5.0).floor() as i32 * 5)
            .or_insert(0) += 1;
        steps += 1;
    }
    (current > 0).then(|| episodes.push(current));

    let total: u32 = episodes.iter().sum();
    let mean = f32::from(total as u16) / episodes.len().max(1) as f32 / 60.0;
    let longest = episodes.iter().copied().max().unwrap_or(0);
    println!("\n=== boost spend pattern ===");
    println!("episodes      : {}", episodes.len());
    println!("total boost   : {:.2} s", f32::from(total as u16) / 60.0);
    println!("mean episode  : {mean:.3} s");
    println!("longest       : {:.2} s", f32::from(longest as u16) / 60.0);
    println!("speed histogram (m/s bucket -> steps): {speed_hist:?}");
}

#[test]
fn how_much_does_the_course_actually_turn() {
    let sim = RaceSim::shipping();
    let track = sim.track();
    let n = (track.length() / track.spacing()) as usize;
    let total: f32 = (0..n)
        .map(|i| {
            let s = track.sample_at(i as f32 * track.spacing());
            s.curvature.abs() * track.spacing()
        })
        .sum();
    println!("\n=== course curvature ===");
    println!("length            : {:.0} m", track.length());
    println!("total |turning|   : {total:.2} rad ({:.1} full circles)", total / 6.2832);
    println!(
        "an inside line 3 m from the centreline saves ~{:.0} m of world travel",
        total * 3.0
    );
    println!("  = {:.2} s at 103 m/s", total * 3.0 / 103.0);
}

#[test]
fn the_ghost_can_actually_drive_the_phone_game() {
    use axiom_burnt_rubber::ghost::GhostRun;
    use axiom_burnt_rubber::PlayProfile;
    [PlayProfile::Wheel, PlayProfile::Rails]
        .into_iter()
        .for_each(|profile| {
            let mut g = GhostRun::new(
                axiom_burnt_rubber::DEFAULT_SEED,
                Tuning::DEFAULT,
                profile,
            );
            let mut steps = 0u32;
            while !g.finished() && steps < LIMIT {
                g.step();
                steps += 1;
            }
            println!(
                "{profile:?}: finished={} time={:.2}s dist={:.0}m nm={} impacts={}",
                g.finished(),
                g.elapsed_seconds(),
                g.distance(),
                g.sim().near_miss_count(),
                g.sim().impact_count()
            );
        });
}
