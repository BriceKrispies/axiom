//! **Can a perfect line hold the boost for the whole lap?**
//!
//! A measuring tool, ignored by default, in the same family as
//! `agent_tuning_sweep` and `agent_search`: those search over how the car is
//! *driven*, this one searches over what it is driven *through*.
//!
//! ```text
//! cargo test -p axiom-burnt-rubber --release --test road_tailoring -- --ignored --nocapture
//! ```
//!
//! # The question, as a number
//!
//! "Boosting the entire time" is a duty cycle of `1.0`, and the course
//! validator already measures the thing that decides it
//! (`course::validation::boost`): per section, `earned / spent`, where `spent`
//! is the section's seconds times the drain times the duty the course asks for.
//! Author `target_boost_duty = 1.0` and a section with `ratio >= 1` is one a
//! perfect line can cross without the meter falling.
//!
//! That is an *estimate* of the opportunities the road compiled. So this
//! harness reports two things side by side:
//!
//! * the validator's per-section ratio — what the road offers;
//! * the agent's measured `boost_steps / steps` — what a real driver took.
//!
//! A road is only tailored when both agree. The estimate alone can be met by a
//! section whose opportunities are all in lanes no single line can visit.
//!
//! # "More than two ways to do it"
//!
//! The three income sources are independent, and a route that leans on each is
//! a genuinely different drive:
//!
//! * **thread** — near misses, which need adjacent-lane traffic and pay the most;
//! * **collect** — pickups, which need a committed line and pay in lumps;
//! * **hold** — the passive high-speed rate, which pays for nothing but being fast.
//!
//! [`Route`] scores a section against each one *alone*. A section only counts
//! as offering a way if that source on its own clears the drain; a section with
//! three ways is one where any of the three lines survives it, which is the
//! shape the road is being tailored toward.

use std::sync::Arc;

use axiom_burnt_rubber::agent::{self, DriverTuning};
use axiom_burnt_rubber::tuning::{RaceTuning, Tuning};
use axiom_burnt_rubber::course::specification::{
    BoostPickupSpec, BoostTier, CourseItem, SectionSpec, TrafficZoneSpec,
};
use axiom_burnt_rubber::{compile_course, CourseSpec, PlayProfile, RaceSim};

const STEP_LIMIT: u32 = 60 * 60 * 20;

/// The three ways a section can pay for the boost it costs.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Route {
    thread: f32,
    collect: f32,
    hold: f32,
}

impl Route {
    /// How many of the three sources clear the drain on their own.
    fn ways(&self) -> usize {
        [self.thread, self.collect, self.hold]
            .iter()
            .filter(|r| **r >= 1.0)
            .count()
    }
}

/// One road under test.
struct Road {
    name: &'static str,
    spec: CourseSpec,
}

/// The **sawtooth**: what the meter actually did over a lap.
///
/// The road is not being tailored for a flat sustain. It is being tailored for
/// a rhythm — spend the meter down to nearly nothing over a long dry stretch,
/// then cross something that fills it all the way back — and a flat 90% duty
/// and a perfect sawtooth score identically on duty alone. Only the *shape*
/// tells them apart, so the shape is what gets measured.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sawtooth {
    /// How many times the meter refilled to near-full after running low.
    cycles: u32,
    /// The deepest the meter got before a refill — how close to empty the dry
    /// stretches actually run.
    deepest: f32,
    /// The shallowest "low" across the lap: the cycle that drained least, i.e.
    /// the one place the road is being too generous.
    shallowest_low: f32,
    /// The worst refill: the peak that came up shortest of full.
    worst_peak: f32,
}

/// A meter is "spent" below this and "full" above it.
const SPENT: f32 = 0.15;
const FULL: f32 = 0.85;

/// Walk a charge trace and count the spend/refill cycles in it.
fn sawtooth(trace: &[f32]) -> Sawtooth {
    let mut cycles = 0;
    let mut deepest = 1.0f32;
    let mut shallowest_low = 0.0f32;
    let mut worst_peak = 1.0f32;
    // A cycle is a trough followed by a peak. Tracked as a little state machine
    // rather than by thresholding each sample, because what matters is the
    // *alternation* — a meter that hovers at 0.5 forever crosses neither line
    // and must score zero cycles, not one long ambiguous one.
    let mut low = f32::INFINITY;
    let mut falling = true;
    for charge in trace.iter().copied() {
        if falling {
            low = low.min(charge);
            if charge >= FULL {
                cycles += 1;
                deepest = deepest.min(low);
                shallowest_low = shallowest_low.max(low);
                worst_peak = worst_peak.min(charge);
                falling = false;
            }
        } else if charge <= SPENT {
            low = charge;
            falling = true;
        }
    }
    Sawtooth {
        cycles,
        deepest,
        shallowest_low,
        worst_peak,
    }
}

/// What a road scored.
struct Scored {
    name: &'static str,
    /// The validator's worst section ratio — the bottleneck.
    worst_ratio: f32,
    /// How many sections offer fewer than three independent ways through.
    thin_sections: usize,
    /// Sections whose ratio is comfortable rather than marginal.
    forgiving: usize,
    /// Sections a perfect line cannot pay for at all.
    starved: usize,
    /// What the agent actually managed, `0..1`.
    measured_duty: f32,
    /// The rhythm the meter actually ran at.
    shape: Sawtooth,
    lap_seconds: f32,
    near_misses: u32,
    impacts: u32,
}

fn score(road: &Road, tuning: &Tuning) -> Scored {
    let plan = Arc::new(compile_course(&road.spec, tuning).expect("the road compiles"));
    let report = plan.report();
    let race = &tuning.race;

    let routes: Vec<Route> = report
        .sections
        .iter()
        .map(|s| {
            // The validator folds all three sources into one `boost_earned`.
            // Splitting them back out is what answers "how many ways", so each
            // is re-derived here against the same `spent` the validator used.
            let spent = s.boost_spent.max(1.0e-3);
            let seconds = (s.end_m - s.start_m) / EXPECTED_SPEED_MPS;
            Route {
                thread: (s.opportunities as f32 * race.near_miss_boost) / spent,
                collect: (s.pickups as f32 * race.pickup_boost[2]) / spent,
                hold: (seconds * race.high_speed_boost_rate) / spent,
            }
        })
        .collect();

    // Driven a step at a time rather than through `agent::race`, so the meter
    // can be sampled: the aggregate hides the very thing being tailored.
    let driver = DriverTuning::for_profile(PlayProfile::Rails);
    let mut sim = RaceSim::from_plan(plan.clone(), *tuning, PlayProfile::Rails);
    let mut trace: Vec<f32> = Vec::new();
    let mut boost_steps = 0u32;
    let mut steps = 0u32;
    while steps < STEP_LIMIT && sim.phase() != axiom_burnt_rubber::RacePhase::Finished {
        let (command, _) = agent::drive_one_step(&sim, &driver, steps as u64);
        boost_steps += u32::from(command.boost & sim.boost().ready(&tuning.race));
        sim.step(command);
        trace.push(sim.boost().charge());
        steps += 1;
    }
    let shape = sawtooth(&trace);
    let run = agent::race(
        RaceSim::from_plan(plan.clone(), *tuning, PlayProfile::Rails),
        &driver,
        STEP_LIMIT,
    );

    Scored {
        name: road.name,
        worst_ratio: report
            .sections
            .iter()
            .map(|s| s.ratio())
            .fold(f32::INFINITY, f32::min),
        thin_sections: routes.iter().filter(|r| r.ways() < 3).count(),
        forgiving: report.sections.iter().filter(|s| s.ratio() >= 1.6).count(),
        starved: report.sections.iter().filter(|s| s.ratio() < 1.0).count(),
        measured_duty: run.boost_steps as f32 / run.steps.max(1) as f32,
        shape,
        lap_seconds: run.elapsed_seconds,
        near_misses: run.near_misses,
        impacts: run.impacts,
    }
}

/// The speed the authored budgets are written against (m/s).
const EXPECTED_SPEED_MPS: f32 = 80.0;

fn report_row(s: &Scored) {
    println!(
        "{:<20} cycles={:<3} deepest={:.2} shallowest={:.2} peak={:.2} | \
         duty={:.0}% worst={:.2} starved={:<3} lap={:.1}s nm={}",
        s.name,
        s.shape.cycles,
        s.shape.deepest,
        s.shape.shallowest_low,
        s.shape.worst_peak,
        s.measured_duty * 100.0,
        s.worst_ratio,
        s.starved,
        s.lap_seconds,
        s.near_misses,
    );
}

/// The baseline: what the shipping road scores today, so every tailored variant
/// below has something to be better than.
#[test]
#[ignore]
fn measure_the_shipping_road() {
    let tuning = Tuning::DEFAULT;
    println!("\ndrain {:.3}/s, near miss {:.3}, high-speed {:.3}/s, large pickup {:.3}",
        tuning.race.boost_drain_rate,
        tuning.race.near_miss_boost,
        tuning.race.high_speed_boost_rate,
        tuning.race.pickup_boost[2],
    );
    println!(
        "a section needs {:.2} near misses per second to pay for a held boost\n",
        (tuning.race.boost_drain_rate - tuning.race.high_speed_boost_rate)
            / tuning.race.near_miss_boost
    );

    let shipping = Road {
        name: "shipping",
        spec: axiom_burnt_rubber::course::procedural::shipping_spec(
            axiom_burnt_rubber::DEFAULT_SEED,
            &tuning,
        ),
    };
    report_row(&score(&shipping, &tuning));
}

/// The economy this road is being tailored *for*: a duty cycle of one.
fn sustained(race: RaceTuning) -> Tuning {
    Tuning {
        race,
        ..Tuning::DEFAULT
    }
}

/// Every tailored variant, measured side by side.
#[test]
#[ignore]
fn compare_tailored_roads() {
    let tuning = sustained(RaceTuning::DEFAULT);
    println!();
    for road in variants(&tuning) {
        report_row(&score(&road, &tuning));
    }
}

/// The economies worth trying the roads against.
///
/// The road is only half the answer. The other half is the *rate* a held boost
/// has to be paid for, and two of these knobs move it far harder than any
/// amount of authoring can:
///
/// * **traffic speed** sets the closing speed, and the closing speed sets how
///   many cars a second pass you at all. It is the dominant term by a wide
///   margin — at 300 km/h against a 331 km/h car you close at 8.7 m/s, and no
///   density survives that, because the cars cannot be packed closer than the
///   live pool allows.
/// * **the live pool** (`traffic_active`) is a hard ceiling on density near the
///   player that authoring cannot see. Nine cars across the 160 m window either
///   side of the car is one car per 18 m *at best*, whatever the road asks for.
fn economies() -> Vec<(&'static str, Tuning)> {
    let at = |kmh: f32, pool: usize, drain: f32| Tuning {
        race: RaceTuning {
            traffic_speed_min: kmh / 3.6,
            traffic_speed_max: kmh / 3.6,
            traffic_active: pool,
            boost_drain_rate: drain,
            ..RaceTuning::DEFAULT
        },
        ..Tuning::DEFAULT
    };
    vec![
        ("300kmh pool9 drain.36", at(300.0, 9, 0.36)),
        ("300kmh pool24 drain.36", at(300.0, 24, 0.36)),
        ("300kmh pool24 drain.20", at(300.0, 24, 0.20)),
        ("300kmh pool24 drain.16", at(300.0, 24, 0.16)),
        ("300kmh pool24 drain.13", at(300.0, 24, 0.13)),
        ("300kmh pool32 drain.16", at(300.0, 32, 0.16)),
        ("200kmh pool24 drain.36", at(200.0, 24, 0.36)),
    ]
}

/// The full grid: every road against every economy.
#[test]
#[ignore]
fn sweep_roads_against_economies() {
    for (label, tuning) in economies() {
        println!(
            "\n== {label} — needs {:.2} near misses/s, closes at {:.1} m/s off boost",
            (tuning.race.boost_drain_rate - tuning.race.high_speed_boost_rate)
                / tuning.race.near_miss_boost,
            tuning.vehicle.top_speed - tuning.race.traffic_speed_max,
        );
        for road in variants(&tuning) {
            report_row(&score(&road, &tuning));
        }
    }
}

/// Walk every section of a spec, whatever shape it was authored in.
fn sections_mut(spec: &mut CourseSpec) -> Vec<&mut SectionSpec> {
    spec.items
        .iter_mut()
        .flat_map(|item| match item {
            CourseItem::Section(s) => vec![s],
            CourseItem::Group(g) => g.parts.iter_mut().collect(),
            CourseItem::Motif(_) => Vec::new(),
        })
        .collect()
}

/// Every traffic zone in the spec, wherever it was hung.
///
/// A zone can be authored on a lone section *or* on a group that several
/// sections share, and the shipping road uses the second — which is worth a
/// named function rather than an inline filter, because reaching only the first
/// is a silent no-op. Densifying through a section walk left every measured
/// number identical to the baseline's, which reads exactly like a change that
/// had no effect and is indistinguishable from one that was not applied.
fn zones_mut(spec: &mut CourseSpec) -> Vec<&mut TrafficZoneSpec> {
    spec.items
        .iter_mut()
        .flat_map(|item| -> Vec<&mut TrafficZoneSpec> {
            match item {
                CourseItem::Section(s) => s.traffic.iter_mut().collect(),
                CourseItem::Group(g) => g
                    .traffic
                    .iter_mut()
                    .chain(g.parts.iter_mut().flat_map(|p| p.traffic.iter_mut()))
                    .collect(),
                // The shipping road is built almost entirely of motifs, so
                // skipping them here is what made two rounds of "tailored"
                // variants score byte-identically to the baseline.
                CourseItem::Motif(m) => m.traffic.iter_mut().collect(),
            }
        })
        .collect()
}

/// Scale a road's traffic density, and vary it along the course so some
/// stretches are more forgiving than others.
///
/// `swell` is the peak multiplier and it is applied as a slow wave over the
/// section index rather than flat, because a road that is uniformly at the
/// sustain threshold is a road with no shape: every section is equally tight
/// and a mistake anywhere is equally fatal. The wave is what makes some
/// stretches a place to recover and others a place to be perfect.
fn densify(spec: &mut CourseSpec, floor: f32, swell: f32) {
    let zones = zones_mut(spec);
    let count = zones.len().max(1) as f32;
    for (index, zone) in zones.into_iter().enumerate() {
        let phase = index as f32 / count * std::f32::consts::TAU * 2.0;
        let scale = floor + (swell - floor) * (phase.sin() * 0.5 + 0.5);
        if let Some(flow) = zone.flow.as_mut() {
            flow.vehicles_per_km *= scale;
            flow.min_headway_m = (flow.min_headway_m / scale).max(12.0);
            flow.preferred_headway_m = (flow.preferred_headway_m / scale).max(16.0);
            flow.max_headway_m = (flow.max_headway_m / scale).max(22.0);
        }
    }
}

/// Lay a continuous line of pickups down one lane, so a driver who commits to
/// that lane is paid for it whether or not there is traffic to thread.
fn pickup_line(spec: &mut CourseSpec, lane: i32, every_m: f32, tier: BoostTier) {
    for section in sections_mut(spec) {
        let length = section.primitive.length_m();
        let count = ((length / every_m).floor() as u32).max(1);
        section.pickups.push(BoostPickupSpec::row(
            every_m * 0.5,
            lane,
            tier,
            count,
            every_m,
        ));
    }
}

/// Author the **cadence**: long dry stretches that spend the meter down to
/// nothing, each ending in a refill zone that puts it back to full.
///
/// The two numbers come out of the economy rather than out of taste. A full
/// meter is `1.0` and drains at [`RaceTuning::boost_drain_rate`], so a dry
/// stretch that empties it lasts `1.0 / drain` seconds — and at the speed a
/// boosting car actually travels, that is how long the stretch has to be. The
/// refill zone then has to hand back the whole meter in the few seconds it takes
/// to cross, which is what makes it dense enough to read as an event rather than
/// as the traffic simply thickening.
///
/// `zones` alternate: every `nth` traffic zone becomes a refill, the rest are
/// emptied out. Emptying is as important as filling — a "dry" stretch with
/// ordinary traffic in it is not dry, it is just slightly less generous, and the
/// meter never actually runs down.
fn cadence(spec: &mut CourseSpec, tuning: &Tuning, refill_every: usize, lumps: u32) {
    // **The whole road goes dry.** This is the half that is easy to skip and
    // impossible to do without: a dry stretch with ordinary traffic still in it
    // is not dry, it is merely less generous, and the meter coasts across it
    // instead of emptying. Authored at 0.35 of the original density the lap
    // still held 80–93% duty and produced two cycles; the refills were not the
    // problem, the floor between them was.
    for zone in zones_mut(spec) {
        if let Some(flow) = zone.flow.as_mut() {
            flow.vehicles_per_km *= DRY_SCALE;
            flow.min_headway_m = (flow.min_headway_m / DRY_SCALE).min(400.0);
            flow.preferred_headway_m = (flow.preferred_headway_m / DRY_SCALE).min(600.0);
            flow.max_headway_m = (flow.max_headway_m / DRY_SCALE).min(900.0);
        }
    }

    // The refills, on **sections** rather than zones. A traffic zone is about a
    // kilometre — six seconds of boosted road, which is the whole drain — so a
    // zone-sized refill is not a spot on the course, it is half the course. A
    // section is ~165 m, about a second, which is short enough to be a *place*
    // the driver aims at and long enough to hand back a full meter.
    let each = tuning.race.pickup_boost[2];
    for (index, section) in sections_mut(spec).into_iter().enumerate() {
        if index % refill_every != refill_every - 1 {
            continue;
        }
        // Three lanes of them, so the refill is not a single line that one
        // wrong lane choice misses entirely — and so there is more than one way
        // to take it.
        for lane in [-1, 0, 1] {
            section.pickups.push(BoostPickupSpec::row(
                30.0,
                lane,
                BoostTier::Large,
                lumps,
                40.0,
            ));
        }
    }
    let _ = each;
}

/// How much of its authored traffic a dry stretch keeps.
///
/// Not zero: a road with nothing on it is not a dry spell, it is a loading
/// screen. This leaves something to look at and the occasional car to thread —
/// just nowhere near enough to pay the drain.
const DRY_SCALE: f32 = 0.08;

/// The hand-authored roads under test. Each is a different answer to "how does
/// a lap pay for itself".
fn variants(tuning: &Tuning) -> Vec<Road> {
    let base = || {
        axiom_burnt_rubber::course::procedural::shipping_spec(
            axiom_burnt_rubber::DEFAULT_SEED,
            tuning,
        )
    };

    // A. **Thread it.** Nothing but traffic, packed hard enough that the near
    //    misses alone pay the drain. The purest read of the original loop, and
    //    the one that asks the most of the driver.
    let mut thread = base();
    densify(&mut thread, 2.6, 4.4);

    // B. **Collect it.** Ordinary traffic and a committed pickup lane. A driver
    //    who holds the line is paid in lumps and barely has to thread at all —
    //    which also means giving up the freedom to dodge, so it is a different
    //    risk rather than a cheaper one.
    let mut collect = base();
    densify(&mut collect, 1.2, 1.8);
    pickup_line(&mut collect, 1, 150.0, BoostTier::Large);

    // C. **Both, halved.** Neither source pays on its own; together they do,
    //    with room to spare. This is the forgiving road — the one where a
    //    missed pass is recoverable because the pickups are still coming.
    let mut mixed = base();
    densify(&mut mixed, 1.8, 3.0);
    pickup_line(&mut mixed, -1, 260.0, BoostTier::Medium);

    // The cadence roads: the same idea at three rhythms. A short cycle is a
    // busy road that never lets the meter get truly low; a long one is a real
    // dry spell you have to survive on what you banked.
    let cadence_road = |every: usize, lumps: u32| {
        let mut spec = base();
        cadence(&mut spec, tuning, every, lumps);
        spec
    };

    vec![
        Road { name: "A thread", spec: thread },
        Road { name: "C mixed", spec: mixed },
        // Every 3rd section is ~500 m of dry road, every 5th ~800 m, every 8th
        // ~1300 m — a drain of 3, 5 and 8 seconds against a meter that lasts
        // 6.25. The middle one should be the sawtooth; the first should never
        // run dry and the last should run dry and stay there.
        Road { name: "D refill /3", spec: cadence_road(3, 2) },
        Road { name: "E refill /5", spec: cadence_road(5, 2) },
        Road { name: "F refill /8", spec: cadence_road(8, 2) },
        Road { name: "G refill /5 x3", spec: cadence_road(5, 3) },
    ]
}
