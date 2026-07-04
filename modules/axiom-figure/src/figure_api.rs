//! [`FigureApi`]: the figure module's single behavioral facade.

use axiom_kernel::{BinaryReader, BinaryWriter};
use axiom_math::Transform;

use crate::definition::FigureDefinition;
use crate::figure_error::{FigureError, FigureResult};
use crate::posed_part::PosedPart;

/// The stateless facade over the figure mechanism: validate and round-trip a
/// [`FigureDefinition`], and pose a figure by pairing its per-part render boxes
/// with world transforms an app has already resolved (from an
/// `axiom-animation` model pose). It never touches the animation module — an app
/// drives the skeleton and hands the resulting world transforms here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FigureApi;

impl FigureApi {
    /// Construct the facade.
    pub const fn new() -> Self {
        Self
    }

    /// Validate a figure's parent-before-child hierarchy.
    pub fn validate(&self, figure: &FigureDefinition) -> FigureResult<()> {
        figure.validate()
    }

    /// Encode a figure to a portable byte buffer.
    pub fn serialize(&self, figure: &FigureDefinition) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        figure.write_to(&mut writer);
        writer.into_bytes()
    }

    /// Decode a figure produced by [`FigureApi::serialize`], then validate it.
    /// Fails with `MalformedData` if the bytes cannot be decoded, or `BadParent`
    /// if the decoded hierarchy is illegal.
    pub fn deserialize(&self, bytes: &[u8]) -> FigureResult<FigureDefinition> {
        FigureDefinition::read_from(&mut BinaryReader::new(bytes))
            .map_err(|_| FigureError::MalformedData)
            .and_then(|figure| figure.validate().map(|()| figure))
    }

    /// Pose a figure: pair each part's render box/tag with the matching
    /// world-space transform. `world_transforms` must have exactly one transform
    /// per part (the app resolves these from an animation model pose, in part
    /// order), else `TransformCountMismatch`.
    pub fn posed_parts(
        &self,
        figure: &FigureDefinition,
        world_transforms: &[Transform],
    ) -> FigureResult<Vec<PosedPart>> {
        (figure.part_count() == world_transforms.len())
            .then_some(())
            .ok_or(FigureError::TransformCountMismatch)
            .map(|()| {
                figure
                    .parts()
                    .iter()
                    .zip(world_transforms.iter())
                    .map(|(part, &world)| {
                        // The part pivots at its joint (`world`); the box is
                        // centered along the segment by baking its local offset
                        // into the world transform it is drawn at.
                        let box_world =
                            Transform::combine(world, Transform::from_translation(part.box_offset));
                        PosedPart::new(box_world, part.box_size, part.tag)
                    })
                    .collect()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::FigurePart;
    use axiom_math::Vec3;

    fn defaulted<T: Default>() -> T {
        T::default()
    }

    fn two_part_figure() -> FigureDefinition {
        FigureDefinition::new(vec![
            FigurePart::root(
                Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
                Vec3::new(0.4, 0.6, 0.4),
                Vec3::ZERO,
                1,
            ),
            FigurePart::child(
                0,
                Transform::from_translation(Vec3::new(0.0, -0.5, 0.0)),
                Vec3::new(0.2, 0.5, 0.2),
                Vec3::new(0.0, -0.25, 0.0),
                2,
            ),
        ])
    }

    #[test]
    fn new_and_default_agree() {
        assert_eq!(FigureApi::new(), FigureApi);
        assert_eq!(defaulted::<FigureApi>(), FigureApi::new());
    }

    #[test]
    fn validate_serialize_deserialize_round_trip() {
        let api = FigureApi::new();
        let figure = two_part_figure();
        assert_eq!(api.validate(&figure), Ok(()));
        let bytes = api.serialize(&figure);
        assert_eq!(api.deserialize(&bytes).unwrap(), figure);
    }

    #[test]
    fn deserialize_rejects_garbage_and_illegal_hierarchy() {
        let api = FigureApi::new();
        assert_eq!(api.deserialize(&[0xFF]), Err(FigureError::MalformedData));
        // A structurally-decodable but illegal figure (child before parent).
        let bad = FigureDefinition::new(vec![FigurePart::child(5, Transform::IDENTITY, Vec3::new(1.0, 1.0, 1.0), Vec3::ZERO, 0)]);
        let bytes = api.serialize(&bad);
        assert_eq!(api.deserialize(&bytes), Err(FigureError::BadParent));
    }

    #[test]
    fn posed_parts_pairs_boxes_with_world_transforms() {
        let api = FigureApi::new();
        let figure = two_part_figure();
        let world = [
            Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
            Transform::from_translation(Vec3::new(0.0, 0.5, 0.0)),
        ];
        let posed = api.posed_parts(&figure, &world).unwrap();
        assert_eq!(posed.len(), 2);
        assert_eq!(posed[0].transform.translation, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(posed[0].box_size, Vec3::new(0.4, 0.6, 0.4));
        assert_eq!(posed[1].tag, 2);
    }

    #[test]
    fn posed_parts_rejects_length_mismatch() {
        let api = FigureApi::new();
        let figure = two_part_figure();
        assert_eq!(
            api.posed_parts(&figure, &[Transform::IDENTITY]),
            Err(FigureError::TransformCountMismatch)
        );
    }
}
