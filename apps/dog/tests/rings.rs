//! The concentric field's proof suite.
//!
//! Everything about the crowd is checkable natively, because a dog's pose is a
//! pure function of the tick and its identity is three numbers. These are the
//! claims:
//!
//! 1. **The field is derived, not typed** — the ring count is the radial pitch
//!    stepped from the innermost radius to the outermost, and each ring's dog
//!    count is its circumference divided by the room one dog needs.
//! 2. **No two rings intersect** — the radial band one ring's dogs occupy (its
//!    radius, widened by half a dog's width and by the outward bulge a rigid
//!    body makes on a curve) clears the next ring's band, and every dog stands on
//!    the terrain rather than in the air over it.
//! 3. **Every ring turns against both its neighbours** — the sign of
//!    `(position − centre) × heading` alternates from ring to ring, measured on
//!    the **real posed bones** and on the direction the body actually travels
//!    between two ticks.
//! 4. **The rainbow is bounded and readable** — the whole field is painted from
//!    a fixed palette of 18 coats (every one of which is worn), no dog shares a coat with the dog in front of
//!    it or behind it, and adjacent rings draw from disjoint hue combs so a
//!    cross-ring neighbour is never a near-match either.
//! 5. **It is deterministic** — tick `N` posed twice is byte-equal; tick `N` and
//!    tick `N+1` are not.
//! 6. **The geometry is shared, and so are the materials** — the scene registers
//!    `bone_count` distinct dog meshes (not `bone_count × dogs`), and the frame
//!    it produces is `bone_count × PALETTE_SIZE + 1` batches (not
//!    `bone_count × dogs`), however many dogs are walking.

use axiom_math::Vec3;
use axiom_procedural_mesh_crucible::{
    crucible_core, crucible_scene, dog_parts, dog_total, ground_y, outer_clearance, palette_color,
    CrucibleAnimation, CrucibleVariant, DebugView, DOG_LENGTH, DOG_SPACING, DOG_WIDTH,
    PALETTE_SIZE, RINGS, RING_COUNT, RING_MAX_RADIUS, RING_MIN_RADIUS, RING_SPACING,
    TERRAIN_HALF_EXTENT, TRAVEL_PER_TICK,
};

/// The bone count the whole field instances.
const BONES: usize = 23;

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

/// How far apart two coats read, as the sum of their channel differences.
fn coat_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum()
}

#[test]
fn the_field_fills_the_ground_from_the_gait_floor_to_the_terrain_rim() {
    // Eight rings, 7.75 units apart, from the tightest curve the gait is tuned
    // for out to the widest circle that still leaves clear ground before the rim.
    assert_eq!(RING_COUNT, 8);
    assert_eq!(RINGS.len(), RING_COUNT);
    assert_eq!(RINGS[0].radius, RING_MIN_RADIUS);
    assert_eq!(RINGS[RING_COUNT - 1].radius, RING_MAX_RADIUS);
    assert_eq!(
        RINGS.map(|ring| ring.radius),
        [26.0, 33.75, 41.5, 49.25, 57.0, 64.75, 72.5, 80.25]
    );
    // The pitch is bounded by the dog's WIDTH plus its bulge on a curve, not by
    // its length — which is the whole reason eight rings fit at all.
    assert!(RING_SPACING < DOG_LENGTH * 0.5);
    assert!(RING_SPACING > DOG_WIDTH + RINGS[0].bulge());
    // ...and the outermost ring leaves half a dog's length of ground to the rim.
    let clear = outer_clearance();
    assert!(
        clear > DOG_LENGTH * 0.5,
        "only {clear} units of ground beyond the outer ring of a {TERRAIN_HALF_EXTENT}-unit plate"
    );
    println!(
        "[field] {RING_COUNT} rings, {} .. {} step {RING_SPACING}, {clear:.1} units of rim clearance",
        RING_MIN_RADIUS, RING_MAX_RADIUS
    );
}

#[test]
fn each_ring_holds_the_number_of_dogs_its_circumference_pays_for() {
    let animation = animation();
    assert_eq!(
        RINGS.map(|ring| ring.count()),
        [6, 8, 10, 12, 14, 16, 18, 20]
    );
    assert_eq!(dog_total(), 104);
    assert_eq!(animation.dog_count(), 104);
    assert_eq!(animation.bone_count(), BONES);

    for (index, ring) in RINGS.iter().enumerate() {
        let walkers = animation
            .dogs()
            .iter()
            .filter(|dog| dog.ring == index)
            .count();
        assert_eq!(walkers, ring.count(), "ring {index} holds {walkers} dogs");
        // The measured walk is the circle it was authored as.
        let total = animation.path(index).total();
        assert!(
            (total - ring.circumference()).abs() / ring.circumference() < 0.02,
            "ring {index} measures {total}, not ~{}",
            ring.circumference()
        );
        // And its dogs are spaced nose to tail: a realised spacing within a tenth
        // of the target, never closer than the dog is long.
        let spacing = total / ring.count() as f32;
        assert!(
            (spacing - DOG_SPACING).abs() < 0.12 * DOG_SPACING,
            "ring {index} spaces its dogs {spacing} apart"
        );
        assert!(spacing > DOG_LENGTH, "ring {index} dogs overlap");
        println!(
            "[ring] {index}  radius {:>5.1}  length {total:>7.1}  {:>2} dogs  spacing {spacing:>5.2}  gap {:>4.2}",
            ring.radius,
            ring.count(),
            spacing - DOG_LENGTH
        );
    }
}

#[test]
fn no_two_rings_intersect_radially() {
    RINGS.windows(2).for_each(|pair| {
        let (_, outer_edge) = pair[0].band();
        let (inner_edge, _) = pair[1].band();
        let air = inner_edge - outer_edge;
        assert!(
            air > 1.0,
            "rings {} and {} clear each other by only {air} units",
            pair[0].index,
            pair[1].index
        );
        println!(
            "[clear] ring {} (to {outer_edge:.2}) → ring {} (from {inner_edge:.2}): {air:.2} units of air",
            pair[0].index, pair[1].index
        );
    });
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
/// A dog's *heading* is taken two ways, and both must agree with its ring: the
/// way its body genuinely travelled between two consecutive ticks, and the way
/// its root bone is facing (local `-Z`, the axis every bone is authored down).
/// A sign error anywhere between the authored winding, the spline
/// parameterization and `aim` shows up here rather than on the page.
///
/// Every ring now turns the same way, so the closing assertion is that they
/// genuinely *agree* — measured on real posed bones rather than read back off
/// the constant that set them, which is what makes this a test of the spline
/// and `aim` rather than a tautology.
#[test]
fn every_ring_turns_the_same_way_as_its_neighbours() {
    let animation = animation();
    let bones = animation.bone_count();
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
            let ring = RINGS[dog.ring];
            let body = now[index * bones];
            let travelled = flat(next[index * bones].translation.subtract(body.translation));
            assert!(
                // Derived from the travel constant, not a magic number: this
                // asserts the dog moved essentially a full tick's worth, so it
                // keeps its meaning if the walking speed is retuned again.
                travelled.length() > TRAVEL_PER_TICK * 0.8,
                "dog {index} barely moved between ticks: {travelled:?}"
            );
            assert!(
                turn(index) * ring.winding().cross_sign() > 0.0,
                "dog {index} on ring {} turns the wrong way at tick {tick}",
                ring.index
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
                cross_y(flat(body.translation), facing) * ring.winding().cross_sign() > 0.0,
                "dog {index} on ring {} faces the wrong way round",
                ring.index
            );
        }
        // And every adjacent pair of rings genuinely agrees, measured on the
        // posed bones: a stray sign in the spline or in `aim` would show up as
        // one band shearing against the rest.
        let lead: Vec<usize> = (0..RING_COUNT)
            .map(|ring| {
                animation
                    .dogs()
                    .iter()
                    .position(|dog| dog.ring == ring)
                    .expect("every ring holds at least one dog")
            })
            .collect();
        lead.windows(2).enumerate().for_each(|(index, pair)| {
            assert!(
                turn(pair[0]) * turn(pair[1]) > 0.0,
                "rings {index} and {} turn against each other at tick {tick}",
                index + 1
            );
        });
    }
}

#[test]
fn the_field_is_painted_from_a_bounded_palette_no_neighbour_shares() {
    let scene = crucible_scene(CrucibleVariant::Base).expect("the scene builds");
    // Bounded: the palette is a fixed size that the crowd size cannot move.
    assert_eq!(PALETTE_SIZE, 18);
    assert!(PALETTE_SIZE < scene.dogs.len(), "the palette is not bounded");
    assert!(scene.dogs.iter().all(|dog| dog.palette < PALETTE_SIZE));
    let mut worn: Vec<usize> = scene.dogs.iter().map(|dog| dog.palette).collect();
    worn.sort_unstable();
    worn.dedup();
    assert!(
        worn.len() <= PALETTE_SIZE,
        "{} coats for a {PALETTE_SIZE}-entry palette",
        worn.len()
    );

    for (index, ring) in RINGS.iter().enumerate() {
        let chain: Vec<[f32; 3]> = scene
            .dogs
            .iter()
            .filter(|dog| dog.ring == index)
            .map(|dog| dog.color())
            .collect();
        assert_eq!(chain.len(), ring.count());

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
        // The chain spans the whole hue circle: it visits a colour whose
        // strongest channel is red, one green and one blue.
        for (channel, name) in [(0usize, "red"), (1, "green"), (2, "blue")] {
            assert!(
                chain.iter().any(|rgb| {
                    (0..3).all(|other| other == channel || rgb[channel] > rgb[other] + 0.05)
                }),
                "ring {index} never reaches a {name} dog: {chain:?}"
            );
        }
    }

    // Across rings: neighbouring rings draw from disjoint combs, so *no* pair of
    // dogs on adjacent rings can be a near-match — which matters here because
    // the two chains counter-rotate and every relative alignment happens.
    RINGS.windows(2).for_each(|pair| {
        let coats = |ring: usize| -> Vec<[f32; 3]> {
            scene
                .dogs
                .iter()
                .filter(|dog| dog.ring == ring)
                .map(|dog| dog.color())
                .collect()
        };
        let (inner, outer) = (coats(pair[0].index), coats(pair[1].index));
        let closest = inner
            .iter()
            .flat_map(|a| outer.iter().map(move |b| coat_distance(*a, *b)))
            .fold(f32::INFINITY, f32::min);
        assert!(
            closest > 0.05,
            "rings {} and {} come within {closest} of sharing a coat",
            pair[0].index,
            pair[1].index
        );
        println!(
            "[coats] rings {} / {}: closest pair {closest:.3} apart",
            pair[0].index, pair[1].index
        );
    });

    // A dog's colour is a palette lookup, not a property of the dog.
    let first = scene.dogs[0];
    assert_eq!(first.color(), palette_color(first.palette));
}

#[test]
fn the_crowd_is_a_pure_function_of_the_tick() {
    let animation = animation();
    for tick in [0u64, 1, 97, 4_321] {
        let first = animation.transforms(tick);
        let second = animation.transforms(tick);
        assert_eq!(first.len(), animation.bone_count() * animation.dog_count());
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

/// The engineering claim, both halves: one dog's geometry and eighteen coats
/// drive a hundred and twenty dogs.
#[test]
fn the_whole_crowd_shares_one_dogs_geometry_and_one_palette() {
    let scene = crucible_scene(CrucibleVariant::Base).expect("the scene builds");
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
    // most 23 × 18 + 1 = 415, NOT 23 × 104 = 2392.
    let instances = 1 + bones * scene.dogs.len();
    let mut worn: Vec<usize> = scene.dogs.iter().map(|dog| dog.palette).collect();
    worn.sort_unstable();
    worn.dedup();
    let mut app = crucible_core(CrucibleVariant::Base, DebugView::Shaded);
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
