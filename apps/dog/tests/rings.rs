//! The concentric field's proof suite.
//!
//! Everything about the crowd is checkable natively, because a dog's pose is a
//! pure function of `(tick, config)` and its identity is three numbers. These are
//! the claims:
//!
//! 1. **The field is derived, not typed** — the ring count is the radial pitch
//!    stepped from the innermost radius, and each ring's dog count is its
//!    circumference divided by the room one dog needs.
//! 2. **No two rings intersect** — the radial band one ring's dogs occupy (its
//!    radius, widened by half a dog's width and by the outward bulge a rigid
//!    body makes on a curve) clears the next ring's band, and every dog stands on
//!    the terrain rather than in the air over it. Held at both ends of every ring
//!    dial, not only at the defaults.
//! 3. **Every ring turns the way the direction dial says** — the sign of
//!    `(position − centre) × heading`, measured on the **real posed bones** and
//!    on the direction the body actually travels between two ticks, at both
//!    settings of the dial.
//! 4. **The rainbow is bounded, balanced and readable** — the whole field is
//!    painted from a fixed palette of 18 coats, each worn within one dog of every
//!    other (which is what the fixed-coat instance pool can honour), and no dog
//!    shares a coat with the dog in front of it or behind it.
//! 5. **It is deterministic** — at a fixed configuration, tick `N` posed twice is
//!    byte-equal; tick `N` and tick `N+1` are not.
//! 6. **The geometry is shared, and so are the materials** — the scene registers
//!    `bone_count` distinct dog meshes (not `bone_count × dogs`), and the frame
//!    it produces is at most `bone_count × PALETTE_SIZE + 1` batches, however
//!    many dogs are walking.

use axiom_math::Vec3;
use axiom_dog::{
    build_scene, dog_parts, dog_total, ground_y, headless_app, inner_radius, outer_clearance,
    palette_color, ring_count, ring_spacing, rings, Animation, DebugView, Dial, SceneConfig,
    SceneVariant, MAX_DOGS, PALETTE_SIZE, TERRAIN_HALF_EXTENT,
};

/// The bone count the whole field instances.
const BONES: usize = 23;

fn animation(config: &SceneConfig) -> Animation {
    Animation::new(
        dog_parts(config.variant()).expect("the dog rigs"),
        config,
    )
    .expect("the rings build")
}

/// The horizontal part of a vector — every direction claim here is about the
/// plan view, and the gait's bob has no business in it.
fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// The `y` component of `a × b`, which is the signed area that says which way
/// round a circle a heading goes.
fn cross_y(a: Vec3, b: Vec3) -> f32 {
    a.z * b.x - a.x * b.z
}

/// How far apart two coats read, as the sum of their channel differences.
fn coat_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum()
}

#[test]
fn the_opening_field_fills_the_ground_from_the_gait_floor_to_the_terrain_rim() {
    let config = SceneConfig::defaults();
    // Eight rings, 7.75 units apart, from the tightest curve the gait is tuned
    // for out to the widest circle that still leaves clear ground before the rim.
    assert_eq!(ring_count(&config), 8);
    let laid = rings(&config);
    assert_eq!(laid.len(), 8);
    assert_eq!(laid[0].radius, inner_radius(&config));
    assert_eq!(
        laid.iter().map(|ring| ring.radius).collect::<Vec<f32>>(),
        vec![26.0, 33.75, 41.5, 49.25, 57.0, 64.75, 72.5, 80.25]
    );
    // The pitch is bounded by the dog's WIDTH plus its bulge on a curve, not by
    // its length — which is the whole reason eight rings fit at all.
    let pitch = ring_spacing(&config);
    assert!(pitch < config.dog_length() * 0.5);
    assert!(pitch > config.dog_width() + laid[0].bulge(&config));
    // ...and the outermost ring leaves half a dog's length of ground to the rim.
    let clear = outer_clearance(&config);
    assert!(
        clear > config.dog_length() * 0.5,
        "only {clear} units of ground beyond the outer ring of a {TERRAIN_HALF_EXTENT}-unit plate"
    );
    println!(
        "[field] 8 rings, 26 .. 80.25 step {pitch}, {clear:.1} units of rim clearance"
    );
}

#[test]
fn each_ring_holds_the_number_of_dogs_its_circumference_pays_for() {
    let config = SceneConfig::defaults();
    let animation = animation(&config);
    assert_eq!(
        rings(&config)
            .iter()
            .map(|ring| ring.count(&config))
            .collect::<Vec<usize>>(),
        vec![6, 8, 10, 12, 14, 16, 18, 20]
    );
    assert_eq!(dog_total(&config), 104);
    assert_eq!(animation.dog_count(), 104);
    assert_eq!(animation.bone_count(), BONES);

    for ring in rings(&config) {
        let index = ring.index;
        let walkers = animation
            .dogs()
            .iter()
            .filter(|dog| dog.ring == index)
            .count();
        assert_eq!(walkers, ring.count(&config), "ring {index} holds {walkers} dogs");
        // The measured walk is the circle it was authored as.
        let total = animation.path(index).total();
        assert!(
            (total - ring.circumference()).abs() / ring.circumference() < 0.02,
            "ring {index} measures {total}, not ~{}",
            ring.circumference()
        );
        // And its dogs are spaced nose to tail: a realised spacing within a tenth
        // of the target, never closer than the dog is long.
        let spacing = total / walkers as f32;
        assert!(
            (spacing - config.dog_spacing()).abs() < 0.12 * config.dog_spacing(),
            "ring {index} spaces its dogs {spacing} apart"
        );
        assert!(spacing > config.dog_length(), "ring {index} dogs overlap");
        println!(
            "[ring] {index}  radius {:>5.1}  length {total:>7.1}  {walkers:>2} dogs  spacing {spacing:>5.2}  gap {:>4.2}",
            ring.radius,
            spacing - config.dog_length()
        );
    }
}

/// The layout invariants, held where they are actually at risk: at the ends of
/// every ring dial and every dial that scales the animal those rings are laid out
/// for. A pitch that is legal at the defaults and illegal at a 16-unit dog is a
/// scene the panel can be driven into, and this is what stops it.
#[test]
fn no_ring_dial_can_be_driven_into_an_illegal_field() {
    for size in [6.0, 10.0, 16.0] {
        for inner in [18.0, 26.0, 60.0] {
            for pitch in [3.0, 7.75, 20.0] {
                for gap in [0.5, 1.5, 20.0] {
                    for count in [1.0, 8.0, 10.0] {
                        let config = SceneConfig::defaults()
                            .with(Dial::DogSize, size)
                            .with(Dial::InnerRadius, inner)
                            .with(Dial::RingSpacing, pitch)
                            .with(Dial::DogGap, gap)
                            .with(Dial::RingCount, count);
                        let laid = rings(&config);
                        let label =
                            format!("size {size} inner {inner} pitch {pitch} gap {gap} count {count}");
                        assert!(!laid.is_empty(), "{label}: the field is empty");
                        assert!(laid.len() <= count as usize, "{label}: more rings than asked");

                        // Rings never intersect.
                        laid.windows(2).for_each(|pair| {
                            let air = pair[1].band(&config).0 - pair[0].band(&config).1;
                            assert!(air > 0.0, "{label}: rings overlap by {air}");
                        });
                        // The field stays on the terrain, with room to spare.
                        assert!(
                            outer_clearance(&config) >= 0.0,
                            "{label}: the outer ring hangs off the plate"
                        );
                        // No ring packs its dogs closer than the dog is long.
                        laid.iter().for_each(|ring| {
                            let arc = ring.circumference() / ring.count(&config) as f32;
                            assert!(
                                arc >= config.dog_length(),
                                "{label}: ring {} gives each dog {arc} units of arc",
                                ring.index
                            );
                        });
                        // And the crowd never outgrows the instance pool.
                        assert!(dog_total(&config) <= MAX_DOGS, "{label}: pool overflow");
                    }
                }
            }
        }
    }
}

#[test]
fn every_dog_stands_on_the_terrain_on_its_own_ring() {
    let config = SceneConfig::defaults();
    let animation = animation(&config);
    let rig = dog_parts(SceneVariant::Base).expect("the dog rigs");
    let bones = animation.bone_count();
    let paws: Vec<usize> = rig
        .parts()
        .iter()
        .enumerate()
        .filter(|(_, part)| part.name.ends_with("-paw"))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(paws.len(), 4, "the dog has four paws");

    for tick in [0u64, 137, 901] {
        let all = animation.transforms(tick);
        assert_eq!(all.len(), bones * animation.dog_count());
        for (index, dog) in animation.dogs().iter().enumerate() {
            let ring = animation.rings()[dog.ring];
            let body = all[index * bones].translation;
            let radius = flat(body).length();
            assert!(
                (radius - ring.radius).abs() < 6.0,
                "dog {index} left ring {} at tick {tick}: radius {radius}",
                ring.index
            );
            assert!(
                body.x.abs() < TERRAIN_HALF_EXTENT && body.z.abs() < TERRAIN_HALF_EXTENT,
                "dog {index} walked off the terrain at tick {tick}: {body:?}"
            );
            // Every paw is on the ground under it, not floating over it or sunk
            // into it — the rings rise and fall with the terrain.
            for paw in &paws {
                let at = all[index * bones + paw].translation;
                let ground = ground_y(at.x, at.z);
                assert!(
                    (at.y - ground).abs() < 2.0,
                    "dog {index}'s paw rests at y = {} over ground {ground} at tick {tick}",
                    at.y
                );
            }
        }
    }
}

/// The direction test that keeps the whole field's traffic honest.
///
/// A dog's *heading* is taken two ways, and both must agree with the direction
/// dial: the way its body genuinely travelled between two consecutive ticks, and
/// the way its root bone is facing (local `-Z`, the axis every bone is authored
/// down). A sign error anywhere between the dial, the spline parameterization and
/// `aim` shows up here rather than on the page — and it is measured at **both**
/// settings, so the dial is proved to be the only thing that picks a winding.
#[test]
fn the_direction_dial_turns_the_whole_field_and_nothing_else_does() {
    for setting in [1.0_f32, -1.0] {
        let config = SceneConfig::defaults().with(Dial::Direction, setting);
        let animation = animation(&config);
        let bones = animation.bone_count();
        let expected = config.winding().cross_sign();
        for tick in [0u64, 61, 349, 1_004] {
            let now = animation.transforms(tick);
            let next = animation.transforms(tick + 1);
            let turn = |index: usize| {
                let body = now[index * bones].translation;
                cross_y(
                    flat(body),
                    flat(next[index * bones].translation.subtract(body)),
                )
            };
            for (index, dog) in animation.dogs().iter().enumerate() {
                let body = now[index * bones];
                let travelled = flat(next[index * bones].translation.subtract(body.translation));
                assert!(
                    // Derived from the walk-speed dial, not a magic number: this
                    // asserts the dog moved essentially a full tick's worth.
                    travelled.length() > config.travel_per_tick() * 0.8,
                    "dog {index} barely moved between ticks: {travelled:?}"
                );
                assert!(
                    turn(index) * expected > 0.0,
                    "dog {index} on ring {} turns the wrong way at dial {setting}, tick {tick}",
                    dog.ring
                );
                // ...and it faces the way it is going, rather than walking
                // backwards round the right circle.
                let facing = flat(body.rotation.rotate(Vec3::new(0.0, 0.0, -1.0)));
                let along = facing.dot(travelled) / (facing.length() * travelled.length());
                assert!(
                    along > 0.9,
                    "dog {index} faces {along} of the way it travels at tick {tick}"
                );
                assert!(
                    cross_y(flat(body.translation), facing) * expected > 0.0,
                    "dog {index} on ring {} faces the wrong way round",
                    dog.ring
                );
            }
        }
        println!("[turn] dial {setting}: the whole field walks {:?}", config.winding());
    }
}

#[test]
fn the_field_is_painted_from_a_bounded_balanced_palette_no_neighbour_shares() {
    let config = SceneConfig::defaults();
    let scene = build_scene(SceneVariant::Base, &config).expect("the scene builds");
    // Bounded: the palette is a fixed size that the crowd size cannot move.
    assert_eq!(PALETTE_SIZE, 18);
    assert!(PALETTE_SIZE < scene.dogs.len(), "the palette is not bounded");
    assert!(scene.dogs.iter().all(|dog| dog.palette < PALETTE_SIZE));

    // Balanced: every coat is worn within one dog of every other. That is what
    // the fixed-coat instance pool carries, so it is what the layout may ask for.
    let worn: Vec<usize> = (0..PALETTE_SIZE)
        .map(|coat| scene.dogs.iter().filter(|dog| dog.palette == coat).count())
        .collect();
    let most = worn.iter().copied().max().expect("the palette is not empty");
    let least = worn.iter().copied().min().expect("the palette is not empty");
    assert!(most - least <= 1, "{worn:?} is not a balanced palette");
    println!("[coats] {worn:?}");

    for ring in rings(&config) {
        let index = ring.index;
        let chain: Vec<[f32; 3]> = scene
            .dogs
            .iter()
            .filter(|dog| dog.ring == index)
            .map(|dog| dog.color())
            .collect();
        assert_eq!(chain.len(), ring.count(&config));

        // Neighbours differ — including the pair that closes the ring.
        for slot in 0..chain.len() {
            let here = chain[slot];
            let next = chain[(slot + 1) % chain.len()];
            assert!(
                coat_distance(here, next) > 0.05,
                "ring {index} dogs {slot} and {} share a coat: {here:?}",
                (slot + 1) % chain.len()
            );
        }
    }

    // The chain spans the whole hue circle across the field: it visits a colour
    // whose strongest channel is red, one green and one blue.
    let all: Vec<[f32; 3]> = scene.dogs.iter().map(|dog| dog.color()).collect();
    for (channel, name) in [(0usize, "red"), (1, "green"), (2, "blue")] {
        assert!(
            all.iter().any(|rgb| {
                (0..3).all(|other| other == channel || rgb[channel] > rgb[other] + 0.05)
            }),
            "the field never reaches a {name} dog"
        );
    }

    // A dog's colour is a palette lookup, not a property of the dog.
    let first = scene.dogs[0];
    assert_eq!(first.color(), palette_color(first.palette));
}

#[test]
fn the_crowd_is_a_pure_function_of_the_tick_and_the_configuration() {
    // Both the shipping configuration and a deliberately off-default one — the
    // dials are inputs, exactly as the tick is, and neither may leak state.
    let awkward = SceneConfig::defaults()
        .with(Dial::Speed, 0.37)
        .with(Dial::Stride, 3.1)
        .with(Dial::LegLength, 1.45)
        .with(Dial::DogSize, 13.5)
        .with(Dial::InnerRadius, 41.0)
        .with(Dial::RingSpacing, 11.25)
        .with(Dial::RingCount, 4.0)
        .with(Dial::DogGap, 6.0)
        .with(Dial::Direction, -1.0);
    for config in [SceneConfig::defaults(), awkward] {
        let animation = animation(&config);
        // A second animation built from the same config is the same animation.
        let twin = animation.transforms(97);
        assert_eq!(self::animation(&config).transforms(97), twin);
        for tick in [0u64, 1, 97, 4_321] {
            let first = animation.transforms(tick);
            let second = animation.transforms(tick);
            assert_eq!(first.len(), animation.bone_count() * animation.dog_count());
            for (a, b) in first.iter().zip(second.iter()) {
                assert_eq!(a.translation, b.translation, "tick {tick} is not reproducible");
                assert_eq!(a.rotation, b.rotation, "tick {tick} is not reproducible");
                assert_eq!(a.scale, b.scale, "tick {tick} is not reproducible");
            }
            // And the next tick is a different pose — every dog actually moves.
            let next = animation.transforms(tick + 1);
            let moved = first
                .iter()
                .zip(next.iter())
                .filter(|(a, b)| a.translation.distance(b.translation) > 1.0e-4)
                .count();
            assert!(
                moved > first.len() / 2,
                "only {moved} of {} bones moved between tick {tick} and {}",
                first.len(),
                tick + 1
            );
        }
        println!(
            "[determinism] {} dogs × {} bones reproduce exactly",
            animation.dog_count(),
            animation.bone_count()
        );
    }
}

/// The engineering claim, both halves: one dog's geometry and eighteen coats
/// drive the whole field.
#[test]
fn the_whole_crowd_shares_one_dogs_geometry_and_one_palette() {
    let config = SceneConfig::defaults();
    let scene = build_scene(SceneVariant::Base, &config).expect("the scene builds");
    let bones = scene.bones().len();
    assert_eq!(bones, BONES);
    assert_eq!(bones, scene.dog.len());
    assert_eq!(scene.objects.len(), bones + 1, "terrain + one dog's bones");
    // The whole point, stated as the inequality it is: the mesh set does NOT
    // grow with the crowd.
    assert!(scene.dogs.len() > 1);
    assert!(
        scene.objects.len() < bones * scene.dogs.len(),
        "the scene registered {} meshes for {} dogs — the geometry is being copied, not shared",
        scene.objects.len(),
        scene.dogs.len()
    );
    // Nor is any bone secretly a duplicate of another that was registered twice:
    // 23 bones, 23 distinct names, and a rig that claims exactly those.
    let mut names: Vec<&str> = scene.bones().iter().map(|object| object.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), bones);

    // ...and the frame the engine actually produces batches the way the palette
    // promises: one batch per (bone mesh, worn coat) pair plus the terrain — at
    // most 23 × 18 + 1 = 415, NOT 23 × 104 = 2392. The retired pool slots are
    // invisible, so they cost neither a draw nor a batch.
    let instances = 1 + bones * scene.dogs.len();
    let mut worn: Vec<usize> = scene.dogs.iter().map(|dog| dog.palette).collect();
    worn.sort_unstable();
    worn.dedup();
    let mut app = headless_app(SceneVariant::Base, DebugView::Shaded, &config);
    let outcome = app.tick(0);
    assert_eq!(outcome.draws().len(), instances);
    let batches = outcome.mesh_batches().len();
    assert_eq!(batches, bones * worn.len() + 1, "one batch per (mesh, coat)");
    assert!(
        batches <= bones * PALETTE_SIZE + 1,
        "{batches} batches for a {PALETTE_SIZE}-entry palette"
    );
    assert!(
        batches < instances / 4,
        "{batches} batches for {instances} instances — the crowd is not being instanced"
    );
    println!(
        "[shared] {} distinct meshes × {} worn coats (of {PALETTE_SIZE}) drive {} dogs = {instances} instances in {batches} batches",
        scene.objects.len(),
        worn.len(),
        scene.dogs.len()
    );
}
