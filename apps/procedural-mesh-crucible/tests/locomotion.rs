//! The locomotion proof suite: the rings, the rig, the gait and the inverse
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
//! 3. **The paws do not skate** — a planted paw's world position is *constant*
//!    for the whole of its stance while the body travels over it.
//! 4. **No leg over-reaches** — on either ring, over more than a full lap.
//! 5. **The rings are closed, uniform and inside the terrain.**
//!
//! Which way round each ring turns, and how the crowd is laid out along it, is
//! `tests/rings.rs`.

use axiom_math::{Transform, Vec3};
use axiom_procedural_mesh_crucible::{
    dog_limbs, dog_parts, dog_travel, solve_two_bone, stride_phase, CrucibleAnimation,
    CrucibleVariant, LoopPath, DOG_GAIT, INNER, OUTER, RINGS, TRAVEL_PER_TICK,
};

/// The terrain's half-extent — nothing may walk off it.
const TERRAIN_HALF_EXTENT: f32 = 96.0;

fn animation() -> CrucibleAnimation {
    CrucibleAnimation::new(dog_parts(CrucibleVariant::Base).expect("the dog rigs"))
        .expect("the rings build")
}

fn finite(t: &Transform) -> bool {
    [t.translation, t.scale]
        .iter()
        .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
        && [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w]
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
    assert!(
        knee.joint.z < -0.4,
        "the knee did not lead forward: {:?}",
        knee.joint
    );
    assert!(
        elbow.joint.z > 0.4,
        "the elbow did not lead back: {:?}",
        elbow.joint
    );
}

#[test]
fn the_rig_resolves_in_one_forward_pass() {
    for variant in CrucibleVariant::ALL {
        let rig = dog_parts(variant).expect("the dog rigs");
        // Parents precede children — the invariant the single pass rests on.
        for (index, part) in rig.parts().iter().enumerate() {
            if let Some(parent) = part.parent {
                assert!(parent < index, "{} is declared before its parent", part.name);
            }
        }
        // Exactly one root.
        let roots = rig.parts().iter().filter(|p| p.parent.is_none()).count();
        assert_eq!(roots, 1, "the dog has {roots} root bones");
        // Names are unique — the scene addresses bones by them.
        let mut names: Vec<&str> = rig.parts().iter().map(|p| p.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "the dog has duplicate bone names");
        // And the pass lands every bone somewhere finite.
        let world = rig.rest_world(Transform::IDENTITY);
        assert_eq!(world.len(), rig.len());
        assert!(
            world.iter().all(finite),
            "the dog resolved a bone to a non-finite transform"
        );
    }
}

#[test]
fn every_bone_of_every_dog_stays_finite_for_a_long_run() {
    let animation = animation();
    for tick in (0u64..3_000).step_by(53) {
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
    // The lead dog's front-left leg: phase offset 0, contact 0.310 behind the
    // origin. It is dog 0, so its bones are the first block of transforms.
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
            (true, Some((step, first, drift, samples))) if step == phase.step => {
                Some((step, first, drift.max(first.distance(here)), samples + 1))
            }
            (true, done) => {
                done.map(|(_, _, drift, samples)| stances.push((drift, samples)));
                Some((phase.step, here, 0.0, 1))
            }
        };
    }

    assert!(
        stances.len() >= 4,
        "only {} stances observed in 90 ticks — the gait is not cycling",
        stances.len()
    );
    // The first entry began before tick 0, so it is a fragment of a stance
    // rather than a stance — the window starts where it starts. Every entry
    // after it is a whole one, and those are the ones that can prove anything.
    for (drift, samples) in &stances[1..] {
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

/// The tuning check that keeps the gait honest.
///
/// The two-bone solver *cannot* stretch a bone: handed a target beyond reach it
/// clamps, and the paw then comes off its plant. So "no leg is ever asked to
/// reach further than it is long" is exactly equivalent to "no paw ever skates",
/// and it is far easier to measure — one distance per limb per tick, across
/// every dog on both rings, over more than a full lap of real terrain.
///
/// A tuning edit that lengthens a stride, shallows a crouch or widens the
/// terrain relief cap past what the legs can absorb fails HERE, with the
/// offending limb and the number, instead of shipping a sliding dog.
#[test]
fn no_limb_is_ever_asked_to_reach_further_than_it_is_long() {
    let rig = dog_parts(CrucibleVariant::Base).expect("the dog rigs");
    let animation = CrucibleAnimation::new(rig.clone()).expect("the rings build");
    let bones = animation.bone_count();
    // 900 ticks is 558 units of travel — nearly two laps of the outer ring and
    // more than three of the inner one, so every dog crosses every part of the
    // terrain its own ring runs over.
    let samples: Vec<Vec<Transform>> = (0u64..900)
        .step_by(3)
        .map(|tick| animation.transforms(tick))
        .collect();

    for limb in dog_limbs().iter() {
        let hip = rig.index_of(limb.upper).expect("the limb's upper bone");
        let tip = rig.index_of(limb.tip).expect("the limb's terminating block");
        let reach = (limb.len_upper + limb.len_lower + limb.len_extra) * DOG_GAIT.scale;
        let worst = animation
            .dogs()
            .iter()
            .enumerate()
            .flat_map(|(dog, _)| {
                samples.iter().map(move |all| {
                    all[dog * bones + hip]
                        .translation
                        .distance(all[dog * bones + tip].translation)
                })
            })
            .fold(0.0f32, f32::max);
        println!(
            "[reach] {:<22} reach {reach:6.3}  worst {worst:6.3}  ({:.0}%)",
            limb.upper,
            100.0 * worst / reach
        );
        assert!(
            worst < reach * 0.97,
            "the dog's {} is stretched to {worst} of its {reach} reach — the gait is \
             over-striding and the paw will come off its plant",
            limb.upper
        );
    }
}

#[test]
fn both_rings_are_closed_uniform_and_inside_the_terrain() {
    for ring in RINGS {
        let path = LoopPath::ring(ring).expect("the ring builds");
        let total = path.total();
        // A circle of this radius, so the sampled length is within a percent.
        let circumference = ring.circumference();
        assert!(
            (total - circumference).abs() / circumference < 0.02,
            "the {} ring measures {total}, not ~{circumference}",
            ring.name
        );
        println!(
            "[ring] {:<6} radius {}, length {total:.1}, lap {:.1} s",
            ring.name,
            ring.radius,
            total / (TRAVEL_PER_TICK * 60.0)
        );

        // Closed: one full lap returns to the start, in position and in heading.
        let start = path.at(0.0);
        let lap = path.at(total);
        assert!(
            start.position.distance(lap.position) < 1.0e-2,
            "the {} ring does not close",
            ring.name
        );
        assert!(
            start.forward.distance(lap.forward) < 1.0e-2,
            "the {} ring kinks at its seam",
            ring.name
        );

        // Uniform: equal arc-length steps really are equal distances on the
        // ground. This is what keeps a walker from surging between samples.
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
            "the {} ring's arc-length sampling is not uniform: spans run {shortest} to {longest}",
            ring.name
        );

        // On the terrain, at the authored radius, all the way round.
        for i in 0..512 {
            let point = path.at(i as f32 * total / 512.0).position;
            assert!(
                point.x.abs() < TERRAIN_HALF_EXTENT && point.z.abs() < TERRAIN_HALF_EXTENT,
                "the {} ring leaves the terrain at {point:?}",
                ring.name
            );
            let radius = Vec3::new(point.x, 0.0, point.z).length();
            assert!(
                (radius - ring.radius).abs() < 0.5,
                "the {} ring wanders to radius {radius}",
                ring.name
            );
        }
    }

    // The two rings clear each other by more than a dog is wide, so the inner
    // chain never walks through the outer one.
    let gap = OUTER.radius - INNER.radius;
    assert!(gap > 12.0, "the rings are only {gap} apart");
}
