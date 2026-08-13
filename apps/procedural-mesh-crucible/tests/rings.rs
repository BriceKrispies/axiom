//! The two counter-rotating rings' proof suite.
//!
//! Everything about the crowd is checkable natively, because a dog's pose is a
//! pure function of the tick and its identity is three numbers. These are the
//! claims:
//!
//! 1. **The rings are populated from their own geometry** — the counts are the
//!    circumference divided by the room one dog needs, and every dog stands on
//!    the terrain rather than in the air over it.
//! 2. **They turn opposite ways** — the sign of `(position − centre) × heading`
//!    is positive for every dog on the outer ring and negative for every dog on
//!    the inner one, measured on the **real posed bones** and on the direction
//!    the body actually travels between two ticks.
//! 3. **The rainbow spans the circle** — every dog's colour is distinct from its
//!    neighbours', both rings cover the full hue wheel, and the two rings are
//!    out of phase with each other.
//! 4. **It is deterministic** — tick `N` posed twice is byte-equal; tick `N` and
//!    tick `N+1` are not.
//! 5. **The geometry is shared** — the scene registers `bone_count` distinct dog
//!    meshes, not `bone_count × dogs`, however many dogs are walking.

use axiom_math::Vec3;
use axiom_procedural_mesh_crucible::{
    crucible_scene, dog_parts, ground_y, hue_to_rgb, CrucibleAnimation, CrucibleVariant, INNER,
    OUTER, RINGS,
};

/// The terrain's half-extent — nothing may walk off it.
const TERRAIN_HALF_EXTENT: f32 = 96.0;

fn animation() -> CrucibleAnimation {
    CrucibleAnimation::new(dog_parts(CrucibleVariant::Base).expect("the dog rigs"))
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

#[test]
fn each_ring_holds_the_number_of_dogs_its_circumference_pays_for() {
    let animation = animation();
    assert_eq!(OUTER.count(), 12);
    assert_eq!(INNER.count(), 7);
    assert_eq!(animation.dog_count(), 19);
    assert_eq!(animation.bone_count(), 23);

    for (index, ring) in RINGS.iter().enumerate() {
        let walkers = animation
            .dogs()
            .iter()
            .filter(|dog| dog.ring == index)
            .count();
        assert_eq!(walkers, ring.count(), "{} holds {walkers} dogs", ring.name);
        // The measured walk is the circle it was authored as.
        let total = animation.path(index).total();
        assert!(
            (total - ring.circumference()).abs() / ring.circumference() < 0.02,
            "{} measures {total}, not ~{}",
            ring.name,
            ring.circumference()
        );
        println!(
            "[ring] {:<6} radius {:>5.1}  length {:>7.1}  {:>2} dogs  spacing {:>5.2}",
            ring.name,
            ring.radius,
            total,
            ring.count(),
            total / ring.count() as f32
        );
    }
}

#[test]
fn every_dog_stands_on_the_terrain_on_its_own_ring() {
    let animation = animation();
    let rig = dog_parts(CrucibleVariant::Base).expect("the dog rigs");
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
            let ring = RINGS[dog.ring];
            let body = all[index * bones].translation;
            let radius = flat(body).length();
            assert!(
                (radius - ring.radius).abs() < 12.0,
                "dog {index} left the {} ring at tick {tick}: radius {radius}",
                ring.name
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

/// The direction test that actually distinguishes the two rings.
///
/// A dog's *heading* is taken two ways, and both must agree with the ring: the
/// way its body genuinely travelled between two consecutive ticks, and the way
/// its root bone is facing (local `-Z`, the axis every bone is authored down).
/// A sign error anywhere between the authored winding, the spline
/// parameterization and `aim` shows up here rather than on the page.
#[test]
fn the_outer_ring_walks_anticlockwise_and_the_inner_ring_clockwise() {
    let animation = animation();
    let bones = animation.bone_count();
    for tick in [0u64, 61, 349, 1_004] {
        let now = animation.transforms(tick);
        let next = animation.transforms(tick + 1);
        for (index, dog) in animation.dogs().iter().enumerate() {
            let ring = RINGS[dog.ring];
            let body = now[index * bones];
            let radius = flat(body.translation);
            let travelled = flat(next[index * bones].translation.subtract(body.translation));
            assert!(
                travelled.length() > 0.2,
                "dog {index} barely moved between ticks: {travelled:?}"
            );
            let turning = cross_y(radius, travelled);
            assert!(
                turning * ring.winding.cross_sign() > 0.0,
                "dog {index} on the {} ring turns the wrong way at tick {tick}: cross {turning}",
                ring.name
            );
            // ...and it faces the way it is going, rather than walking backwards
            // round the right circle.
            let facing = flat(body.rotation.rotate(Vec3::new(0.0, 0.0, -1.0)));
            let along = facing.dot(travelled) / (facing.length() * travelled.length());
            assert!(
                along > 0.9,
                "dog {index} faces {along} of the way it travels at tick {tick}"
            );
            assert!(
                cross_y(radius, facing) * ring.winding.cross_sign() > 0.0,
                "dog {index} on the {} ring faces the wrong way round",
                ring.name
            );
        }
        // And the two rings genuinely disagree: the signs are opposite, not
        // merely "each consistent with itself".
        let outer = animation.dogs().iter().position(|d| d.ring == 0).unwrap();
        let inner = animation.dogs().iter().position(|d| d.ring == 1).unwrap();
        let turn = |index: usize| {
            cross_y(
                flat(now[index * bones].translation),
                flat(next[index * bones].translation.subtract(now[index * bones].translation)),
            )
        };
        assert!(
            turn(outer) * turn(inner) < 0.0,
            "both rings turn the same way at tick {tick}"
        );
    }
}

#[test]
fn the_two_rings_are_rainbows_and_no_two_neighbours_share_a_colour() {
    let scene = crucible_scene(CrucibleVariant::Base).expect("the scene builds");
    for (index, ring) in RINGS.iter().enumerate() {
        let chain: Vec<[f32; 3]> = scene
            .dogs
            .iter()
            .filter(|dog| dog.ring == index)
            .map(|dog| dog.color)
            .collect();
        assert_eq!(chain.len(), ring.count());

        // Neighbours differ — including the pair that closes the ring.
        for slot in 0..chain.len() {
            let here = chain[slot];
            let next = chain[(slot + 1) % chain.len()];
            let apart: f32 = here
                .iter()
                .zip(next.iter())
                .map(|(a, b)| (a - b).abs())
                .sum();
            assert!(
                apart > 0.05,
                "{} dogs {slot} and {} share a colour: {here:?}",
                ring.name,
                (slot + 1) % chain.len()
            );
        }
        // The chain spans the whole hue circle: it visits a colour whose
        // strongest channel is red, one green and one blue.
        for (channel, name) in [(0usize, "red"), (1, "green"), (2, "blue")] {
            assert!(
                chain.iter().any(|rgb| {
                    (0..3).all(|other| other == channel || rgb[channel] > rgb[other] + 0.05)
                }),
                "the {} ring never reaches a {name} dog: {chain:?}",
                ring.name
            );
        }
        println!(
            "[rainbow] {:<6}: {} colours, first {:?}",
            ring.name, chain.len(), chain[0]
        );
    }
    // The rings start half a turn apart, so the pair does not read as one
    // palette drawn twice.
    assert_ne!(OUTER.hue_phase, INNER.hue_phase);
    let outer_first = scene.dogs.iter().find(|d| d.ring == 0).unwrap().color;
    let inner_first = scene.dogs.iter().find(|d| d.ring == 1).unwrap().color;
    assert_eq!(outer_first, hue_to_rgb(OUTER.hue_phase));
    assert_ne!(outer_first, inner_first);
}

#[test]
fn the_crowd_is_a_pure_function_of_the_tick() {
    let animation = animation();
    for tick in [0u64, 1, 97, 4_321] {
        let first = animation.transforms(tick);
        let second = animation.transforms(tick);
        assert_eq!(
            first.len(),
            animation.bone_count() * animation.dog_count()
        );
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(
                a.translation, b.translation,
                "tick {tick} is not reproducible"
            );
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
}

/// The engineering claim: one dog's geometry, nineteen dogs on screen.
#[test]
fn the_whole_crowd_shares_one_dogs_geometry() {
    let scene = crucible_scene(CrucibleVariant::Base).expect("the scene builds");
    let bones = scene.bones().len();
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
    println!(
        "[shared] {} distinct meshes drive {} dogs = {} instances",
        scene.objects.len(),
        scene.dogs.len(),
        1 + bones * scene.dogs.len()
    );
}
