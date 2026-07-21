//! The locomotion + biomechanics debug view: the development-only markers and
//! text rows that make the gait and the whole-body sprint carriage inspectable.
//!
//! Split out of [`super`] so the general diagnostic markers (routes, collision,
//! trajectory) stay separate from the locomotion-biomechanics read-out. Like
//! everything under `debug`, this reads the immutable snapshot only and can
//! never affect the simulation or the pose.

use axiom::prelude::Vec3;

use crate::presentation::locomotion::PlantedFoot;
use crate::presentation::LocomotionSample;

use super::{cube, push, DebugInstance, DebugMaterial};
use crate::presentation::locomotion::OverrideReason;

/// The biomechanical markers for one player: the three roots (gameplay root,
/// the visual body root derived from it, and the pelvis riding under that), the
/// weight-shift point, and the foot currently bearing weight. Reading them
/// together is how you see whether weight is actually stacked over the stance
/// leg. Development-only — drawn by the same F1 debug view as the foot markers,
/// and never able to affect the sim or the pose.
pub fn markers(sample: &LocomotionSample, out: &mut Vec<DebugInstance>) {
    // The gameplay root sits on the turf; the visual body root floats above it
    // by exactly the cosmetic offset, so any gap between them IS the offset.
    push(
        out,
        cube(sample.gameplay_root, 0.14),
        DebugMaterial::GameplayRoot,
    );
    push(out, cube(sample.visual_root, 0.12), DebugMaterial::VisualRoot);
    push(out, cube(sample.pelvis, 0.15), DebugMaterial::Pelvis);
    push(
        out,
        cube(sample.weight_point, 0.18),
        DebugMaterial::WeightPoint,
    );
    let stance = match sample.carriage.stance {
        PlantedFoot::Left => sample.left_ankle,
        PlantedFoot::Right => sample.right_ankle,
    };
    push(out, cube(stance, 0.2), DebugMaterial::StanceFoot);
}

/// The whole-body biomechanics read-out: the gait phase and which foot is
/// bearing weight, how far through its stance that foot is, and the cosmetic
/// offset between the gameplay root and the visual body root — the number that
/// proves the hips are moving without the gameplay position moving.
pub fn push_rows(rows: &mut Vec<(String, String)>, loco: &LocomotionSample) {
    let c = loco.carriage;
    let stance = match c.stance {
        PlantedFoot::Left => "L",
        PlantedFoot::Right => "R",
    };
    let leg = ["stance", "flight"][usize::from(c.in_flight)];
    rows.push((
        "bio.gait".to_string(),
        format!("{:.3} {stance} {leg} {:.2}", loco.gait_phase, c.stance_progress),
    ));
    let offset = loco.visual_root.subtract(loco.gameplay_root);
    rows.push((
        "bio.visualOffset".to_string(),
        format!("lat {:.3} / lift {:.3} yd", c.root_lateral, c.root_lift),
    ));
    rows.push((
        "bio.rootGap".to_string(),
        format!("{:.3} yd", Vec3::new(offset.x, 0.0, offset.z).length()),
    ));
    rows.push((
        "bio.pelvis".to_string(),
        format!(
            "yaw {:.3} roll {:.3} pitch {:.3}",
            c.pelvis_yaw, c.pelvis_roll, c.pelvis_pitch
        ),
    ));
}


/// Small markers for one player's locomotion: planted-foot lock targets, the
/// current solved foot positions, the swing foot's next landing, and the
/// resolved movement vector. Purely diagnostic — never affects the sim or pose.
pub fn foot_markers(sample: &LocomotionSample, pos: Vec3, out: &mut Vec<DebugInstance>) {
    push(out, cube(sample.left_ankle, 0.1), DebugMaterial::FootNow);
    push(out, cube(sample.right_ankle, 0.1), DebugMaterial::FootNow);
    push(
        out,
        cube(sample.planted_target, 0.16),
        DebugMaterial::FootLock,
    );
    push(
        out,
        cube(sample.next_landing, 0.13),
        DebugMaterial::FootLanding,
    );
    // The resolved movement vector: a few dots from the player along the actual
    // displacement this tick (scaled up so a slow drift is still visible).
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        let tip = Vec3::new(
            pos.x + sample.move_vector.x * t * 8.0,
            0.2,
            pos.z + sample.move_vector.z * t * 8.0,
        );
        push(out, cube(tip, 0.08), DebugMaterial::MoveVector);
    }
}

/// The locomotion read-out for the selected player: authoritative vs requested
/// speed, actual distance moved, mode, gait phase, stride, cadence, planted
/// foot, both foot states, both lock errors, and any override.
pub fn push_locomotion_rows(rows: &mut Vec<(String, String)>, requested: f32, loco: &LocomotionSample) {
    let planted = match loco.planted {
        PlantedFoot::Left => "L",
        PlantedFoot::Right => "R",
    };
    rows.push(("loco.speed".to_string(), format!("{:.2} yd/s", loco.speed)));
    rows.push(("loco.requested".to_string(), format!("{requested:.2} yd/s")));
    rows.push((
        "loco.moved".to_string(),
        format!("{:.4} yd", loco.distance_moved),
    ));
    rows.push(("loco.mode".to_string(), format!("{:?}", loco.mode)));
    rows.push(("loco.phase".to_string(), format!("{:.3}", loco.gait_phase)));
    rows.push((
        "loco.stride".to_string(),
        format!("{:.2} yd", loco.stride_length),
    ));
    rows.push((
        "loco.cadence".to_string(),
        format!("{:.2} /s", loco.cadence),
    ));
    rows.push(("loco.planted".to_string(), planted.to_string()));
    rows.push((
        "loco.feet".to_string(),
        format!("L {:?} / R {:?}", loco.left_phase, loco.right_phase),
    ));
    rows.push((
        "loco.lockErr".to_string(),
        format!(
            "L {:.3} / R {:.3}",
            loco.left_lock_error, loco.right_lock_error
        ),
    ));
    let over = match loco.reason {
        OverrideReason::None => "no".to_string(),
        other => format!("yes ({other:?})"),
    };
    rows.push(("loco.override".to_string(), over));
    push_rows(rows, loco);
}

