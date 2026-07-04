//! Authoring the soccer kicker as **portable data**.
//!
//! This is the one place the kicker's *meaning* lives: a refined 13-part
//! articulated figure (torso, head, two legs with knees, two arms with elbows)
//! and a sagittal right-foot kick clip. It builds them through the generic
//! `axiom-figure` and `axiom-animation` facades and serializes them to bytes —
//! the exact bytes the game embeds. Tuning the kick here and re-emitting the
//! assets is how the lab and the game stay 1-1. Nothing here is engine code;
//! it is a game's content expressed against generic mechanisms.

use axiom_animation::{AnimationApi, BoneId};
use axiom_figure::{FigureApi, FigureDefinition, FigurePart};
use axiom_math::{Quat, Transform, Vec3};

/// Total frames in the kick clip.
pub const FRAME_COUNT: u32 = 48;
/// The frame the `KickContact` event fires on.
pub const CONTACT_FRAME: u32 = 33;
/// Opaque clip-event code the app reads back as "the ball is struck".
pub const KICK_CONTACT_CODE: u32 = 1;
/// Part index of the kicking (right) foot.
pub const RIGHT_FOOT: usize = 8;
/// Part index of the support (plant, left) foot.
pub const LEFT_FOOT: usize = 5;

// Opaque render tags (a game maps these to materials).
const TAG_JERSEY: u32 = 0;
const TAG_SHORTS: u32 = 1;
const TAG_SKIN: u32 = 2;
const TAG_SOCK: u32 = 3;
const TAG_BOOT: u32 = 4;

/// `(parent, rest offset, box size, box offset, tag)` for each of the 13 parts,
/// in parent-before-child order. Y up, +Z forward (kick direction), +X right.
/// Boxes pivot at the joint (part origin) and are centered along the segment via
/// the box offset.
struct PartSpec {
    parent: Option<u32>,
    offset: Vec3,
    box_size: Vec3,
    box_offset: Vec3,
    tag: u32,
}

const fn p(parent: Option<u32>, offset: Vec3, box_size: Vec3, box_offset: Vec3, tag: u32) -> PartSpec {
    PartSpec { parent, offset, box_size, box_offset, tag }
}

const PARTS: [PartSpec; 13] = [
    p(None, Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.34, 0.30, 0.24), Vec3::ZERO, TAG_SHORTS), // 0 pelvis
    p(Some(0), Vec3::new(0.0, 0.34, 0.0), Vec3::new(0.42, 0.44, 0.28), Vec3::new(0.0, 0.06, 0.0), TAG_JERSEY), // 1 chest
    p(Some(1), Vec3::new(0.0, 0.36, 0.0), Vec3::new(0.22, 0.26, 0.24), Vec3::new(0.0, 0.08, 0.0), TAG_SKIN), // 2 head
    p(Some(0), Vec3::new(-0.11, -0.06, 0.0), Vec3::new(0.17, 0.48, 0.19), Vec3::new(0.0, -0.24, 0.0), TAG_SKIN), // 3 L thigh
    p(Some(3), Vec3::new(0.0, -0.48, 0.0), Vec3::new(0.15, 0.46, 0.16), Vec3::new(0.0, -0.23, 0.0), TAG_SOCK), // 4 L shin
    p(Some(4), Vec3::new(0.0, -0.48, 0.0), Vec3::new(0.15, 0.11, 0.30), Vec3::new(0.0, -0.02, 0.09), TAG_BOOT), // 5 L foot
    p(Some(0), Vec3::new(0.11, -0.06, 0.0), Vec3::new(0.17, 0.48, 0.19), Vec3::new(0.0, -0.24, 0.0), TAG_SKIN), // 6 R thigh
    p(Some(6), Vec3::new(0.0, -0.48, 0.0), Vec3::new(0.15, 0.46, 0.16), Vec3::new(0.0, -0.23, 0.0), TAG_SOCK), // 7 R shin
    p(Some(7), Vec3::new(0.0, -0.48, 0.0), Vec3::new(0.15, 0.11, 0.30), Vec3::new(0.0, -0.02, 0.09), TAG_BOOT), // 8 R foot
    p(Some(1), Vec3::new(-0.28, 0.16, 0.0), Vec3::new(0.14, 0.44, 0.14), Vec3::new(0.0, -0.22, 0.0), TAG_JERSEY), // 9 L upper arm
    p(Some(9), Vec3::new(0.0, -0.44, 0.0), Vec3::new(0.12, 0.40, 0.12), Vec3::new(0.0, -0.20, 0.0), TAG_SKIN), // 10 L forearm
    p(Some(1), Vec3::new(0.28, 0.16, 0.0), Vec3::new(0.14, 0.44, 0.14), Vec3::new(0.0, -0.22, 0.0), TAG_JERSEY), // 11 R upper arm
    p(Some(11), Vec3::new(0.0, -0.44, 0.0), Vec3::new(0.12, 0.40, 0.12), Vec3::new(0.0, -0.20, 0.0), TAG_SKIN), // 12 R forearm
];

/// A per-part sagittal pitch track (rotation about X): `(frame, radians)`.
struct PitchTrack {
    part: u32,
    keys: &'static [(u32, f32)],
}

const KICK_TRACKS: &[PitchTrack] = &[
    PitchTrack { part: 1, keys: &[(0, 0.0), (9, 0.18), (33, 0.12), (47, 0.05)] }, // chest lean
    PitchTrack { part: 6, keys: &[(0, 0.0), (15, -0.15), (21, 0.10), (27, 0.70), (33, -0.90), (39, -0.50), (47, 0.0)] }, // R hip swing
    PitchTrack { part: 7, keys: &[(0, 0.15), (27, 1.20), (33, 0.10), (39, 0.50), (47, 0.20)] }, // R knee
    PitchTrack { part: 11, keys: &[(0, 0.0), (27, -0.40), (33, 0.50), (47, 0.0)] }, // R arm counter-swing
    PitchTrack { part: 9, keys: &[(0, 0.0), (27, 0.40), (33, -0.50), (47, 0.0)] }, // L arm counter-swing
    PitchTrack { part: 3, keys: &[(0, 0.0), (21, -0.10), (47, 0.0)] }, // L (plant) hip
    PitchTrack { part: 4, keys: &[(0, 0.10), (21, 0.30), (47, 0.10)] }, // L (plant) knee
];

/// The eight kick phases in order, as `(name, start, end)` frame spans. The
/// phase *code* stored in the clip is the index; the name is app-side meaning.
pub const KICK_PHASES: [(&str, u32, u32); 8] = [
    ("ready", 0, 6),
    ("lean_forward", 6, 12),
    ("approach", 12, 18),
    ("plant", 18, 24),
    ("backswing", 24, 30),
    ("strike", 30, 36),
    ("follow_through", 36, 42),
    ("recover", 42, 48),
];

/// The name of the phase with code `code`, or `"-"`.
pub fn phase_name(code: u32) -> &'static str {
    KICK_PHASES.get(code as usize).map_or("-", |(name, _, _)| *name)
}

/// The rest local transform of part `i` (its offset, identity rotation).
fn rest_of(i: u32) -> Transform {
    Transform::from_translation(PARTS[i as usize].offset)
}

/// Build the kicker figure (the render rig).
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

/// The kicker figure serialized to portable bytes.
pub fn figure_bytes() -> Vec<u8> {
    FigureApi::new().serialize(&build_figure())
}

/// A pitch rotation about X, as a local transform keeping the part's rest
/// offset.
fn pitch_transform(part: u32, angle: f32) -> Transform {
    Transform::new(PARTS[part as usize].offset, Quat::from_euler_xyz(angle, 0.0, 0.0), Vec3::ONE)
}

/// The kick clip serialized to portable bytes: pitch tracks, the eight phases,
/// and the `KickContact` event on the strike frame.
pub fn clip_bytes() -> Vec<u8> {
    use axiom_kernel::Tick;
    let mut api = AnimationApi::new();
    let clip = api.create_clip();
    for track in KICK_TRACKS {
        let keys: Vec<(Tick, Transform)> = track
            .keys
            .iter()
            .map(|&(frame, angle)| (Tick::new(u64::from(frame)), pitch_transform(track.part, angle)))
            .collect();
        api.add_track(clip, BoneId::from_raw(u64::from(track.part)), &keys).unwrap();
    }
    for (code, (_, start, end)) in KICK_PHASES.iter().enumerate() {
        api.add_phase(clip, Tick::new(u64::from(*start)), Tick::new(u64::from(*end)), code as u32)
            .unwrap();
    }
    api.add_event(clip, Tick::new(u64::from(CONTACT_FRAME)), KICK_CONTACT_CODE).unwrap();
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
        assert_eq!(phase_name(0), "ready");
        assert_eq!(phase_name(5), "strike");
        assert_eq!(phase_name(99), "-");
    }
}
