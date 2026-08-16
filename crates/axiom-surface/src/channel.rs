//! The closed seven-channel appearance vocabulary.

use axiom_field::{FieldType, FieldValue, Scalar};
use axiom_math::{Vec3, Vec4};

/// How many channels a surface has. Closedness is the whole point: the model is
/// fixed in Rust and parameterised by data, never extended at runtime.
pub const SURFACE_CHANNEL_COUNT: usize = 7;

/// The seven shading channels a renderer consumes.
///
/// The discriminant **is** the wire code and it indexes [`SurfaceChannel::ALL`],
/// the type table and the default table, so this order is the wire order and
/// must not be reshuffled.
///
/// **Decisions, recorded so they are not relitigated:**
///
/// * **[`SurfaceChannel::Metallic`] is a channel, not a BRDF.** It is carried,
///   digested and reported; no lighting model reads it yet. `SPEC-11`'s *"Resist
///   PBR scope creep"* still binds.
/// * **There is no transmission, subsurface, clear-coat or anisotropy channel.**
///   Adding a channel nothing can render is a capability nothing composes, which
///   is debt, not capability.
/// * **There is no texture-sampling channel.** A surface binds *fields*, not
///   images: one of the two backends cannot sample at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum SurfaceChannel {
    /// Linear RGBA albedo.
    BaseColor = 0,
    /// Perceptual roughness in `0..=1`.
    Roughness = 1,
    /// Metalness in `0..=1`. Carried and available; no shading reads it.
    Metallic = 2,
    /// A tangent-space normal, `+Z` out of the surface — bound directly, or
    /// derived from a height field by
    /// [`crate::SurfaceBuilder::normal_from_height`].
    Normal = 3,
    /// Linear RGB radiance added after shading. The `w` lane is ignored.
    Emission = 4,
    /// Coverage in `0..=1`.
    Opacity = 5,
    /// An object-space vertex offset. The one channel a vertex stage reads.
    Displacement = 6,
}

/// The type each channel's value carries, indexed by discriminant.
const CHANNEL_TYPES: [FieldType; SURFACE_CHANNEL_COUNT] = [
    FieldType::Vec4,
    FieldType::Scalar,
    FieldType::Scalar,
    FieldType::Vec3,
    FieldType::Vec4,
    FieldType::Scalar,
    FieldType::Vec3,
];

/// The value each channel holds when nobody binds it: an opaque white albedo, a
/// half-rough non-metal, a flat tangent-space normal, no emission, full opacity
/// and no displacement. Chosen so an unbound surface is the engine's existing
/// default material rather than a black hole.
const CHANNEL_DEFAULTS: [FieldValue; SURFACE_CHANNEL_COUNT] = [
    FieldValue::vec4(Vec4::new(1.0, 1.0, 1.0, 1.0)),
    FieldValue::scalar(Scalar::new(0.5)),
    FieldValue::scalar(Scalar::new(0.0)),
    FieldValue::vec3(Vec3::new(0.0, 0.0, 1.0)),
    FieldValue::vec4(Vec4::new(0.0, 0.0, 0.0, 0.0)),
    FieldValue::scalar(Scalar::new(1.0)),
    FieldValue::vec3(Vec3::new(0.0, 0.0, 0.0)),
];

impl SurfaceChannel {
    /// Every channel, in discriminant order. The array **is** the decode table
    /// and its index **is** the channel code.
    pub const ALL: [SurfaceChannel; SURFACE_CHANNEL_COUNT] = [
        SurfaceChannel::BaseColor,
        SurfaceChannel::Roughness,
        SurfaceChannel::Metallic,
        SurfaceChannel::Normal,
        SurfaceChannel::Emission,
        SurfaceChannel::Opacity,
        SurfaceChannel::Displacement,
    ];

    /// The wire code — the discriminant, which is also the table index.
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// The channel's index into a surface's binding array.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The single bit this channel occupies in a channel bitset, such as
    /// [`crate::SurfaceRequirements::varying_channels`].
    pub const fn bit(self) -> u16 {
        1_u16 << (self as u16)
    }

    /// The [`FieldType`] every value of this channel must carry. A binding whose
    /// type disagrees is rejected by [`crate::Surface::validate`].
    pub const fn ty(self) -> FieldType {
        CHANNEL_TYPES[self as usize]
    }

    /// The value this channel holds when nobody binds it.
    pub const fn default_value(self) -> FieldValue {
        CHANNEL_DEFAULTS[self as usize]
    }

    /// The channel a wire code names, or `None` if the code names no channel.
    /// Table lookup, so an unknown code is a bounds miss rather than a branch.
    pub fn from_code(code: u16) -> Option<SurfaceChannel> {
        SurfaceChannel::ALL.get(code as usize).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_their_table_indices() {
        assert_eq!(SurfaceChannel::BaseColor as u16, 0);
        assert_eq!(SurfaceChannel::Roughness as u16, 1);
        assert_eq!(SurfaceChannel::Metallic as u16, 2);
        assert_eq!(SurfaceChannel::Normal as u16, 3);
        assert_eq!(SurfaceChannel::Emission as u16, 4);
        assert_eq!(SurfaceChannel::Opacity as u16, 5);
        assert_eq!(SurfaceChannel::Displacement as u16, 6);
        SurfaceChannel::ALL.iter().enumerate().for_each(|(index, channel)| {
            assert_eq!(channel.code() as usize, index);
            assert_eq!(channel.index(), index);
            assert_eq!(channel.bit(), 1_u16 << index);
        });
    }

    #[test]
    fn the_vocabulary_is_closed_at_seven() {
        assert_eq!(SURFACE_CHANNEL_COUNT, 7);
        assert_eq!(SurfaceChannel::ALL.len(), SURFACE_CHANNEL_COUNT);
        let distinct: std::collections::BTreeSet<SurfaceChannel> =
            SurfaceChannel::ALL.iter().copied().collect();
        assert_eq!(distinct.len(), SURFACE_CHANNEL_COUNT);
    }

    #[test]
    fn every_channel_declares_the_type_it_carries() {
        assert_eq!(SurfaceChannel::BaseColor.ty(), FieldType::Vec4);
        assert_eq!(SurfaceChannel::Roughness.ty(), FieldType::Scalar);
        assert_eq!(SurfaceChannel::Metallic.ty(), FieldType::Scalar);
        assert_eq!(SurfaceChannel::Normal.ty(), FieldType::Vec3);
        assert_eq!(SurfaceChannel::Emission.ty(), FieldType::Vec4);
        assert_eq!(SurfaceChannel::Opacity.ty(), FieldType::Scalar);
        assert_eq!(SurfaceChannel::Displacement.ty(), FieldType::Vec3);
    }

    #[test]
    fn every_default_is_the_type_its_channel_declares() {
        SurfaceChannel::ALL.iter().for_each(|channel| {
            assert_eq!(channel.default_value().ty(), channel.ty());
        });
        assert_eq!(
            SurfaceChannel::BaseColor.default_value(),
            FieldValue::vec4(Vec4::new(1.0, 1.0, 1.0, 1.0))
        );
        assert_eq!(
            SurfaceChannel::Opacity.default_value(),
            FieldValue::scalar(Scalar::new(1.0))
        );
        assert_eq!(
            SurfaceChannel::Normal.default_value(),
            FieldValue::vec3(Vec3::new(0.0, 0.0, 1.0))
        );
        assert_eq!(
            SurfaceChannel::Displacement.default_value(),
            FieldValue::vec3(Vec3::ZERO)
        );
        assert_eq!(
            SurfaceChannel::Emission.default_value(),
            FieldValue::vec4(Vec4::ZERO)
        );
        assert_eq!(
            SurfaceChannel::Metallic.default_value(),
            FieldValue::scalar(Scalar::new(0.0))
        );
        assert_eq!(
            SurfaceChannel::Roughness.default_value(),
            FieldValue::scalar(Scalar::new(0.5))
        );
    }

    #[test]
    fn a_known_code_decodes_and_an_unknown_one_does_not() {
        assert_eq!(SurfaceChannel::from_code(0), Some(SurfaceChannel::BaseColor));
        assert_eq!(SurfaceChannel::from_code(6), Some(SurfaceChannel::Displacement));
        assert_eq!(SurfaceChannel::from_code(7), None);
        assert_eq!(SurfaceChannel::from_code(u16::MAX), None);
    }

    #[test]
    fn channels_order_by_their_code() {
        assert!(SurfaceChannel::BaseColor < SurfaceChannel::Displacement);
        assert_ne!(SurfaceChannel::Roughness, SurfaceChannel::Metallic);
    }
}
