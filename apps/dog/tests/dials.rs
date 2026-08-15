//! The slider panel's proof suite.
//!
//! The panel is generated from [`Dial::ALL`], so these are the claims that keep a
//! *generated* control honest:
//!
//! 1. **Every live dial does something.** Moved off its default, it changes the
//!    posed output — the transforms, or the size of the crowd. A slider that
//!    silently does nothing fails here rather than on the page.
//! 2. **No dial can be driven into a broken scene.** Every extreme, and the
//!    combinations that compound, still produce a field whose rings clear each
//!    other, whose dogs do not overlap along an arc, and whose crowd fits the
//!    instance pool. (The limb-reach half of that lives in
//!    `tests/locomotion.rs`, which walks a lap of real terrain at every corner.)
//! 3. **The leg dial moves the leg, the solver AND the body together.** A limb
//!    that looks longer but solves at its old length is a bug, not a slider, so
//!    the drawn bone scale, the solved bone length and the standing height are
//!    all measured at both ends of the dial.
//! 4. **The dials are inputs, not state.** At a fixed non-default configuration,
//!    tick `N` poses identically twice; and re-authoring an installed scene at a
//!    new configuration moves the *drawn* instance count to match — which is what
//!    proves the pooled crowd is genuinely being shown and retired rather than
//!    parked somewhere the renderer still pays for.

use axiom_dog::{
    dog_limbs, dog_parts, dog_total, headless_animated, rings, Animation, DebugView, Dial,
    SceneConfig, Stage, MAX_DOGS,
};

fn animation(config: &SceneConfig) -> Animation {
    Animation::new(dog_parts(config.variant()).expect("the dog rigs"), config)
        .expect("the rings build")
}

/// The pose of the whole field at `tick`, as a flat list of numbers — enough to
/// tell two configurations apart without caring which bone moved.
fn posed(config: &SceneConfig, tick: u64) -> Vec<f32> {
    animation(config)
        .transforms(tick)
        .iter()
        .flat_map(|t| {
            [
                t.translation.x,
                t.translation.y,
                t.translation.z,
                t.rotation.x,
                t.rotation.y,
                t.rotation.z,
                t.rotation.w,
                t.scale.x,
                t.scale.y,
                t.scale.z,
            ]
        })
        .collect()
}

#[test]
fn every_live_dial_changes_the_posed_field() {
    let base = SceneConfig::defaults();
    // Tick 40 rather than 0: at tick 0 nothing has travelled, so a *speed* dial
    // legitimately shows no difference yet. Forty ticks is eight units of walk at
    // the default speed and twenty-four at the top of the dial.
    let reference = posed(&base, 40);
    for dial in Dial::ALL.into_iter().filter(|dial| dial.spec().live) {
        let spec = dial.spec();
        // Both ends. A dial whose *derived* value is capped at one end — the
        // stride is, by the reach the leg has — is still doing its job as long as
        // it moves the field somewhere in its range; a dial that moves it at
        // NEITHER end is inert, and that is what this fails on.
        let ends: Vec<(f32, bool)> = [spec.min, spec.max]
            .into_iter()
            .map(|value| {
                let after = posed(&base.with(dial, value), 40);
                (value, after.len() != reference.len() || after != reference)
            })
            .collect();
        assert!(
            ends.iter().any(|(_, changed)| *changed),
            "{} changed nothing about the posed field at either end of its range \
             ({} .. {}) — the slider is inert",
            spec.key,
            spec.min,
            spec.max
        );
        println!(
            "[dial] {:<8} {:>7.2} .. {:>7.2}  moves the field at {}",
            spec.key,
            spec.min,
            spec.max,
            ends.iter()
                .filter(|(_, changed)| *changed)
                .map(|(value, _)| format!("{value}"))
                .collect::<Vec<String>>()
                .join(" and ")
        );
    }
}

#[test]
fn the_non_live_dial_is_the_only_one_that_needs_a_reload() {
    let live: Vec<&str> = Dial::ALL
        .into_iter()
        .filter(|dial| !dial.spec().live)
        .map(|dial| dial.spec().key)
        .collect();
    assert_eq!(
        live,
        vec!["detail"],
        "the geometry dial is the only one the live backend cannot answer by re-posing"
    );
    // ...and it really does change the geometry it claims to.
    let base = SceneConfig::defaults();
    let dense = base.with(Dial::Detail, 2.0);
    assert_ne!(base.variant(), dense.variant());
}

#[test]
fn the_leg_dial_moves_the_bone_the_solver_and_the_body_together() {
    let rig = dog_parts(SceneConfig::defaults().variant()).expect("the dog rigs");
    let limb = dog_limbs()[0];
    let upper = rig.index_of(limb.upper).expect("the shoulder bone");
    let lower = rig.index_of(limb.lower).expect("the elbow bone");
    let spine = rig.index_of("dog-spine").expect("the spine");

    let mut heights: Vec<f32> = Vec::new();
    let mut lengths: Vec<f32> = Vec::new();
    for leg in [0.70_f32, 1.0, 1.80] {
        let config = SceneConfig::defaults().with(Dial::LegLength, leg);
        let animation = animation(&config);
        let posed = animation.transforms(0);
        let span = config.dog_scale() * config.leg_scale();

        // The bone is DRAWN longer: a leg bone runs down its own local `-Z`, so
        // its `z` scale is the leg's world span while `x`/`y` stay the animal's.
        let drawn = posed[upper].scale;
        assert!(
            (drawn.z - span).abs() < 1.0e-3,
            "at leg {leg} the shoulder bone is drawn at z-scale {} for a {span}-unit span",
            drawn.z
        );
        assert!((drawn.x - config.dog_scale()).abs() < 1.0e-3);

        // The bone is SOLVED at the same length: hip to elbow is exactly the
        // upper bone, scaled. A limb that looked longer but solved at its old
        // length would fail right here.
        let solved = posed[upper].translation.distance(posed[lower].translation);
        assert!(
            (solved - limb.len_upper * span).abs() < 1.0e-2,
            "at leg {leg} the solver placed the elbow {solved} from the shoulder, \
             not {}",
            limb.len_upper * span
        );
        lengths.push(solved);

        // And the BODY stands on it: a longer leg lifts the barrel rather than
        // folding further under an unmoved one.
        heights.push(posed[spine].translation.y);
        println!(
            "[leg] {leg:.2}  span {span:>5.2}  upper bone {solved:>5.2}  spine y {:>6.2}",
            posed[spine].translation.y
        );
    }
    assert!(
        lengths[0] < lengths[1] && lengths[1] < lengths[2],
        "the solved bone did not lengthen with the dial: {lengths:?}"
    );
    assert!(
        heights[0] < heights[1] && heights[1] < heights[2],
        "the body did not stand up with the leg: {heights:?}"
    );
}

/// The size dial is coupled to the leg dial by a derived floor: below a leg of
/// [`MIN_LEG_SPAN`] world units the terrain's roll is more than any stride
/// reduction can absorb, so a small dog is given proportionally longer legs
/// instead of being allowed into a scene whose paws slide.
#[test]
fn the_size_dial_floors_the_leg_rather_than_allowing_an_unwalkable_animal() {
    let short = SceneConfig::defaults().with(Dial::LegLength, Dial::LegLength.spec().min);
    for size in [6.0_f32, 8.0, 10.0, 16.0] {
        let config = short.with(Dial::DogSize, size);
        let span = config.dog_scale() * config.leg_scale();
        assert!(
            span >= 6.0 - 1.0e-4,
            "a {size}-unit dog on the shortest legs walks on a {span}-unit leg"
        );
        // The dial's own request is still honoured wherever it can be.
        assert!(config.leg_scale() >= short.raw(Dial::LegLength));
        println!("[size] {size:>5.1}  leg scale {:.2}  leg span {span:.2}", config.leg_scale());
    }
}

#[test]
fn no_extreme_of_any_dial_produces_an_illegal_field() {
    // Every dial at each end, one at a time, plus the two ends of the ring dials
    // together — the combination that decides whether the crowd fits the pool.
    let corners: Vec<SceneConfig> = Dial::ALL
        .into_iter()
        .flat_map(|dial| {
            let spec = dial.spec();
            [
                SceneConfig::defaults().with(dial, spec.min),
                SceneConfig::defaults().with(dial, spec.max),
            ]
        })
        .chain([
            SceneConfig::defaults()
                .with(Dial::RingCount, 10.0)
                .with(Dial::RingSpacing, 3.0)
                .with(Dial::DogGap, 0.5)
                .with(Dial::InnerRadius, 18.0)
                .with(Dial::DogSize, 6.0),
            SceneConfig::defaults()
                .with(Dial::RingCount, 10.0)
                .with(Dial::RingSpacing, 20.0)
                .with(Dial::DogGap, 20.0)
                .with(Dial::InnerRadius, 60.0)
                .with(Dial::DogSize, 16.0),
        ])
        .collect();

    for config in corners {
        let laid = rings(&config);
        assert!(!laid.is_empty(), "an empty field at {:?}", config.to_query());
        // Rings clear each other.
        laid.windows(2).for_each(|pair| {
            let air = pair[1].band(&config).0 - pair[0].band(&config).1;
            assert!(air > 0.0, "rings overlap by {air} at {:?}", config.to_query());
        });
        // Dogs clear each other along their own arc.
        laid.iter().for_each(|ring| {
            let arc = ring.circumference() / ring.count(&config) as f32;
            assert!(
                arc >= config.dog_length(),
                "ring {} packs a {}-unit dog into {arc} units at {:?}",
                ring.index,
                config.dog_length(),
                config.to_query()
            );
        });
        // The crowd fits the pool that was actually spawned.
        assert!(dog_total(&config) <= MAX_DOGS);
        // And the whole thing poses to finite numbers.
        assert!(
            posed(&config, 313).iter().all(|value| value.is_finite()),
            "a non-finite pose at {:?}",
            config.to_query()
        );
    }
}

#[test]
fn a_fixed_non_default_configuration_is_replayable() {
    let config = SceneConfig::defaults()
        .with(Dial::Speed, 0.44)
        .with(Dial::Stride, 2.7)
        .with(Dial::Duty, 0.71)
        .with(Dial::LegLength, 1.35)
        .with(Dial::DogSize, 12.5)
        .with(Dial::InnerRadius, 34.5)
        .with(Dial::RingSpacing, 9.5)
        .with(Dial::RingCount, 6.0)
        .with(Dial::DogGap, 3.5)
        .with(Dial::Lean, 0.17)
        .with(Dial::Direction, -1.0);
    for tick in [0u64, 13, 907, 12_345] {
        assert_eq!(posed(&config, tick), posed(&config, tick), "tick {tick} drifted");
    }
    // ...and it is genuinely a different scene from the default one.
    assert_ne!(posed(&config, 907), posed(&SceneConfig::defaults(), 907));
    // The query string is the whole of what distinguishes them.
    assert_eq!(SceneConfig::from_query(&config.to_query()), config);
    println!("[replay] {} dogs at ?{}", dog_total(&config), config.to_query());
}

/// The end-to-end pooling claim, through the real engine: re-authoring an
/// installed scene at a new ring configuration moves the number of instances the
/// frame actually draws.
///
/// This is the test that would catch a retired dog still being submitted (parked
/// somewhere the renderer pays for) or a newly shown one never appearing (the
/// pool spawned too small).
#[test]
fn re_authoring_at_a_new_layout_moves_the_drawn_instance_count() {
    let opening = SceneConfig::defaults();
    let (mut app, mut installed) =
        headless_animated(opening.variant(), DebugView::Shaded, &opening);
    let bones = installed.bone_count;
    let drawn = |app: &mut axiom::prelude::RunningApp, tick: u64| app.tick(tick).draws().len();

    assert_eq!(drawn(&mut app, 0), 1 + bones * dog_total(&opening));

    // Fewer rings: the retired slots leave the frame entirely.
    let fewer = opening.with(Dial::RingCount, 3.0);
    installed.animate(&mut app, 1, &fewer, Stage::Field);
    assert_eq!(drawn(&mut app, 1), 1 + bones * dog_total(&fewer));
    assert!(dog_total(&fewer) < dog_total(&opening));

    // More dogs than the opening scene: the pool has them, so they come back.
    // A smaller dog needs less arc and less radial pitch, so ten rings fit the
    // plate where eight of the authored size is already the terrain's ceiling.
    let more = opening
        .with(Dial::DogSize, 8.0)
        .with(Dial::RingCount, 10.0);
    installed.animate(&mut app, 2, &more, Stage::Field);
    assert_eq!(drawn(&mut app, 2), 1 + bones * dog_total(&more));
    assert!(dog_total(&more) > dog_total(&opening));

    // And back to where it started, exactly.
    installed.animate(&mut app, 3, &opening, Stage::Field);
    assert_eq!(drawn(&mut app, 3), 1 + bones * dog_total(&opening));
    println!(
        "[pool] {} -> {} -> {} -> {} dogs of a {MAX_DOGS}-slot pool",
        dog_total(&opening),
        dog_total(&fewer),
        dog_total(&more),
        dog_total(&opening)
    );
}
