//! Dragging a dog, through the real layout and the real engine.
//!
//! `src/herd.rs` proves the disturbance in isolation, on a row of anchors four
//! units apart. What only the real field can answer is whether the thing it was
//! built against actually holds: that the crowd the *ring layout* produces is
//! out of contact at rest, that a drag moves the bones the frame submits, and —
//! the claim the whole design was chosen for — that a released dog comes back
//! **exactly** into step rather than approximately near it.
//!
//! That last one is asserted the strongest way available: a field that has been
//! hauled about and let go is compared, transform by transform, against a field
//! nobody ever touched, at the same tick. Equal, not close.

use axiom::prelude::RunningApp;
use axiom_dog::{crowd_space, headless_animated, DebugView, Dial, Herd, Ray, SceneConfig, Stage};
use axiom_math::Vec3;

/// A small field — three rings — so a test can run a thousand ticks of settling
/// without paying for a hundred dogs on every one of them. The disturbance is
/// indifferent to the crowd size; the *layout* claim below is the one that
/// sweeps the dials properly.
fn small() -> SceneConfig {
    SceneConfig::defaults().with(Dial::RingCount, 3.0)
}

/// A ray pointing straight down at a world point, from well above the terrain —
/// the simplest pointer a test can hold.
fn from_above(target: Vec3) -> Ray {
    Ray {
        origin: Vec3::new(target.x, target.y + 200.0, target.z),
        direction: Vec3::new(0.0, -1.0, 0.0),
    }
}

/// Every dog's bones at `tick`, as the frame submits them.
fn drawn(app: &mut RunningApp, tick: u64) -> Vec<[f32; 16]> {
    app.tick(tick).draws().iter().map(|draw| draw.world()).collect()
}

#[test]
fn a_dragged_dog_moves_as_one_rigid_animal_and_its_neighbours_stay_put() {
    let config = small();
    let (_, installed) = headless_animated(config.variant(), DebugView::Shaded, &config);
    let animation = &installed.animation;
    let mut herd = Herd::undisturbed();

    let anchors = animation.anchors(1);
    herd.settle(&anchors, 1, crowd_space(&config));
    assert!(herd.grab(from_above(anchors[0].position)), "the pointer missed the dog");
    assert_eq!(herd.holding(), Some(0));
    // Haul it 30 units out along +X, well clear of anything.
    herd.drag(from_above(Vec3::new(
        anchors[0].position.x + 30.0,
        anchors[0].position.y,
        anchors[0].position.z,
    )));
    herd.settle(&anchors, 2, crowd_space(&config));

    let bones = animation.bone_count();
    let still = animation.transforms(2);
    let moved = animation.displaced(2, &herd);
    assert_eq!(still.len(), moved.len());

    // The dragged dog moved as one piece: every one of its bones by the *same*
    // vector, and not one of them turned. That is the mechanical statement of
    // "the gait was never touched" — the pose is the pose the ring gave it,
    // carried somewhere else.
    let slide = moved[0].translation.subtract(still[0].translation);
    assert!(slide.length() > 25.0, "the dog barely moved: {slide:?}");
    (0..bones).for_each(|bone| {
        let step = moved[bone].translation.subtract(still[bone].translation);
        assert!(
            step.distance(slide) < 1.0e-4,
            "bone {bone} moved by {step:?}, not {slide:?} — the dog deformed"
        );
        assert_eq!(
            moved[bone].rotation, still[bone].rotation,
            "bone {bone} turned"
        );
    });

    // ...and every other dog in the field is exactly where it was. A drag is one
    // dog's business until a collision makes it someone else's.
    assert_eq!(moved[bones..], still[bones..]);
}

#[test]
fn a_dog_shoved_into_the_crowd_pushes_its_neighbour_out_of_the_way() {
    let config = small();
    let (_, installed) = headless_animated(config.variant(), DebugView::Shaded, &config);
    let animation = &installed.animation;
    let space = crowd_space(&config);
    let mut herd = Herd::undisturbed();

    let anchors = animation.anchors(1);
    herd.settle(&anchors, 1, space);
    // Find the nearest other dog to dog 0 and drive dog 0 straight onto it.
    let (victim, _) = anchors
        .iter()
        .enumerate()
        .skip(1)
        .map(|(dog, at)| (dog, at.position.distance(anchors[0].position)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("the field has more than one dog");
    assert!(herd.grab(from_above(anchors[0].position)));
    herd.drag(from_above(anchors[victim].position));
    (2..40u64).for_each(|tick| herd.settle(&animation.anchors(tick), tick, space));

    // The neighbour has been pushed off its own place, and the two are no longer
    // inside one another.
    let shoved = herd.displacement(victim);
    assert!(
        shoved.length() > space.half_width * 0.5,
        "the neighbour was walked through rather than shoved: {shoved:?}"
    );
    let anchors = animation.anchors(39);
    let apart = anchors[0]
        .position
        .add(herd.displacement(0))
        .distance(anchors[victim].position.add(herd.displacement(victim)));
    assert!(
        apart > space.half_width,
        "two dogs are standing inside each other: {apart} apart"
    );
}

#[test]
fn a_field_that_has_been_hauled_about_comes_back_bit_for_bit_in_step() {
    // The claim the whole design exists for. Two identical fields: one is
    // dragged, shoved and released; the other is never touched. Run both to the
    // same tick and the frames must agree *exactly* — same bones, same places,
    // same point in the trot.
    let config = small();
    let (mut disturbed, mut installed) =
        headless_animated(config.variant(), DebugView::Shaded, &config);
    let (mut pristine, mut untouched) =
        headless_animated(config.variant(), DebugView::Shaded, &config);
    let herd = &mut Herd::undisturbed();
    let calm = &mut Herd::undisturbed();

    // Grab a dog and drag it across the middle of the field, into the path of
    // the inner ring, so the shove propagates through the crowd as well.
    installed.animate(&mut disturbed, 1, &config, Stage::Field, herd);
    let anchors = installed.animation.anchors(1);
    assert!(herd.grab(from_above(anchors[0].position)));
    herd.drag(from_above(Vec3::new(0.0, anchors[0].position.y, 0.0)));
    (2..30u64).for_each(|tick| {
        installed.animate(&mut disturbed, tick, &config, Stage::Field, herd);
    });
    assert!(herd.disturbed(), "the drag did nothing at all");
    herd.release();

    // Let it walk home. The return is measured in ticks, so the frames may
    // stride: 40 frames of 30 ticks is the same journey as 1200 of one.
    (1..=40u64).for_each(|frame| {
        installed.animate(&mut disturbed, 30 + frame * 30, &config, Stage::Field, herd);
    });
    assert!(!herd.disturbed(), "the field never finished coming home");

    let settled = 30 + 40 * 30;
    untouched.animate(&mut pristine, settled, &config, Stage::Field, calm);
    assert_eq!(
        drawn(&mut disturbed, settled),
        drawn(&mut pristine, settled),
        "the disturbed field is not the field that was never touched"
    );
}

#[test]
fn a_dog_hauled_across_a_still_eight_ring_field_still_gets_all_the_way_home() {
    // The case that caught this on screen. The full field is eight chains of
    // dogs laid nose to tail, and a chain is a wall with gaps far narrower than
    // a dachshund — so a dog dragged from the outer ring to the middle has to
    // cross several of them to get back. With the walk stopped (`speed = 0`)
    // nothing shakes the lattice loose either, which makes this the hardest
    // arrangement the app can be put in.
    //
    // Under an even split of every push it does not get home at all: it wedges
    // in the first chain it meets and sits there. See `Herd::shares`.
    let config = SceneConfig::defaults().with(Dial::Speed, 0.0);
    let (mut app, mut installed) = headless_animated(config.variant(), DebugView::Shaded, &config);
    let herd = &mut Herd::undisturbed();

    installed.animate(&mut app, 1, &config, Stage::Field, herd);
    let anchors = installed.animation.anchors(1);
    // The outermost dog: the one with the most crowd between it and the middle.
    let (outermost, home) = anchors
        .iter()
        .enumerate()
        .map(|(dog, at)| (dog, at.position))
        .max_by(|a, b| {
            Vec3::new(a.1.x, 0.0, a.1.z)
                .length()
                .total_cmp(&Vec3::new(b.1.x, 0.0, b.1.z).length())
        })
        .expect("the field has dogs");
    assert!(herd.grab(from_above(home)));
    assert_eq!(herd.holding(), Some(outermost));

    // Haul it into the middle of the field, through every ring on the way.
    (1..=30u64).for_each(|step| {
        let across = home.mul_scalar(1.0 - step as f32 / 30.0);
        herd.drag(from_above(Vec3::new(across.x, home.y, across.z)));
        installed.animate(&mut app, 1 + step, &config, Stage::Field, herd);
    });
    assert!(
        herd.displacement(outermost).length() > 40.0,
        "the drag never crossed the field"
    );
    herd.release();

    // ...and let go, it comes all the way back — not most of the way, and not to
    // a pocket in the crowd that happens to balance the pull.
    // A dog hauled the full 80-unit radius needs ~445 ticks to decay under the
    // settle threshold, plus whatever the crowd costs it on the way; 600 frames
    // is that with margin. It is a bound on the arithmetic, not a hope.
    (1..=600u64).for_each(|frame| {
        installed.animate(&mut app, 31 + frame, &config, Stage::Field, herd);
    });
    // ...and it is back on its own ring, not merely somewhere tidy.
    let home_again = anchors[outermost].position.add(herd.displacement(outermost));
    assert!(
        (Vec3::new(home_again.x, 0.0, home_again.z).length()
            - Vec3::new(home.x, 0.0, home.z).length())
        .abs()
            < 1.0e-3,
        "the dog settled somewhere other than its own ring"
    );
    assert!(
        !herd.disturbed(),
        "the field jammed: {} dogs are still out of place, the worst by {}",
        (0..anchors.len())
            .filter(|dog| herd.displacement(*dog).length() > 0.0)
            .count(),
        (0..anchors.len())
            .map(|dog| herd.displacement(dog).length())
            .fold(0.0f32, f32::max)
    );
}

#[test]
fn the_study_stage_calms_whatever_the_field_was_left_in() {
    let config = small();
    let (mut app, mut installed) = headless_animated(config.variant(), DebugView::Shaded, &config);
    let herd = &mut Herd::undisturbed();

    installed.animate(&mut app, 1, &config, Stage::Field, herd);
    let anchors = installed.animation.anchors(1);
    assert!(herd.grab(from_above(anchors[2].position)));
    herd.drag(from_above(Vec3::new(0.0, anchors[2].position.y, 0.0)));
    installed.animate(&mut app, 2, &config, Stage::Field, herd);
    assert!(herd.disturbed());

    // One dog, suspended at the origin: there is no crowd to disturb, so the
    // field is put down on the way in and comes back pristine.
    installed.animate(&mut app, 3, &config, Stage::Study, herd);
    assert!(!herd.disturbed(), "the study kept the field's disturbance");
    assert_eq!(herd.holding(), None, "the study kept hold of a dog");

    let (mut fresh, mut clean) = headless_animated(config.variant(), DebugView::Shaded, &config);
    installed.animate(&mut app, 4, &config, Stage::Field, herd);
    clean.animate(&mut fresh, 4, &config, Stage::Field, &mut Herd::undisturbed());
    assert_eq!(drawn(&mut app, 4), drawn(&mut fresh, 4));
}
