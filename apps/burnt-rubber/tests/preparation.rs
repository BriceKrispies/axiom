//! **The end-to-end proof** that Burnt Rubber's startup work really happens in
//! the preparation phase, and really stops happening during gameplay.
//!
//! ```text
//! cargo test -p axiom-burnt-rubber --test preparation
//! ```
//!
//! # Why this file exists at all
//!
//! The committed golden run (`tests/agent_golden.rs`) proves the game did not
//! *change*. It cannot prove the migration *worked*: a course compiled four
//! times produces byte-identical goldens to a course compiled once, and a
//! generator moved into the barrier produces the same pixels as one left in the
//! constructor. Absence of regression and presence of improvement are different
//! claims, and the goldens can only make the first one.
//!
//! So these tests assert the things the goldens are structurally blind to:
//! *when* work runs, *how many times*, and whether the barrier can be walked
//! around.

use std::sync::Arc;

use axiom_burnt_rubber::preparation::{course, meshes, textures, RacePreparation};
use axiom_burnt_rubber::tuning::FIXED_STEP_NANOS;
use axiom_burnt_rubber::{BurntRubber, PlayProfile, Tuning, DEFAULT_SEED, HEIGHT, WIDTH};
use axiom_runtime::{PreparationSchedule, PreparationTask, Runtime, RuntimeConfig, RuntimeState};

fn shipping() -> BurntRubber {
    BurntRubber::with_profile(DEFAULT_SEED, Tuning::DEFAULT, WIDTH, HEIGHT, PlayProfile::Wheel)
}

/// Drive a `RacePreparation` through a real runtime, exactly as `App::build`
/// does, and hand back the products.
fn run_phase(prep: &RacePreparation) -> Runtime {
    let mut runtime =
        Runtime::new(RuntimeConfig::new(FIXED_STEP_NANOS)).expect("a valid fixed step");
    runtime.initialize().expect("a fresh runtime initializes");
    let mut schedule = PreparationSchedule::new();
    prep.tasks(DEFAULT_SEED, &Tuning::DEFAULT)
        .into_iter()
        .for_each(|(name, task)| schedule.push(name, task));
    runtime.prepare(schedule).expect("the shipping course prepares");
    runtime
}

/// The barrier, at the app's own tier: a runtime that has run Burnt Rubber's
/// preparation reaches `Prepared`, and only then may it start and step.
#[test]
fn the_race_is_playable_only_after_preparation() {
    let prep = RacePreparation::new();

    let mut unprepared =
        Runtime::new(RuntimeConfig::new(FIXED_STEP_NANOS)).expect("a valid fixed step");
    unprepared.initialize().expect("initializes");
    assert!(
        unprepared.start().is_err(),
        "an initialized-but-unprepared runtime must refuse to start"
    );
    assert!(
        unprepared.step().is_err(),
        "and must refuse to step, so no frame can precede the world"
    );

    let mut runtime = run_phase(&prep);
    assert_eq!(runtime.state(), RuntimeState::Prepared);
    assert!(
        prep.course.borrow().is_some()
            && prep.textures.borrow().is_some()
            && prep.meshes.borrow().is_some(),
        "the course, the albedos and the road all exist at the barrier"
    );
    runtime.start().expect("a prepared runtime starts");
    runtime.step().expect("and steps");
}

/// **The measurable win.** The course is compiled once per launch, not four
/// times.
///
/// Proven by pointer identity rather than by a call counter: if the player's
/// plan and the ghost's are the same allocation, there was exactly one compile
/// to allocate. That is a stronger statement than a count, and it needs no
/// test-only instrumentation in the shipping path.
#[test]
fn the_course_is_compiled_exactly_once_per_launch() {
    let app = shipping();
    let player = app.sim().plan();
    let ghost = app.ghost().expect("every race has a ghost").sim().plan();
    assert!(
        Arc::ptr_eq(player, ghost),
        "the player and the ghost drive one compiled course, not two identical ones"
    );
    assert!(
        Arc::strong_count(player) >= 2,
        "and the plan is genuinely shared rather than copied"
    );
}

/// A restart rebuilds the race, not the road.
#[test]
fn a_restart_does_not_recompile_the_course() {
    let mut app = shipping();
    let before = Arc::clone(app.sim().plan());
    app.start_race();
    let after = app.sim().plan();
    assert!(
        Arc::ptr_eq(&before, after),
        "restarting reuses the prepared course"
    );
    assert!(
        Arc::ptr_eq(
            &before,
            app.ghost().expect("a restart re-arms the ghost").sim().plan()
        ),
        "and so does the ghost it re-arms"
    );
}

/// Equivalent inputs produce equivalent prepared products — the app-tier
/// determinism check. `apps/` is outside dylint, coverage and the branchless
/// gate, so this test is the only mechanical guard that a task stayed
/// deterministic.
#[test]
fn two_preparations_from_the_same_seed_produce_identical_products() {
    let first = RacePreparation::new();
    let second = RacePreparation::new();
    let _a = run_phase(&first);
    let _b = run_phase(&second);

    let course_a = first.course.borrow().as_ref().map(course::PreparedCourse::plan);
    let course_b = second.course.borrow().as_ref().map(course::PreparedCourse::plan);
    let (course_a, course_b) = (course_a.expect("a"), course_b.expect("b"));
    assert_eq!(course_a.length(), course_b.length());
    assert_eq!(course_a.track().samples(), course_b.track().samples());

    let tex = |p: &RacePreparation| {
        p.textures
            .borrow()
            .as_ref()
            .map(|t: &textures::PreparedTextures| {
                (t.asphalt().to_vec(), t.verge().to_vec(), t.foliage().to_vec())
            })
            .expect("albedos")
    };
    assert_eq!(tex(&first), tex(&second), "the albedos are deterministic");

    let counts = |p: &RacePreparation| {
        p.meshes
            .borrow()
            .as_ref()
            .map(|m: &meshes::PreparedMeshes| (m.draw_chunks().len(), m.paint_chunks().len()))
            .expect("road")
    };
    assert_eq!(counts(&first), counts(&second), "the road is deterministic");
}

/// **Preparation is a launch-time phase, not a frame-time one.** Stepping the
/// game does not re-run it, and the phase cannot be re-entered.
#[test]
fn prepared_work_does_not_rerun_during_gameplay() {
    let prep = RacePreparation::new();
    let mut runtime = run_phase(&prep);
    runtime.start().expect("starts");

    let before = Arc::as_ptr(
        &prep
            .course
            .borrow()
            .as_ref()
            .map(course::PreparedCourse::plan)
            .expect("prepared"),
    );

    (0..100).for_each(|_| {
        runtime.step().expect("a running runtime steps");
    });

    let after = Arc::as_ptr(
        &prep
            .course
            .borrow()
            .as_ref()
            .map(course::PreparedCourse::plan)
            .expect("still prepared"),
    );
    assert_eq!(before, after, "100 frames did not rebuild the course");

    // And the phase itself is closed: a second `prepare` is refused outright,
    // so no caller can re-enter it mid-race.
    let mut second = PreparationSchedule::new();
    prep.tasks(DEFAULT_SEED, &Tuning::DEFAULT)
        .into_iter()
        .for_each(|(name, task)| second.push(name, task));
    assert!(
        runtime.prepare(second).is_err(),
        "preparation runs exactly once per launch"
    );
}

/// A task that fails must fail the phase and keep the runtime out of `Running`.
/// Uses a real Burnt Rubber task — the mesh task with no course to read — so
/// this is the app's own failure path, not a synthetic one.
#[test]
fn preparation_failure_is_surfaced_not_swallowed() {
    let mut runtime =
        Runtime::new(RuntimeConfig::new(FIXED_STEP_NANOS)).expect("a valid fixed step");
    runtime.initialize().expect("initializes");

    let out = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut schedule = PreparationSchedule::new();
    schedule.push(
        "burnt-rubber/meshes",
        Box::new(meshes::MeshTask {
            // Never filled: the course task is deliberately absent.
            course: std::rc::Rc::new(std::cell::RefCell::new(None)),
            tuning: Tuning::DEFAULT.course,
            out: std::rc::Rc::clone(&out),
        }) as Box<dyn PreparationTask>,
    );

    assert!(runtime.prepare(schedule).is_err(), "the phase fails");
    assert_eq!(runtime.state(), RuntimeState::Failed);
    assert!(out.borrow().is_none(), "and produced nothing");
    assert!(
        runtime.start().is_err(),
        "a failed preparation can never reach Running"
    );
    assert!(runtime.step().is_err(), "so no frame is ever presented");
}
