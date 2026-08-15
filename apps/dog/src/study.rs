//! The single-dog **study**: the same animal the field walks, held still and
//! re-seated at the origin so a close camera can be flown around it.
//!
//! ## Why it is a re-seated field dog and not a second model
//!
//! Geometry is uploaded once at bind, so a study dog *cannot* be a second model
//! — there is one registered mesh set for the session and the study is drawn
//! with the same 23 bone meshes, in the same instance pool, as every dog on
//! every ring. What differs is only what the pose pass is handed.
//!
//! That constraint is the right one anyway. A study whose geometry, rig or gait
//! could drift from the field's would be a picture of a *different* dog, and the
//! whole reason to look closely at one is to see the animal the field is made
//! of. Here the study is that animal by construction: [`Study::pose`] calls the
//! same [`Gait::pose`] with the same rig, the same limb chains and the same
//! resolved dials, and then applies one rigid translation.
//!
//! ## Still, by construction
//!
//! [`Study::pose`] takes **no tick**. It is a pure function of the rig and the
//! configuration, so a stopped dog is stopped because there is no clock in the
//! expression — not because a paused flag is being honoured somewhere. The gait
//! dials still re-pose it (a longer leg is a taller dog, here as in the field);
//! the walk-speed dial cannot touch it, because there is no travel to scale.
//!
//! ## Standing, not frozen mid-air
//!
//! A dog stopped at an arbitrary point of its walk is a dog with a paw hanging
//! in the air — which reads as a *paused animation*, not as a standing animal.
//! The dachshund's four contacts are spread nearly a quarter-cycle apart (a
//! four-beat walk, not a two-beat trot: measured, its effective phases are
//! 0.17 / 0.37 / 0.67 / 0.87), so at a 0.52 stance duty there is **no** instant
//! of the cycle with all four paws down. No choice of travel can produce one.
//!
//! The study therefore does two derived things rather than one chosen one:
//!
//! * [`square_stance`] scans a whole stride and takes the instant with the least
//!   foot travel in the air, measured on the swing arc's own *shape* so the paw-
//!   lift dial cannot move which instant that is;
//! * it poses at that instant with the swing **height** at zero, because a still
//!   animal has no swing. The feet keep the fore-aft stations the walk gives
//!   them and every one of them is genuinely on the ground.
//!
//! The result is one standing dog at a natural mid-walk stance, and it still
//! answers every gait dial: a longer leg stands it taller, a deeper crouch folds
//! it, a wider duty moves which instant it is holding.
//!
//! ## Suspended in space
//!
//! The pose is taken on a fixed circle — the innermost ring's radius at the
//! app's defaults, so the study dog stands exactly as a field dog stands, curve
//! correction and terrain relief included — and is then translated by the ground
//! point it was standing on. Its paws therefore land on `y ≈ 0` at the origin,
//! with no terrain drawn under them: the ground it is posed against is real, and
//! simply not presented.

use axiom_math::Transform;
use axiom_mesh::MeshResult;

use crate::creature_dog::dog_limbs;
use crate::creature_pose::Gait;
use crate::creature_rig::{CreatureRig, LimbChain};
use crate::leg_ik::{stride_phase, swing_lift};
use crate::locomotion::LoopPath;
use crate::rings::Winding;

/// The circle the study pose is taken from, in world units — the innermost
/// ring's radius at the app's defaults (`Dial::InnerRadius`). Fixed rather than
/// read off the live configuration, so the ring dials cannot move the animal the
/// close camera is framed on: they lay out a field, and the study is not a
/// field.
const STUDY_RADIUS: f32 = 26.0;

/// The winding the study circle is authored in, fixed for the same reason the
/// radius is. It decides which way the dog faces, which is what the study
/// camera's framing is chosen against.
const STUDY_WINDING: Winding = Winding::CounterClockwise;

/// Where on that circle the pose is anchored. Zero puts the dog at the circle's
/// authored start, where the tangent runs down world `-Z` — the axis the dog is
/// modelled facing — so the study camera sees it in profile from `+X` rather
/// than at some angle that would have to be measured. [`square_stance`] then
/// offsets it by less than one stride to find the standing instant, which is far
/// too little to turn the dog appreciably off that axis.
const STUDY_ANCHOR: f32 = 0.0;

/// How many instants of one stride [`square_stance`] scans. The stance windows
/// it is looking for are as narrow as `2·duty − 1` of a cycle — 4% at the
/// authored duty — so the scan has to be fine enough to land inside one: 512
/// samples put ten of them in that window at the defaults, and the whole scan is
/// four cheap phase computations apiece, paid once per re-pose.
const STANCE_SAMPLES: usize = 512;

/// Where the study camera sits and what it looks at.
///
/// The target is the **posed dog's own centre**, measured off the pose rather
/// than assumed to be the origin: the re-seat puts the ground point under the
/// body at `(0, 0, 0)`, but the animal is not symmetric about it — the barrel
/// runs from `z ≈ −10` (nose) to `z ≈ +10` (tail), rising to `y ≈ 6.9` at the
/// skull. Aiming at the middle of *that* box is what puts the dog in the middle
/// of the frame, and — because the orbit rotates about its target — what makes
/// a full turn spin the animal in place instead of swinging it across the
/// canvas. `tests/` holds the pose inside the box this pair is framed for.
///
/// The eye is that centre plus a fixed offset: **19 units out, 15° above the
/// horizon, 64° round from the dog's own axis** — a near-profile three-quarter
/// view. At the app's 58° vertical field, 19 units spans ~39 world units across
/// the canvas, so the 24-unit animal fills about two-thirds of the frame: close
/// enough to read a swept limb's taper, far enough that a full orbit never puts
/// the camera inside the dog.
///
/// As with the field's framing in `install.rs`, this pair is authored **once**
/// and `src/orbit.rs` derives the interactive camera's opening yaw/pitch/
/// distance from it, so moving these numbers moves the whole study shot.
pub const STUDY_TARGET: [f32; 3] = [-1.2, 3.0, 2.4];
pub const STUDY_EYE: [f32; 3] = [
    STUDY_TARGET[0] + 16.5,
    STUDY_TARGET[1] + 4.9,
    STUDY_TARGET[2] + 8.0,
];

/// The still dog: the path its pose is taken from, its limb chains, and the
/// ground point it is re-seated by.
///
/// Built once, at install, because inverting a circle into an arc-length table
/// is startup work and not per-frame work (see `locomotion.rs`). Posing from it
/// afterwards is one dog's worth of forward kinematics and four two-bone solves.
#[derive(Debug, Clone)]
pub struct Study {
    /// The fixed circle the pose is taken from.
    path: LoopPath,
    /// The dog's four solvable legs — the same chains the field's animation
    /// walks on.
    limbs: [LimbChain; 4],
}

impl Study {
    /// Build the study's fixed path.
    pub fn new() -> MeshResult<Study> {
        Ok(Study {
            path: LoopPath::circle(STUDY_RADIUS, STUDY_WINDING)?,
            limbs: dog_limbs(),
        })
    }

    /// Every bone of the one dog, in rig order, re-seated at the origin.
    ///
    /// The re-seat is a pure **translation**: each bone keeps its own rotation
    /// and its own scale, because a leg bone is drawn at a different scale along
    /// its own length than across it (see `creature_pose.rs`) and a composed
    /// transform would not survive that. Translating is also all that is needed
    /// — the pose already faces the axis the study camera is framed against.
    pub fn pose(&self, rig: &CreatureRig, gait: Gait) -> Vec<Transform> {
        let standing = standing_gait(gait);
        let travel = STUDY_ANCHOR + square_stance(standing, &self.limbs);
        let ground = self.path.at(travel).position;
        standing
            .pose(rig, &self.limbs, &self.path, travel)
            .into_iter()
            .map(|bone| {
                Transform::new(
                    bone.translation.subtract(ground),
                    bone.rotation,
                    bone.scale,
                )
            })
            .collect()
    }
}

/// The live gait as a **standing** one: the same animal, the same stride, duty,
/// crouch, lean and flex — with the swing arc's height at zero.
///
/// This is the one thing the study changes about the walk it is holding, and it
/// changes it because the walk is not happening: a swing arc is where a foot is
/// *on its way* somewhere, and nothing here is on its way anywhere. Zeroing it
/// puts every paw on the ground the pose pass stood the body on, which is what a
/// still animal looks like. Nothing else about the gait is touched, so every
/// dial on the panel still reaches the study dog.
fn standing_gait(gait: Gait) -> Gait {
    Gait { lift: 0.0, ..gait }
}

/// How far into a stride this gait carries the least foot in the air, in world
/// units.
///
/// Scored on the swing arc's own **shape** (a unit-height lift), not on the
/// paw-lift dial's height, for two reasons: the study poses with that height at
/// zero, which would make every sample score zero and the answer arbitrary; and
/// which instant of a walk is the most planted is a property of the *timing* —
/// the stride, the duty and the limb offsets — rather than of how high the paws
/// are being picked up. The scan is over one stride because the gait repeats on
/// exactly that period.
fn square_stance(gait: Gait, limbs: &[LimbChain]) -> f32 {
    (0..STANCE_SAMPLES)
        .map(|sample| gait.stride * sample as f32 / STANCE_SAMPLES as f32)
        .map(|travel| (airborne(gait, limbs, travel), travel))
        .fold((f32::INFINITY, 0.0), |best, here| {
            [best, here][usize::from(here.0 < best.0)]
        })
        .1
}

/// How much of this gait's feet are in the air at `travel`, summed over the
/// limbs, on a unit swing arc. Zero means every paw is planted.
fn airborne(gait: Gait, limbs: &[LimbChain], travel: f32) -> f32 {
    limbs
        .iter()
        .map(|limb| {
            // The same phase the pose pass reads: a limb's contact sits `fore`
            // units along the path from the body, so that is where its own
            // stride is measured from. See `Gait::plant`.
            let fore = -limb.contact.z * gait.scale;
            let phase = stride_phase(travel + fore, gait.stride, limb.offset, gait.duty);
            swing_lift(phase.swing, 1.0)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Dial, SceneConfig};
    use crate::creature_dog::dog_parts;
    use crate::variant::SceneVariant;

    /// The rig every test below poses.
    fn rig() -> CreatureRig {
        dog_parts(SceneVariant::Coarse).expect("the authored dog is a valid rig")
    }

    #[test]
    fn the_study_is_the_whole_rig_standing_on_the_origin() {
        let study = Study::new().expect("the study circle is a valid path");
        let rig = rig();
        let config = SceneConfig::defaults();
        let pose = study.pose(&rig, config.gait());
        assert_eq!(pose.len(), rig.len(), "the study poses every bone");
        let feet = pose
            .iter()
            .map(|bone| bone.translation.y)
            .fold(f32::INFINITY, f32::min);
        let head = pose
            .iter()
            .map(|bone| bone.translation.y)
            .fold(f32::NEG_INFINITY, f32::max);
        // Its paws are on the plane the terrain would have been, and the animal
        // stands its own height above them rather than being buried in it.
        assert!(feet.abs() < 1.5, "the study's lowest bone is at y = {feet}");
        assert!(head > 3.0 && head < 12.0, "the study's highest bone is at y = {head}");
        // ...and it is at the origin in plan, which is what the close camera is
        // aimed at.
        let span = |axis: fn(&Transform) -> f32| {
            pose.iter().map(axis).fold(f32::NEG_INFINITY, f32::max)
                - pose.iter().map(axis).fold(f32::INFINITY, f32::min)
        };
        let centre = |axis: fn(&Transform) -> f32| {
            pose.iter().map(axis).sum::<f32>() / pose.len() as f32
        };
        assert!(centre(|b| b.translation.x).abs() < 4.0);
        assert!(centre(|b| b.translation.z).abs() < 4.0);
        // The dog is much longer than it is wide — it is lying along one axis,
        // which is the profile the study camera is framed for.
        let (length, width) = (span(|b| b.translation.z), span(|b| b.translation.x));
        assert!(length > width * 2.0, "{length} long, {width} wide");
    }

    #[test]
    fn the_study_camera_is_aimed_at_the_animal_it_is_framing() {
        let study = Study::new().expect("the study circle is a valid path");
        let rig = rig();
        let pose = study.pose(&rig, SceneConfig::defaults().gait());
        // The claim `STUDY_TARGET`'s doc makes: the shot is aimed at the middle
        // of the posed dog, not at the origin under its feet. Held against the
        // pose itself, so a re-proportioned animal that moved its own centre
        // fails here rather than quietly drifting out of frame.
        let axes: [fn(&Transform) -> f32; 3] = [
            |bone| bone.translation.x,
            |bone| bone.translation.y,
            |bone| bone.translation.z,
        ];
        axes.iter().zip(STUDY_TARGET).enumerate().for_each(
            |(axis, (component, aimed_at))| {
                let low = pose.iter().map(component).fold(f32::INFINITY, f32::min);
                let high = pose.iter().map(component).fold(f32::NEG_INFINITY, f32::max);
                let middle = (low + high) * 0.5;
                let slack = (high - low) * 0.25;
                assert!(
                    (aimed_at - middle).abs() <= slack.max(1.0),
                    "axis {axis}: the study camera aims at {aimed_at}, \
                     the dog's middle is {middle} of {low}..{high}"
                );
            },
        );
        // ...and the eye is off that centre by the authored offset, not sitting
        // on top of it.
        let distance = (0..3)
            .map(|axis| (STUDY_EYE[axis] - STUDY_TARGET[axis]).powi(2))
            .sum::<f32>()
            .sqrt();
        assert!((distance - 19.0).abs() < 0.2, "the study eye is {distance} out");
    }

    #[test]
    fn the_study_is_still_and_deterministic() {
        let study = Study::new().expect("the study circle is a valid path");
        let rig = rig();
        let config = SceneConfig::defaults();
        // There is no tick to advance: the same configuration always produces
        // exactly the same pose, bone for bone.
        let once = study.pose(&rig, config.gait());
        let twice = study.pose(&rig, config.gait());
        assert_eq!(once.len(), twice.len());
        assert!(once
            .iter()
            .zip(twice.iter())
            .all(|(a, b)| a.translation == b.translation && a.rotation == b.rotation));
        // ...and the walk-speed dial, which is the only dial that means "time",
        // cannot move it at all.
        let hurried = study.pose(&rig, config.with(Dial::Speed, 0.6).gait());
        assert!(once
            .iter()
            .zip(hurried.iter())
            .all(|(a, b)| a.translation == b.translation));
    }

    /// The height of each of the four paw blocks in a posed study.
    fn paws(rig: &CreatureRig, pose: &[Transform]) -> Vec<f32> {
        dog_limbs()
            .iter()
            .filter_map(|limb| rig.index_of(limb.tip))
            .map(|index| pose[index].translation.y)
            .collect()
    }

    #[test]
    fn the_still_dog_stands_on_all_four_paws() {
        let study = Study::new().expect("the study circle is a valid path");
        let rig = rig();
        let config = SceneConfig::defaults();
        // The whole point of the study pose: four paws, all of them on the
        // ground the body was stood on, with none of them hanging mid-swing.
        // The band they are allowed to spread over is the terrain relief the
        // legs may follow — a real dog on real ground, not a dog on a table.
        let feet = paws(&rig, &study.pose(&rig, config.gait()));
        assert_eq!(feet.len(), 4, "the study did not pose four paws");
        let low = feet.iter().copied().fold(f32::INFINITY, f32::min);
        let high = feet.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            high - low < config.relief() * 2.0,
            "the study dog is standing on {feet:?}"
        );

        // ...and it stays standing at every stance duty, including the ones with
        // no all-four-down instant in the cycle at all.
        for duty in [0.30, 0.45, 0.52, 0.70, 0.90] {
            let feet = paws(&rig, &study.pose(&rig, config.with(Dial::Duty, duty).gait()));
            let low = feet.iter().copied().fold(f32::INFINITY, f32::min);
            let high = feet.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            assert!(high - low < config.relief() * 2.0, "at duty {duty}: {feet:?}");
        }
    }

    #[test]
    fn the_stance_scan_finds_the_most_planted_instant_of_the_walk() {
        let config = SceneConfig::defaults();
        let limbs = dog_limbs();
        let gait = super::standing_gait(config.gait());
        let best = square_stance(gait, &limbs);
        // It lands inside the stride it scans — the study dog is anchored where
        // the study camera is framed, not a lap away from it.
        assert!((0.0..gait.stride).contains(&best));
        // And it really is the minimum: no sample of the cycle has less foot in
        // the air than the one it picked.
        let score = airborne(gait, &limbs, best);
        assert!((0..97)
            .map(|s| airborne(gait, &limbs, gait.stride * s as f32 / 97.0))
            .all(|other| other >= score - 1.0e-3));
        // The dachshund's contacts are spread nearly a quarter-cycle apart, so
        // there is genuinely no all-four-down instant to find — which is exactly
        // why the study poses with the swing height at zero.
        assert!(score > 0.0, "a four-beat walk cannot have four feet down");
        assert_eq!(super::standing_gait(config.gait()).lift, 0.0);
    }

    #[test]
    fn a_gait_dial_still_re_poses_the_still_dog() {
        let study = Study::new().expect("the study circle is a valid path");
        let rig = rig();
        let config = SceneConfig::defaults();
        let standing = study.pose(&rig, config.gait());
        let taller = study.pose(&rig, config.with(Dial::LegLength, 1.8).gait());
        let top = |pose: &[Transform]| {
            pose.iter()
                .map(|bone| bone.translation.y)
                .fold(f32::NEG_INFINITY, f32::max)
        };
        assert!(
            top(&taller) > top(&standing) + 0.5,
            "a longer leg did not stand the study dog taller: {} vs {}",
            top(&taller),
            top(&standing)
        );
    }
}
