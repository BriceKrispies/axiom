//! The stage switch, through the real engine.
//!
//! `src/stage.rs` and `src/study.rs` prove what a stage *means* in isolation —
//! how many dogs, whether they walk, where the camera opens, and that the still
//! pose is a pure function of the configuration. What is left to prove is the
//! part only a running app can answer: that switching stage changes what the
//! frame actually **draws**, and that it does so out of the *same* instance pool
//! rather than by building a second scene.
//!
//! That is the claim the page rests on. Geometry is uploaded once at bind (see
//! `NOTES.md` §8), so a study dog that needed geometry of its own could not
//! exist at all; these tests are what keep the study honest about being one of
//! the field's own dogs, held still.

use axiom_dog::{dog_total, headless_animated, DebugView, Dial, Herd, SceneConfig, Stage};

/// How many instances the frame draws at `tick`.
fn drawn(app: &mut axiom::prelude::RunningApp, tick: u64) -> usize {
    app.tick(tick).draws().len()
}

#[test]
fn the_study_draws_one_dog_and_no_ground_and_the_field_comes_back_whole() {
    let config = SceneConfig::defaults();
    let (mut app, mut installed) = headless_animated(config.variant(), DebugView::Shaded, &config);
    // Nobody has dragged anything: these are claims about the walk itself.
    let herd = &mut Herd::undisturbed();
    let bones = installed.bone_count;
    let field = 1 + bones * dog_total(&config);

    // The opening stage: the terrain plus every dog the layout placed.
    assert_eq!(drawn(&mut app, 0), field);

    // The study: exactly one dog's bones, and nothing else at all — the whole
    // crowd and the terrain are retired, not merely moved off camera.
    installed.animate(&mut app, 1, &config, Stage::Study, herd);
    assert_eq!(drawn(&mut app, 1), bones, "the study is one dog, on nothing");

    // ...and back, exactly. Nothing was despawned, so nothing has to be rebuilt.
    installed.animate(&mut app, 2, &config, Stage::Field, herd);
    assert_eq!(drawn(&mut app, 2), field);

    // A second round trip lands on the same two numbers: the visibility
    // reconciliation is idempotent, not a one-way ratchet.
    installed.animate(&mut app, 3, &config, Stage::Study, herd);
    assert_eq!(drawn(&mut app, 3), bones);
    installed.animate(&mut app, 4, &config, Stage::Field, herd);
    assert_eq!(drawn(&mut app, 4), field);
    println!("[stages] field {field} instances, study {bones} instances");
}

#[test]
fn the_study_dog_does_not_move_however_far_the_ticks_run() {
    let config = SceneConfig::defaults();
    let (mut app, mut installed) = headless_animated(config.variant(), DebugView::Shaded, &config);
    // Nobody has dragged anything: these are claims about the walk itself.
    let herd = &mut Herd::undisturbed();

    let posed = |app: &mut axiom::prelude::RunningApp,
                 installed: &mut axiom_dog::InstalledScene,
                 herd: &mut Herd,
                 tick: u64| {
        installed.animate(app, tick, &config, Stage::Study, herd);
        app.tick(tick)
            .draws()
            .iter()
            .map(|draw| draw.world())
            .collect::<Vec<_>>()
    };

    let first = posed(&mut app, &mut installed, herd, 1);
    let much_later = posed(&mut app, &mut installed, herd, 9_000);
    assert_eq!(first, much_later, "the study dog moved with the clock");

    // The field, by contrast, is somewhere else entirely over the same span —
    // which is what makes the comparison above mean something. (The engine's
    // frame sequence must strictly increase, so the walking pair is taken after
    // the still one rather than at the same two ticks.)
    installed.animate(&mut app, 9_001, &config, Stage::Field, herd);
    let walking = app
        .tick(9_001)
        .draws()
        .iter()
        .map(|d| d.world())
        .collect::<Vec<_>>();
    installed.animate(&mut app, 18_000, &config, Stage::Field, herd);
    let walked = app
        .tick(18_000)
        .draws()
        .iter()
        .map(|d| d.world())
        .collect::<Vec<_>>();
    assert_ne!(walking, walked, "the field did not walk");
}

#[test]
fn a_gait_dial_still_re_poses_the_study_but_a_ring_dial_cannot_touch_it() {
    let config = SceneConfig::defaults();
    let (mut app, mut installed) = headless_animated(config.variant(), DebugView::Shaded, &config);
    // Nobody has dragged anything: these are claims about the walk itself.
    let herd = &mut Herd::undisturbed();
    let bones = installed.bone_count;

    let study = |app: &mut axiom::prelude::RunningApp,
                 installed: &mut axiom_dog::InstalledScene,
                 herd: &mut Herd,
                 config: &SceneConfig,
                 tick: u64| {
        installed.animate(app, tick, config, Stage::Study, herd);
        app.tick(tick)
            .draws()
            .iter()
            .map(|draw| draw.world())
            .collect::<Vec<_>>()
    };

    let standing = study(&mut app, &mut installed, herd, &config, 1);
    // A layout dial lays out a field. The study is not a field, so it is
    // untouched — and it is still exactly one dog afterwards.
    let relaid = study(
        &mut app,
        &mut installed,
        herd,
        &config.with(Dial::RingCount, 2.0).with(Dial::InnerRadius, 50.0),
        2,
    );
    assert_eq!(standing, relaid, "a ring dial moved the study dog");
    assert_eq!(relaid.len(), bones);

    // A gait dial is the animal, not the field, so it does re-pose it.
    let taller = study(&mut app, &mut installed, herd, &config.with(Dial::LegLength, 1.8), 3);
    assert_ne!(standing, taller, "the leg dial did not re-pose the study dog");
}
