//! Authoring the sample figure and motion as **portable data**.
//!
//! This is the one place the lab's built-in *content* lives: a generic 13-part
//! articulated box-figure (torso, head, and four two-segment limbs) and a
//! sagittal motion clip that swings its limbs. It builds them
//! through the generic `axiom-figure` and `axiom-animation` facades and
//! serializes them to bytes. Nothing here is engine code; it is example content
//! expressed against generic mechanisms, so any other figure and clip can be
//! substituted by loading different bytes.

use axiom_animation::{AnimationApi, BoneId};
use axiom_figure::{FigureApi, FigureDefinition, FigurePart};
use axiom_math::{Quat, Transform, Vec3};

/// Total frames in the motion clip.
pub const FRAME_COUNT: u32 = 48;
/// The frame the sample event fires on.
pub const EVENT_FRAME: u32 = 33;
/// Opaque clip-event code the app reads back as "the marked event fires".
pub const EVENT_CODE: u32 = 1;
/// Part index of the joint that swings the most (highlighted in the view).
pub const SWING_JOINT: usize = 8;
/// Part index of the joint that stays anchored (highlighted in the view).
pub const ANCHOR_JOINT: usize = 5;

// Opaque render tags (a consumer maps these to materials).
const TAG_BODY: u32 = 0;
const TAG_PELVIS: u32 = 1;
const TAG_SKIN: u32 = 2;
const TAG_LIMB: u32 = 3;
const TAG_END: u32 = 4;

/// `(parent, rest offset, box size, box offset, tag)` for each of the 13 parts,
/// in parent-before-child order. Y up, +Z forward (motion direction), +X right.
/// Boxes pivot at the joint (part origin) and are centered along the segment via
/// the box offset.
struct PartSpec {
    parent: Option<u32>,
    offset: Vec3,
    box_size: Vec3,
    box_offset: Vec3,
    tag: u32,
}

const fn p(
    parent: Option<u32>,
    offset: Vec3,
    box_size: Vec3,
    box_offset: Vec3,
    tag: u32,
) -> PartSpec {
    PartSpec {
        parent,
        offset,
        box_size,
        box_offset,
        tag,
    }
}

// Proportion pass toward the lean reference athlete (broad squared shoulders,
// small head, trim tapered waist, long slightly-slimmer legs, slimmer arms). The
// root sits 0.06 higher to soak up the extra leg length so the feet keep their
// original ground contact. Segment child offsets and box offsets track the new
// thigh/shin lengths (offset = full segment length; box offset = half length) so
// the limbs stay connected end-to-end.
const PARTS: [PartSpec; 13] = [
    p(
        None,
        Vec3::new(0.0, 1.06, 0.0),
        Vec3::new(0.32, 0.30, 0.24),
        Vec3::ZERO,
        TAG_PELVIS,
    ), // 0 pelvis (trimmer waist, raised to reground longer legs)
    p(
        Some(0),
        Vec3::new(0.0, 0.34, 0.0),
        Vec3::new(0.48, 0.44, 0.28),
        Vec3::new(0.0, 0.06, 0.0),
        TAG_BODY,
    ), // 1 chest (broader squared shoulders)
    p(
        Some(1),
        Vec3::new(0.0, 0.36, 0.0),
        Vec3::new(0.19, 0.22, 0.20),
        Vec3::new(0.0, 0.08, 0.0),
        TAG_SKIN,
    ), // 2 head (smaller)
    p(
        Some(0),
        Vec3::new(-0.11, -0.06, 0.0),
        Vec3::new(0.16, 0.52, 0.18),
        Vec3::new(0.0, -0.26, 0.0),
        TAG_SKIN,
    ), // 3 L thigh (longer, slimmer)
    p(
        Some(3),
        Vec3::new(0.0, -0.52, 0.0),
        Vec3::new(0.14, 0.50, 0.15),
        Vec3::new(0.0, -0.25, 0.0),
        TAG_LIMB,
    ), // 4 L shin (longer, slimmer)
    p(
        Some(4),
        Vec3::new(0.0, -0.50, 0.0),
        Vec3::new(0.15, 0.11, 0.30),
        Vec3::new(0.0, -0.02, 0.09),
        TAG_END,
    ), // 5 L foot (anchor)
    p(
        Some(0),
        Vec3::new(0.11, -0.06, 0.0),
        Vec3::new(0.16, 0.52, 0.18),
        Vec3::new(0.0, -0.26, 0.0),
        TAG_SKIN,
    ), // 6 R thigh (longer, slimmer)
    p(
        Some(6),
        Vec3::new(0.0, -0.52, 0.0),
        Vec3::new(0.14, 0.50, 0.15),
        Vec3::new(0.0, -0.25, 0.0),
        TAG_LIMB,
    ), // 7 R shin (longer, slimmer)
    p(
        Some(7),
        Vec3::new(0.0, -0.50, 0.0),
        Vec3::new(0.15, 0.11, 0.30),
        Vec3::new(0.0, -0.02, 0.09),
        TAG_END,
    ), // 8 R foot (swing)
    p(
        Some(1),
        Vec3::new(-0.32, 0.16, 0.0),
        Vec3::new(0.12, 0.44, 0.12),
        Vec3::new(0.0, -0.22, 0.0),
        TAG_BODY,
    ), // 9 L upper-arm (wider set, slimmer)
    p(
        Some(9),
        Vec3::new(0.0, -0.44, 0.0),
        Vec3::new(0.10, 0.40, 0.10),
        Vec3::new(0.0, -0.20, 0.0),
        TAG_SKIN,
    ), // 10 L forearm (slimmer)
    p(
        Some(1),
        Vec3::new(0.32, 0.16, 0.0),
        Vec3::new(0.12, 0.44, 0.12),
        Vec3::new(0.0, -0.22, 0.0),
        TAG_BODY,
    ), // 11 R upper-arm (wider set, slimmer)
    p(
        Some(11),
        Vec3::new(0.0, -0.44, 0.0),
        Vec3::new(0.10, 0.40, 0.10),
        Vec3::new(0.0, -0.20, 0.0),
        TAG_SKIN,
    ), // 12 R forearm (slimmer)
];

/// A per-part sagittal pitch track (rotation about X): `(frame, radians)`.
struct PitchTrack {
    part: u32,
    keys: &'static [(u32, f32)],
}

const MOTION_TRACKS: &[PitchTrack] = &[
    PitchTrack {
        part: 1,
        keys: &[(0, 0.0), (9, 0.20), (26, 0.34), (33, 0.14), (47, 0.05)],
    }, // torso lean (stronger run-up lean; peaks at the displayed plant)
    PitchTrack {
        part: 6,
        keys: &[
            (0, 0.0),
            (15, -0.15),
            (21, 0.10),
            (27, 0.70),
            (33, -0.90),
            (39, -0.50),
            (47, 0.0),
        ],
    }, // R lower-limb root swing
    PitchTrack {
        part: 7,
        keys: &[(0, 0.15), (27, 1.20), (33, 0.10), (39, 0.50), (47, 0.20)],
    }, // R lower-limb mid
    PitchTrack {
        part: 11,
        keys: &[(0, 0.0), (26, -0.55), (33, 0.55), (47, 0.0)],
    }, // R upper-limb counter-swing (stronger; leads the cocked leg)
    PitchTrack {
        part: 9,
        keys: &[(0, 0.0), (26, 0.60), (33, -0.55), (47, 0.0)],
    }, // L upper-limb counter-swing (stronger; trails back for balance)
    PitchTrack {
        part: 3,
        keys: &[(0, 0.0), (21, -0.10), (47, 0.0)],
    }, // L lower-limb root (anchor)
    PitchTrack {
        part: 4,
        keys: &[(0, 0.10), (21, 0.30), (47, 0.10)],
    }, // L lower-limb mid (anchor)
];

/// The eight motion phases in order, as `(name, start, end)` frame spans. The
/// phase *code* stored in the clip is the index; the name is app-side meaning.
pub const PHASES: [(&str, u32, u32); 8] = [
    ("rest", 0, 6),
    ("anticipate", 6, 12),
    ("prepare", 12, 18),
    ("load", 18, 24),
    ("windup", 24, 30),
    ("action", 30, 36),
    ("follow_through", 36, 42),
    ("recover", 42, 48),
];

/// The name of the phase with code `code`, or `"-"`.
pub fn phase_name(code: u32) -> &'static str {
    PHASES.get(code as usize).map_or("-", |(name, _, _)| *name)
}

/// The rest local transform of part `i` (its offset, identity rotation).
fn rest_of(i: u32) -> Transform {
    Transform::from_translation(PARTS[i as usize].offset)
}

/// Build the sample figure (the render rig).
pub fn build_figure() -> FigureDefinition {
    let parts = PARTS
        .iter()
        .map(|s| match s.parent {
            None => FigurePart::root(rest_of(0), s.box_size, s.box_offset, s.tag),
            Some(parent) => FigurePart::child(
                parent,
                Transform::from_translation(s.offset),
                s.box_size,
                s.box_offset,
                s.tag,
            ),
        })
        .collect();
    FigureDefinition::new(parts)
}

/// The sample figure serialized to portable bytes.
pub fn figure_bytes() -> Vec<u8> {
    FigureApi::new().serialize(&build_figure())
}

/// A pitch rotation about X, as a local transform keeping the part's rest
/// offset.
fn pitch_transform(part: u32, angle: f32) -> Transform {
    Transform::new(
        PARTS[part as usize].offset,
        Quat::from_euler_xyz(angle, 0.0, 0.0),
        Vec3::ONE,
    )
}

/// The motion clip serialized to portable bytes: pitch tracks, the eight phases,
/// and the sample event on the event frame.
pub fn clip_bytes() -> Vec<u8> {
    use axiom_kernel::Tick;
    let mut api = AnimationApi::new();
    let clip = api.create_clip();
    for track in MOTION_TRACKS {
        let keys: Vec<(Tick, Transform)> = track
            .keys
            .iter()
            .map(|&(frame, angle)| {
                (
                    Tick::new(u64::from(frame)),
                    pitch_transform(track.part, angle),
                )
            })
            .collect();
        api.add_track(clip, BoneId::from_raw(u64::from(track.part)), &keys)
            .unwrap();
    }
    for (code, (_, start, end)) in PHASES.iter().enumerate() {
        api.add_phase(
            clip,
            Tick::new(u64::from(*start)),
            Tick::new(u64::from(*end)),
            code as u32,
        )
        .unwrap();
    }
    api.add_event(clip, Tick::new(u64::from(EVENT_FRAME)), EVENT_CODE)
        .unwrap();
    api.serialize_clip(clip).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figure_validates_and_has_thirteen_parts() {
        let figure = build_figure();
        assert_eq!(figure.part_count(), 13);
        assert_eq!(FigureApi::new().validate(&figure), Ok(()));
    }

    #[test]
    fn figure_and_clip_bytes_are_deterministic_and_reloadable() {
        assert_eq!(figure_bytes(), figure_bytes());
        assert_eq!(clip_bytes(), clip_bytes());
        // Both round-trip through their module facades.
        assert!(FigureApi::new().deserialize(&figure_bytes()).is_ok());
        let mut api = AnimationApi::new();
        assert!(api.deserialize_clip(&clip_bytes()).is_ok());
    }

    #[test]
    fn phase_names_cover_the_timeline() {
        assert_eq!(phase_name(0), "rest");
        assert_eq!(phase_name(5), "action");
        assert_eq!(phase_name(99), "-");
    }
}
