//! Where the agent's near misses are being *left on the table*.
//!
//! A near miss is worth 0.13 of the boost meter and the meter drains at 0.36/s,
//! so every one is 0.36 s of boost — about 7.9 m at the +22 m/s boost gives.
//! Lap time is therefore very nearly a linear function of how many of the cars
//! the agent overtakes it also scores. This harness measures the gap between
//! those two numbers, and *why* each unscored overtake failed, so the driver is
//! tuned against evidence rather than against a hunch.

use axiom_burnt_rubber::agent::{drive_one_step, DriverTuning};
use axiom_burnt_rubber::sim::{RacePhase, RaceSim};

#[test]
fn audit_where_the_near_misses_go() {
    let driver = DriverTuning::FAST;
    let mut sim = RaceSim::shipping();

    // Per traffic slot: did we overtake it, and what was the best (smallest)
    // lane delta we ever showed it while alongside?
    let mut overtaken: std::collections::BTreeMap<u32, i32> = std::collections::BTreeMap::new();
    let mut same_lane = 0u32;
    let mut too_far = 0u32;
    let mut lane_histogram: std::collections::BTreeMap<i32, u32> = std::collections::BTreeMap::new();
    let mut room_min = f32::INFINITY;

    let mut steps = 0u32;
    while (sim.phase() != RacePhase::Finished) & (steps < 60 * 60 * 20) {
        let (command, _) = drive_one_step(&sim, &driver, u64::from(steps));

        let car_distance = sim.car().distance;
        let car_lateral = sim.car().lateral;
        let here = sim.track().sample_at(car_distance);
        let player_lane = sim.track().lane_at_lateral(&here, car_lateral);
        *lane_histogram.entry(player_lane).or_insert(0) += 1;
        room_min = room_min.min(here.half_width);

        // Anything we are physically alongside right now counts as an overtake
        // opportunity: same along-course window the near-miss rule uses.
        sim.traffic().active().for_each(|other| {
            let along = (car_distance - other.distance).abs();
            let alongside = along < 2.25 + 2.4 + 2.0;
            let passing = sim.car().forward_speed > other.speed;
            (alongside && passing).then(|| {
                let delta = (player_lane - other.lane).abs();
                let entry = overtaken.entry(other.slot).or_insert(i32::MAX);
                *entry = (*entry).min(delta);
            });
        });

        sim.step(command);
        steps += 1;
    }

    overtaken.values().for_each(|&d| {
        (d == 0).then(|| same_lane += 1);
        (d >= 2).then(|| too_far += 1);
    });
    let scored = overtaken.values().filter(|&&d| d == 1).count();

    println!("\n=== near-miss audit ===");
    println!("race time        : {:.2} s", sim.elapsed_seconds());
    println!("near misses      : {}", sim.near_miss_count());
    println!("overtakes seen   : {}", overtaken.len());
    println!("  scored (|d|==1): {scored}");
    println!("  same lane (0)  : {same_lane}");
    println!("  too far (>=2)  : {too_far}");
    println!("player lane time : {lane_histogram:?}");
    println!("narrowest road   : {room_min:.2} m half-width");
    let missed = overtaken.len() as u32 - sim.near_miss_count();
    println!(
        "LEFT ON TABLE    : {missed} overtakes unscored = {:.1} s of boost = ~{:.0} m",
        f32::from(missed as u16) * 0.13 / 0.36,
        f32::from(missed as u16) * 0.13 / 0.36 * 22.0
    );
}
