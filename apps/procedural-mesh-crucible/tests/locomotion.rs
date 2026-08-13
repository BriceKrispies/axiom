//! The locomotion proof suite: the loop, the rig, the gait and the inverse
//! kinematics, all held natively and without a browser.
//!
//! Every claim the animation makes is checkable off-GPU, because the pose is a
//! pure function of the tick. These are the claims:
//!
//! 1. **The solver is exact and total** — it reaches a reachable target with the
//!    bones at their authored lengths, clamps an unreachable one without ever
//!    stretching a bone or producing a NaN, and bends the joint toward the pole
//!    it was given rather than wherever the arithmetic fell.
//! 2. **The rig resolves** — parents precede children, and one forward pass
//!    lands every bone at a finite world transform.
//! 3. **The animation is deterministic** — tick `N` posed twice is byte-equal,
//!    and tick `N` differs from tick `N+1`.
//! 4. **The feet do not skate** — a planted paw's world position is *constant*
//!    for the whole of its stance while the body travels over it.
//! 5. **The pair runs the same loop** — the human holds a fixed arc-length lag
//!    behind the dog, on the dog's own track, and both stay on the loop and on
//!    the terrain.

use axiom_kernel::Ratio;
use axiom_math::{Transform, Vec3};
use axiom_procedural_mesh_crucible::{
    dog_limbs, dog_parts, dog_travel, human_limbs, human_parts, human_travel, road_curve,
    solve_two_bone, stride_phase, CrucibleAnimation, CrucibleVariant, LoopPath, DOG_GAIT,
    HUMAN_GAIT, HUMAN_LAG, LOOP_RADIUS, TRAVEL_PER_TICK,
};

/// The terrain's half-extent — nothing may run off it.
const TERRAIN_HALF_EXTENT: f32 = 96.0;

fn animation() -> CrucibleAnimation {
    let dog = dog_parts(CrucibleVariant::Base).expect("the dog rigs");
    let human = human_parts(CrucibleVariant::Base).expect("the human rigs");
    CrucibleAnimation::new(dog, human).expect("the loop builds")
}

fn finite(t: &Transform) -> bool {
    [t.translation, t.scale].iter().all(|v| {
        v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
    }) && [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w]
        .iter()
        .all(|c| c.is_finite())
}

#[test]
fn the_solver_reaches_clamps_and_bends_where_it_is_told() {
    let root = Vec3::new(0.0, 6.0, 0.0);
    let (upper, lower) = (2.6, 2.9);

    // Reachable: the end lands on the target and both bones are exact.
    let target = Vec3::new(0.4, 1.6, -0.9);
    let reached = solve_two_bone(root, target, Vec3::new(0.0, 0.0, -1.0), upper, lower);
    assert!(!reached.clamped);
    assert!(
        reached.end.distance(target) < 1.0e-3,
        "the solver missed by {}",
        reached.end.distance(target)
    );
    assert!((reached.joint.distance(root) - upper).abs() < 1.0e-3);
    assert!((reached.end.distance(reached.joint) - lower).abs() < 1.0e-3);

    // Unreachable: still exact bones, still finite, and the end is on the ray.
    let far = Vec3::new(0.0, -40.0, 0.0);
    let clamped = solve_two_bone(root, far, Vec3::new(0.0, 0.0, -1.0), upper, lower);
    assert!(clamped.clamped);
    assert!((clamped.joint.distance(root) - upper).abs() < 1.0e-3);
    assert!((clamped.end.distance(clamped.joint) - lower).abs() < 1.0e-3);
    for v in [clamped.joint, clamped.end] {
        assert!(
            v.x.is_finite() && v.y.is_finite() && v.z.is_finite(),
            "the clamped solve produced {v:?}"
        );
    }
    assert!((clamped.end.distance(root) - (upper + lower)).abs() < 1.0e-2);

    // The pole, and only the pole, chooses which side the joint bulges to.
    let straight_down = Vec3::new(0.0, 1.2, 0.0);
    let knee = solve_two_bone(root, straight_down, Vec3::new(0.0, 0.0, -1.0), upper, lower);
    let elbow = solve_two_bone(root, straight_down, Vec3::new(0.0, 0.0, 1.0), upper, lower);
    assert!(knee.joint.z < -0.4, "the knee did not lead forward: {:?}", knee.joint);
    assert!(elbow.joint.z > 0.4, "the elbow did not lead back: {:?}", elbow.joint);
}

#[test]
fn every_rig_resolves_in_one_forward_pass() {
    for variant in CrucibleVariant::ALL {
        for (name, rig) in [
            ("dog", dog_parts(variant).expect("the dog rigs")),
            ("human", human_parts(variant).expect("the human rigs")),
        ] {
            // Parents precede children — the invariant the single pass rests on.
            for (index, part) in rig.parts().iter().enumerate() {
                if let Some(parent) = part.parent {
                    assert!(
                        parent < index,
                        "{name}/{} is declared before its parent",
                        part.name
                    );
                }
            }
            // Exactly one root.
            let roots = rig.parts().iter().filter(|p| p.parent.is_none()).count();
            assert_eq!(roots, 1, "{name} has {roots} root bones");
            // Names are unique — the scene addresses bones by them.
            let mut names: Vec<&str> = rig.parts().iter().map(|p| p.name).collect();
            let count = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), count, "{name} has duplicate bone names");
            // And the pass lands every bone somewhere finite.
            let world = rig.rest_world(Transform::IDENTITY);
            assert_eq!(world.len(), rig.len());
            assert!(
                world.iter().all(finite),
                "{name} resolved a bone to a non-finite transform"
            );
        }
    }
}

#[test]
fn the_pose_is_a_pure_function_of_the_tick() {
    let animation = animation();
    for tick in [0u64, 1, 97, 4_321] {
        let first = animation.transforms(tick);
        let second = animation.transforms(tick);
        assert_eq!(
            first.len(),
            animation.dog_bone_count() + animation.human_bone_count()
        );
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.translation, b.translation, "tick {tick} is not reproducible");
            assert_eq!(a.rotation, b.rotation, "tick {tick} is not reproducible");
            assert_eq!(a.scale, b.scale, "tick {tick} is not reproducible");
        }
        // And the next tick is a different pose — the animation actually moves.
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
    // Every transform is finite for a long run, including past a full lap.
    for tick in (0u64..3_000).step_by(37) {
        assert!(
            animation.transforms(tick).iter().all(finite),
            "tick {tick} produced a non-finite transform"
        );
    }
}

#[test]
fn a_planted_paw_does_not_skate_while_the_body_travels_over_it() {
    let animation = animation();
    let rig = dog_parts(CrucibleVariant::Base).expect("the dog rigs");
    // The front-left leg: phase offset 0, contact 0.310 behind the origin.
    let limb = dog_limbs()[0];
    let paw = rig.index_of(limb.tip).expect("the dog has a front-left paw");
    let fore = -limb.contact.z * DOG_GAIT.scale;

    // Walk a couple of strides and collect, per stance, how far the paw moved.
    let mut stances: Vec<(f32, usize)> = Vec::new();
    let mut current: Option<(f32, Vec3, f32, usize)> = None;
    for tick in 0u64..90 {
        let phase = stride_phase(
            dog_travel(tick) + fore,
            DOG_GAIT.stride,
            limb.offset,
            DOG_GAIT.duty,
        );
        let here = animation.transforms(tick)[paw].translation;
        current = match (phase.planted, current) {
            (false, done) => {
                done.map(|(_, _, drift, samples)| stances.push((drift, samples)));
                None
            }
            (true, Some((step, first, drift, samples))) if step == phase.step => Some((
                step,
                first,
                drift.max(first.distance(here)),
                samples + 1,
            )),
            (true, done) => {
                done.map(|(_, _, drift, samples)| stances.push((drift, samples)));
                Some((phase.step, here, 0.0, 1))
            }
        };
    }

    assert!(
        stances.len() >= 3,
        "only {} stances observed in 90 ticks — the gait is not cycling",
        stances.len()
    );
    for (drift, samples) in &stances {
        println!("[stance] {samples} ticks planted, paw drifted {drift:.5}");
        assert!(
            *samples >= 3,
            "a stance lasted only {samples} ticks — too short to prove anything"
        );
        // The paw is placed at the plant point, which is a function of the step
        // number alone. Anything but ~0 here means the leg is being stretched
        // (a clamped solve) or the plant is being recomputed per frame.
        assert!(
            *drift < 0.02,
            "a planted paw skated {drift} units across {samples} ticks"
        );
    }
    // Meanwhile the body genuinely moved: this is not a frozen scene.
    let travelled = dog_travel(90) - dog_travel(0);
    assert!(travelled > 2.0 * DOG_GAIT.stride, "the dog barely moved");
}

#[test]
fn the_human_holds_a_fixed_arc_length_lag_on_the_dogs_own_track() {
    let path = LoopPath::perimeter().expect("the loop builds");
    // The lag is fixed, exactly, at every tick.
    for tick in [0u64, 1, 500, 9_999] {
        let gap = dog_travel(tick) - human_travel(tick);
        assert!(
            (gap - HUMAN_LAG).abs() < 1.0e-3,
            "the gap at tick {tick} is {gap}, not {HUMAN_LAG}"
        );
    }
    // And it is a lag along the *path*: where the human is now is where the dog
    // was exactly `HUMAN_LAG / TRAVEL_PER_TICK` ticks ago. The two constants are
    // chosen so that quotient is a whole number of ticks.
    let behind = HUMAN_LAG / TRAVEL_PER_TICK;
    assert!(
        (behind - behind.round()).abs() < 1.0e-4,
        "the lag is {behind} ticks — not a whole tick, so this check cannot be exact"
    );
    for tick in [200u64, 733, 1_500] {
        let human_now = path.at(human_travel(tick)).position;
        let dog_then = path.at(dog_travel(tick - behind.round() as u64)).position;
        assert!(
            human_now.distance(dog_then) < 1.0e-2,
            "the human is not on the dog's track: {human_now:?} vs {dog_then:?}"
        );
    }
}

/// "The human follows the dog" is a claim about *direction*, not just about a
/// signed arc-length constant: the human must be behind the dog **along the way
/// they are both facing**. This projects the gap onto the human's own forward
/// vector, so a sign error anywhere between the travel constant, the path
/// parameterization and `aim` shows up here rather than on the page.
#[test]
fn the_human_runs_behind_the_dog_in_the_direction_they_both_face() {
    let path = LoopPath::perimeter().expect("the loop builds");
    for tick in (0u64..1_200).step_by(23) {
        let dog = path.at(dog_travel(tick));
        let human = path.at(human_travel(tick));
        let gap = Vec3::new(
            dog.position.x - human.position.x,
            0.0,
            dog.position.z - human.position.z,
        );
        let ahead = gap.dot(human.forward);
        assert!(
            ahead > 0.5 * HUMAN_LAG,
            "at tick {tick} the dog is only {ahead} ahead of the human along its heading"
        );
        // They face the same way, give or take the loop's own curvature over
        // the lag (34 units of a 540-unit loop is ~23 degrees).
        let alignment = dog.forward.dot(human.forward);
        assert!(
            alignment > 0.85,
            "at tick {tick} the pair are not running the same way: {alignment}"
        );
    }
}

#[test]
fn both_creatures_stay_on_the_loop_and_on_the_terrain() {
    let animation = animation();
    let dog_bones = animation.dog_bone_count();
    for tick in (0u64..1_800).step_by(11) {
        let all = animation.transforms(tick);
        for (index, bone) in all.iter().enumerate() {
            let who = ["dog", "human"][usize::from(index >= dog_bones)];
            let radius = (bone.translation.x * bone.translation.x
                + bone.translation.z * bone.translation.z)
                .sqrt();
            // A creature is ~11-18 units tall at presentation scale, so its
            // bones sit within a body's reach of the loop it runs.
            assert!(
                (radius - LOOP_RADIUS).abs() < 14.0,
                "the {who}'s bone {index} left the loop at tick {tick}: radius {radius}"
            );
            assert!(
                bone.translation.x.abs() < TERRAIN_HALF_EXTENT
                    && bone.translation.z.abs() < TERRAIN_HALF_EXTENT,
                "the {who} ran off the terrain at tick {tick}: {:?}",
                bone.translation
            );
            // The terrain is a shallow basin; nothing should be near the sky or
            // buried under the skirt.
            assert!(
                bone.translation.y > -20.0 && bone.translation.y < 30.0,
                "the {who}'s bone {index} is at y = {} at tick {tick}",
                bone.translation.y
            );
        }
    }
}

/// The tuning check that keeps the gait honest.
///
/// The two-bone solver *cannot* stretch a bone: handed a target beyond reach it
/// clamps, and the foot then comes off its plant. So "no leg is ever asked to
/// reach further than it is long" is exactly equivalent to "no foot ever
/// skates", and it is far easier to measure — one distance per limb per tick,
/// over more than two full laps of real terrain.
///
/// A tuning edit that lengthens a stride, shallows a crouch or widens the
/// terrain relief cap past what the legs can absorb fails HERE, with the
/// offending limb and the number, instead of shipping a sliding dog.
#[test]
fn no_limb_is_ever_asked_to_reach_further_than_it_is_long() {
    let dog = dog_parts(CrucibleVariant::Base).expect("the dog rigs");
    let human = human_parts(CrucibleVariant::Base).expect("the human rigs");
    let animation =
        CrucibleAnimation::new(dog.clone(), human.clone()).expect("the loop builds");
    let dog_bones = animation.dog_bone_count();

    for (label, rig, limbs, gait, base) in [
        ("dog", &dog, dog_limbs(), DOG_GAIT, 0usize),
        ("human", &human, human_limbs(), HUMAN_GAIT, dog_bones),
    ] {
        for limb in limbs.iter() {
            let hip = rig.index_of(limb.upper).expect("the limb's upper bone");
            let tip = rig.index_of(limb.tip).expect("the limb's terminating block");
            let reach = (limb.len_upper + limb.len_lower + limb.len_extra) * gait.scale;
            let worst = (0u64..2_000)
                .map(|tick| {
                    let all = animation.transforms(tick);
                    all[base + hip]
                        .translation
                        .distance(all[base + tip].translation)
                })
                .fold(0.0f32, f32::max);
            println!(
                "[reach] {label:<6} {:<22} reach {reach:6.3}  worst {worst:6.3}  ({:.0}%)",
                limb.upper,
                100.0 * worst / reach
            );
            assert!(
                worst < reach * 0.97,
                "{label}'s {} is stretched to {worst} of its {reach} reach — the gait                  is over-striding and the foot will come off its plant",
                limb.upper
            );
        }
    }
}

#[test]
fn the_loop_is_closed_uniform_and_clear_of_the_scene() {
    let path = LoopPath::perimeter().expect("the loop builds");
    let total = path.total();
    // A circle of this radius, so the sampled length is within a percent of it.
    let circumference = core::f32::consts::TAU * LOOP_RADIUS;
    assert!(
        (total - circumference).abs() / circumference < 0.02,
        "the loop measures {total}, not ~{circumference}"
    );
    println!("[loop] radius {LOOP_RADIUS}, length {total:.1}, lap {:.1} s", total / (TRAVEL_PER_TICK * 60.0));

    // Closed: one full lap returns to the start, in position and in heading.
    let start = path.at(0.0);
    let lap = path.at(total);
    assert!(start.position.distance(lap.position) < 1.0e-2, "the loop does not close");
    assert!(start.forward.distance(lap.forward) < 1.0e-2, "the loop kinks at its seam");

    // Uniform: equal arc-length steps really are equal distances on the ground.
    let step = total / 256.0;
    let spans: Vec<f32> = (0..256)
        .map(|i| {
            let a = path.at(i as f32 * step).position;
            let b = path.at((i + 1) as f32 * step).position;
            Vec3::new(a.x, 0.0, a.z).distance(Vec3::new(b.x, 0.0, b.z))
        })
        .collect();
    let shortest = spans.iter().copied().fold(f32::INFINITY, f32::min);
    let longest = spans.iter().copied().fold(0.0f32, f32::max);
    assert!(
        longest / shortest < 1.05,
        "arc-length sampling is not uniform: spans run {shortest} to {longest}"
    );

    // Clear of everything standing in the scene, and inside the terrain.
    const OBSTACLES: [(&str, f32, f32, f32); 4] = [
        ("building", -44.0, -16.0, 12.0),
        ("sculpture", 44.0, 8.0, 10.0),
        ("primitive row", 0.0, -74.0, 4.0),
        ("lod ladder", 0.0, -60.0, 4.0),
    ];
    for i in 0..512 {
        let point = path.at(i as f32 * total / 512.0).position;
        assert!(
            point.x.abs() < TERRAIN_HALF_EXTENT && point.z.abs() < TERRAIN_HALF_EXTENT,
            "the loop leaves the terrain at {point:?}"
        );
        for (name, x, z, clearance) in OBSTACLES {
            let gap = Vec3::new(point.x, 0.0, point.z).distance(Vec3::new(x, 0.0, z));
            assert!(gap > clearance, "the loop passes {gap} from the {name}");
        }
    }

    // Clear of the road itself, measured against the real curve rather than a
    // bounding box: the road bends, and a box around it would either pass a
    // loop that clips its outside of a bend or fail one that does not.
    let road = road_curve().expect("the road curve builds");
    let stations: Vec<Vec3> = (0..=400)
        .map(|i| road.position_at(Ratio::finite_or_zero(i as f32 / 400.0)))
        .collect();
    let mut nearest = f32::INFINITY;
    for i in 0..512 {
        let point = path.at(i as f32 * total / 512.0).position;
        for station in &stations {
            nearest = nearest.min(
                Vec3::new(point.x, 0.0, point.z).distance(Vec3::new(station.x, 0.0, station.z)),
            );
        }
    }
    println!("[loop] nearest approach to the road: {nearest:.1}");
    assert!(nearest > 12.0, "the loop passes {nearest} from the road");
    assert!(nearest < 40.0, "the loop is so far out the road is no longer the near thing");
}
