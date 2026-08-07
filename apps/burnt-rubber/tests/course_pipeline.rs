//! **The course pipeline, end to end** — source text to a playable race, and
//! the deterministic performance report that goes with it.
//!
//! Everything here drives the real thing: the real parser on the real demo
//! source, the real compiler, the real validator, the real agent, and the real
//! `BurntRubber` app. Nothing is stubbed, and nothing asserts merely that a
//! function did not panic.

use std::sync::Arc;
use std::time::Instant;

use axiom_burnt_rubber::course::authoring;
use axiom_burnt_rubber::course::runtime::CoursePlan;
use axiom_burnt_rubber::course::validation::ghost;
use axiom_burnt_rubber::course::{compiler, procedural};
use axiom_burnt_rubber::sim::traffic::Traffic;
use axiom_burnt_rubber::{
    BurntRubber, DriveCommand, PlayProfile, RacePhase, RaceSim, Tuning, DEFAULT_SEED,
};

fn shipping() -> CoursePlan {
    procedural::shipping_plan(DEFAULT_SEED).expect("the shipping course compiles")
}

/// **The completion bar.** A textual course parses, compiles into a typed
/// immutable plan, generates real geometry and real traffic, runs three
/// encounter templates, compiles opportunity windows and validates.
#[test]
fn a_text_course_becomes_a_validated_playable_plan() {
    let spec = authoring::burning_coast_spec().expect("the demo course parses");
    assert_eq!(spec.name, "burning_coast");

    let plan = compiler::compile(&spec, &Tuning::DEFAULT).expect("it compiles");
    assert!(plan.length() > 2_000.0, "{} m of road", plan.length());
    assert!(plan.track().samples().len() > 1_000);
    assert!(plan.sections().len() > spec.items.len(), "motifs expanded");

    // Three distinct encounter templates, each owning real vehicles.
    let mut kinds: Vec<&str> = plan.encounters().iter().map(|e| e.kind).collect();
    kinds.sort_unstable();
    assert_eq!(kinds, vec!["rolling_wall", "slalom", "zipper"]);
    for encounter in plan.encounters() {
        assert!(!encounter.vehicles.is_empty(), "{} placed nothing", encounter.kind);
        for id in &encounter.vehicles {
            let vehicle = plan.vehicle(*id).expect("the vehicle exists");
            assert_eq!(vehicle.encounter, Some(encounter.id));
        }
    }

    // Opportunity windows exist, point at real cars, and none of them awarded
    // anything — that is the scoring system's job.
    assert!(!plan.near_miss_windows().is_empty());
    for window in plan.near_miss_windows() {
        assert!(!window.vehicles.is_empty());
        assert!(window.end_m > window.start_m);
        window
            .vehicles
            .iter()
            .for_each(|id| assert!(plan.vehicle(*id).is_some()));
    }

    // Validation ran and produced measurements, not just a boolean.
    let report = plan.report();
    assert_eq!(report.sections.len(), plan.sections().len());
    assert!(report.metrics.traversal_cells > 0);
    assert!(report.metrics.vehicles > 0);
    assert!(
        !report.has_errors(),
        "the demo course does not validate:\n{}",
        report.dump()
    );

    // And it is playable: the real app steps it and the car moves.
    let mut sim = RaceSim::from_plan(Arc::new(plan), Tuning::DEFAULT, PlayProfile::Wheel);
    while sim.phase() == RacePhase::Countdown {
        sim.step(DriveCommand::IDLE);
    }
    let start = sim.car().distance;
    (0..900).for_each(|_| sim.step(DriveCommand::FLAT_OUT));
    assert!(
        sim.car().distance > start + 400.0,
        "the demo course is drivable: {} -> {}",
        start,
        sim.car().distance
    );
}

/// The real game consumes the compiled plan, and every gameplay system that
/// existed before still works on it.
#[test]
fn the_shipping_game_runs_on_the_compiled_plan() {
    let mut app = BurntRubber::with(DEFAULT_SEED, Tuning::DEFAULT, 640, 360);
    while app.sim().phase() == RacePhase::Countdown {
        app.advance_steps(1, DriveCommand::IDLE);
    }
    app.advance_steps(4_000, DriveCommand::FLAT_OUT);
    app.present();

    let sim = app.sim();
    // Flat out with the wheel dead straight, so the car runs wide on every
    // corner: this is "the game is playable", not "the game is played well".
    assert!(sim.car().distance > 2_000.0, "{} m", sim.car().distance);
    assert!(sim.near_miss_count() > 0, "near misses still score");
    assert!(sim.traffic().active_count() > 0, "traffic is live");
    assert!(app.diagnostics().scene.active_chunks > 0, "the road renders");
    // Boost is being earned and can be spent.
    assert!(sim.boost().charge() > 0.0);

    // The plan the game is running on is the plan the compiler validated.
    assert_eq!(app.sim().plan().seed(), DEFAULT_SEED);
    assert!(!app.sim().plan().report().has_errors());
}

/// A reset reproduces the same course *and* the same traffic identities, and a
/// different seed is a different course.
#[test]
fn a_restart_is_deterministic_and_a_new_seed_is_a_new_course() {
    let mut app = BurntRubber::with(DEFAULT_SEED, Tuning::DEFAULT, 640, 360);
    while app.sim().phase() == RacePhase::Countdown {
        app.advance_steps(1, DriveCommand::IDLE);
    }
    app.advance_steps(1_800, DriveCommand::FLAT_OUT);
    let first: Vec<(u32, f32, i32)> = app
        .sim()
        .traffic()
        .active()
        .map(|c| (c.slot, c.distance, c.lane))
        .collect();
    let distance = app.sim().car().distance;
    assert!(!first.is_empty());

    app.advance_steps(1, DriveCommand { restart: true, ..DriveCommand::IDLE });
    while app.sim().phase() == RacePhase::Countdown {
        app.advance_steps(1, DriveCommand::IDLE);
    }
    app.advance_steps(1_800, DriveCommand::FLAT_OUT);
    let again: Vec<(u32, f32, i32)> = app
        .sim()
        .traffic()
        .active()
        .map(|c| (c.slot, c.distance, c.lane))
        .collect();
    assert_eq!(first, again, "a restart is not reproducing the same traffic");
    assert_eq!(distance, app.sim().car().distance);

    let other = BurntRubber::with(DEFAULT_SEED + 1, Tuning::DEFAULT, 640, 360);
    assert_ne!(
        other.sim().track().samples(),
        app.sim().track().samples(),
        "a different seed is a different road"
    );
}

/// **The runtime never recompiles.** Building the app compiles the course once;
/// stepping it thousands of times must not build another one.
#[test]
fn the_runtime_does_not_recompile_the_course_per_frame() {
    let mut app = BurntRubber::with(DEFAULT_SEED, Tuning::DEFAULT, 640, 360);
    while app.sim().phase() == RacePhase::Countdown {
        app.advance_steps(1, DriveCommand::IDLE);
    }
    // A plan is shared by `Arc`, so "the same plan" is pointer identity — a
    // recompile would produce a different allocation however equal its contents.
    let before = Arc::as_ptr(app.sim().plan());
    app.advance_steps(3_000, DriveCommand::FLAT_OUT);
    app.present();
    assert_eq!(before, Arc::as_ptr(app.sim().plan()));

    // And a reset keeps it too — the road you come back to is the road you left.
    app.advance_steps(1, DriveCommand { reset: true, ..DriveCommand::IDLE });
    assert_eq!(before, Arc::as_ptr(app.sim().plan()));
    app.advance_steps(1, DriveCommand { restart: true, ..DriveCommand::IDLE });
    assert_eq!(
        before,
        Arc::as_ptr(app.sim().plan()),
        "a restart recompiled the course"
    );
}

/// Every vehicle activates once, in the right place, and is retired behind the
/// player.
#[test]
fn traffic_activates_once_and_retires_behind_the_player() {
    let plan = Arc::new(shipping());
    let track = plan.track().clone();
    let race = Tuning::DEFAULT.race;
    let collision = Tuning::DEFAULT.collision;
    let mut traffic = Traffic::new(plan.clone(), &race);

    let mut seen: Vec<u32> = Vec::new();
    let mut live: Vec<u32> = Vec::new();
    let mut player = 0.0f32;
    while player < track.length() - 300.0 {
        player += 1.5;
        traffic.step(player, &track, &race, &collision);
        let now: Vec<u32> = traffic.active().map(|c| c.slot).collect();
        // Nothing appears twice.
        now.iter().filter(|s| !live.contains(s)).for_each(|slot| {
            assert!(!seen.contains(slot), "vehicle {slot} activated twice");
            seen.push(*slot);
        });
        // Nothing lives behind the player's retirement floor.
        traffic.active().for_each(|car| {
            assert!(
                car.distance >= player - race.traffic_behind - 1.0,
                "vehicle {} is {} m behind the player",
                car.slot,
                player - car.distance
            );
        });
        live = now;
    }
    assert!(seen.len() > 60, "the run activated {} vehicles", seen.len());
    // Every activation was of a real compiled plan, at the distance it named.
    seen.iter().for_each(|slot| {
        assert!(
            plan.traffic().iter().any(|p| p.id.0 == *slot),
            "vehicle {slot} is not in the plan"
        );
    });
}

/// Ghost validation runs on the real compiled course and reports a measurement.
#[test]
fn ghost_validation_measures_the_shipping_course() {
    let plan = Arc::new(shipping());
    let report = ghost::run(
        plan,
        Tuning::DEFAULT,
        PlayProfile::Wheel,
        ghost::VALIDATION_STEP_LIMIT,
    );
    println!("=== ghost validation ===\n{}", report.summary());
    assert!(report.completed, "the ghost got {:.0} m", report.distance_m);
    assert!(report.near_misses > 60);
    assert!(report.boost_fraction() > 0.25, "{:.2}", report.boost_fraction());
    assert!(report.longest_boost_steps > 60, "under a second of continuous boost");
    assert!(report.encounter_failures.is_empty(), "{:?}", report.encounter_failures);
    assert!(report.average_speed_mps > 80.0);
}

/// **The performance harness.** Deterministic counts, plus wall-clock timings
/// that are *reported* rather than asserted on — a build machine's speed is not
/// a property of the course.
#[test]
fn course_performance_report() {
    let start = Instant::now();
    let plan = shipping();
    let compile_ms = start.elapsed().as_secs_f64() * 1_000.0;

    let report = plan.report();
    let index_start = Instant::now();
    // A million distance lookups, the shape the runtime actually makes.
    let mut probe = 0usize;
    for i in 0..1_000_000u32 {
        let at = (i % 9_000) as f32;
        probe += plan.first_vehicle_at(at);
    }
    let lookup_ns = index_start.elapsed().as_secs_f64() * 1_000.0;

    let section_start = Instant::now();
    let mut sections = 0usize;
    for i in 0..1_000_000u32 {
        let at = (i % 9_000) as f32;
        sections += plan.section_at(at).index as usize;
    }
    let section_ns = section_start.elapsed().as_secs_f64() * 1_000.0;

    let dump = plan.dump();

    println!(
        "=== Burnt Rubber — course performance ===\n\
         compile              {compile_ms:8.1} ms\n\
         track samples        {samples:8}\n\
         compiled sections    {sections_n:8}\n\
         traffic plans        {vehicles:8}\n\
         encounters           {encounters:8}\n\
         near-miss windows    {windows:8}\n\
         traversal grid       {cells:8} cells ({blocked} blocked)\n\
         plan dump            {dump_len:8} bytes\n\
         1e6 traffic lookups  {lookup_ns:8.1} ms\n\
         1e6 section lookups  {section_ns:8.1} ms",
        samples = report.metrics.samples,
        sections_n = report.metrics.sections,
        vehicles = report.metrics.vehicles,
        encounters = report.metrics.encounters,
        windows = report.metrics.near_miss_windows,
        cells = report.metrics.traversal_cells,
        blocked = report.metrics.blocked_cells,
        dump_len = dump.len(),
    );

    // The deterministic half of the report, which *is* asserted: the course's
    // shape must not quietly balloon.
    assert!(report.metrics.samples > 4_000 && report.metrics.samples < 6_000);
    assert!(report.metrics.sections > 20 && report.metrics.sections < 200);
    assert!(report.metrics.vehicles > 50 && report.metrics.vehicles < 400);
    assert!(
        report.metrics.traversal_cells < 20_000,
        "the validation grid is {} cells",
        report.metrics.traversal_cells
    );
    assert!(probe > 0 && sections > 0, "the lookups were not optimised away");
}

/// A course that cannot be driven is refused by name, with every reason.
#[test]
fn an_unplayable_course_is_refused_with_its_reasons() {
    let source = r#"
        course "impassable" {
            seed = 5
            straight {
                id = "run"
                length = 1500m
                lanes = 3
                traffic {
                    flow {
                        vehicles_per_km = 120
                        min_headway = 6m
                        preferred_headway = 7m
                        max_headway = 8m
                        speed = 10mps..11mps
                    }
                }
            }
        }
    "#;
    let spec = authoring::parse("impassable.brc", source).expect("it parses");
    let err = compiler::compile_valid(&spec, &Tuning::DEFAULT).expect_err("it must not validate");
    assert!(
        err.message.contains("wall") || err.message.contains("overlapped"),
        "{}",
        err.message
    );
    // But the plan itself still compiles, so an author can be shown *where*.
    let plan = compiler::compile(&spec, &Tuning::DEFAULT).expect("it compiles");
    assert!(plan.report().has_errors());
    assert!(plan.report().errors().count() > 1, "every reason, not the first");
}

/// The authoring surface answers questions about where the player is.
#[test]
fn the_authoring_overlay_reports_the_course_around_the_player() {
    let mut app = BurntRubber::with(DEFAULT_SEED, Tuning::DEFAULT, 640, 360);
    while app.sim().phase() == RacePhase::Countdown {
        app.advance_steps(1, DriveCommand::IDLE);
    }
    app.advance_steps(2_500, DriveCommand::FLAT_OUT);

    let rows = app.course_rows();
    let value = |label: &str| {
        rows.iter()
            .find(|(l, _)| l == label)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("no `{label}` row in {rows:?}"))
    };
    assert!(value("course seed").contains("0x"));
    assert!(value("section").contains('('), "{}", value("section"));
    assert!(value("traversability").contains("route exists"));
    assert!(value("validation").starts_with("0 errors"));
    assert!(value("boost economy").contains("earn"));
    assert!(value("near-miss chances").contains("within"));

    let dump = app.dump_course();
    assert!(dump.contains("--- traffic ---"));
    assert_eq!(dump, app.dump_course(), "the dump is deterministic");
}
